use super::CmdlineCtx;
use crate::docker::DockerCfg;
use crate::ui::actions::{
    service_name_from_label_list, stack_compose_dirs, template_name_from_stack,
};
use crate::ui::core::requests::ActionRequest;
use crate::ui::core::runtime::{current_docker_cmd_from_app, current_runner_from_app};
use crate::ui::core::types::{DeployMarker, StackUpdateService};
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ActiveView, MsgLevel, ShellFocus, ShellView};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub(in crate::ui) fn stack_update(
    app: &mut App,
    action_req_tx: &tokio::sync::mpsc::UnboundedSender<ActionRequest>,
    force: bool,
    services_filter: Option<Vec<String>>,
) {
    let target = app.selected_stack_entry().map(|s| s.name.clone());
    let Some(target) = target else {
        app.set_warn("no stack selected");
        return;
    };
    if app.stack_update_inflight.contains_key(&target) {
        app.set_warn(format!("stack '{target}' is already updating"));
        return;
    }
    let tpl_name = template_name_from_stack(app, &target);
    let compose_dirs = stack_compose_dirs(app, &target, tpl_name.as_deref());
    if compose_dirs.is_empty() {
        app.set_warn("stack update: no compose dirs");
        return;
    }
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    if docker.docker_cmd.is_empty() {
        app.set_warn("no server configured");
        return;
    }
    let runner = current_runner_from_app(app);
    let services = collect_stack_services(app, &target, services_filter);
    if services.is_empty() && !force {
        app.set_warn("stack update: no services found");
        return;
    }
    app.stack_update_containers.insert(
        target.clone(),
        services
            .iter()
            .map(|svc| svc.container_id.clone())
            .collect(),
    );
    app.stack_update_inflight.insert(
        target.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.stack_update_error.remove(&target);
    let _ = action_req_tx.send(ActionRequest::StackUpdate {
        stack_name: target.clone(),
        runner,
        docker,
        compose_dirs,
        pull: true,
        dry: false,
        force,
        services,
    });
    let mut msg = format!("stack update {target}");
    if force {
        msg.push_str(" [all]");
    }
    app.set_info(msg);
    app.log_msg(MsgLevel::Info, format!("stack update started: {target}"));
}

pub(super) fn handle_stack_cmd<'a>(
    app: &mut App,
    cmd: &str,
    it: &mut impl Iterator<Item = &'a str>,
    force: bool,
    cmdline_full: &str,
    ctx: &CmdlineCtx<'_>,
) -> bool {
    if !matches!(cmd, "stack" | "stacks") {
        return false;
    }
    let mut args: Vec<&str> = it.collect();
    if args.is_empty() {
        app.set_warn(
            "usage: :stack [start|stop|restart|rm|check|update] [name] | :stacks running|all",
        );
        return true;
    }
    let sub = args.remove(0);
    match sub {
        "running" => {
            app.set_main_view(ShellView::Stacks);
            app.shell_focus = ShellFocus::List;
            app.active_view = ActiveView::Stacks;
            app.stacks_only_running = true;
            app.rebuild_stacks();
            true
        }
        "all" => {
            app.set_main_view(ShellView::Stacks);
            app.shell_focus = ShellFocus::List;
            app.active_view = ActiveView::Stacks;
            app.stacks_only_running = false;
            app.rebuild_stacks();
            true
        }
        "start" | "stop" | "restart" | "rm" | "remove" | "delete" | "check" | "updates" => {
            handle_stack_simple_action(app, sub, &args, force, cmdline_full, ctx);
            true
        }
        "update" | "up" => {
            handle_stack_update_cmd(app, &args, ctx);
            true
        }
        _ => {
            app.set_warn(
                "usage: :stack [start|stop|restart|rm|check|update] [name] | :stacks running|all",
            );
            true
        }
    }
}

fn handle_stack_simple_action(
    app: &mut App,
    sub: &str,
    args: &[&str],
    force: bool,
    cmdline_full: &str,
    ctx: &CmdlineCtx<'_>,
) {
    let name = args.first().copied();
    if args.len() > 1 {
        app.set_warn("usage: :stack (start|stop|restart|rm|check) [name]");
        return;
    }
    match sub {
        "start" => {
            crate::ui::state::actions::exec_stack_action(
                app,
                crate::docker::ContainerAction::Start,
                name,
                ctx.action_req_tx,
            );
        }
        "stop" => {
            crate::ui::state::actions::exec_stack_action(
                app,
                crate::docker::ContainerAction::Stop,
                name,
                ctx.action_req_tx,
            );
        }
        "restart" => {
            crate::ui::state::actions::exec_stack_action(
                app,
                crate::docker::ContainerAction::Restart,
                name,
                ctx.action_req_tx,
            );
        }
        "rm" | "remove" | "delete" => {
            if force {
                crate::ui::state::actions::exec_stack_action(
                    app,
                    crate::docker::ContainerAction::Remove,
                    name,
                    ctx.action_req_tx,
                );
            } else if let Some(name) = name {
                crate::ui::state::shell_types::shell_begin_confirm(
                    app,
                    format!("stack rm {name}"),
                    cmdline_full.to_string(),
                );
            } else if let Some(sel) = app.selected_stack_entry().map(|s| s.name.clone()) {
                crate::ui::state::shell_types::shell_begin_confirm(
                    app,
                    format!("stack rm {sel}"),
                    cmdline_full.to_string(),
                );
            } else {
                app.set_warn("no stack selected");
            }
        }
        "check" | "updates" => {
            let stack = if let Some(name) = name {
                name.to_string()
            } else if let Some(sel) = app.selected_stack_entry().map(|s| s.name.clone()) {
                sel
            } else {
                app.set_warn("no stack selected");
                return;
            };
            let mut images: HashSet<String> = HashSet::new();
            for c in app
                .containers
                .iter()
                .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack.as_str()))
            {
                images.insert(c.image.clone());
            }
            crate::ui::actions::check_image_updates(
                app,
                images.into_iter().collect(),
                ctx.action_req_tx,
            );
        }
        _ => {}
    }
}

fn handle_stack_update_cmd(app: &mut App, args: &[&str], ctx: &CmdlineCtx<'_>) {
    let mut name: Option<String> = None;
    let mut pull = true;
    let mut all = false;
    let mut dry = false;
    let mut services_filter: Option<Vec<String>> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        match arg {
            "--pull" | "pull" => pull = true,
            "--no-pull" | "nopull" => pull = false,
            "--all" | "all" => all = true,
            "--dry" | "dry" => dry = true,
            "--services" | "services" => {
                let Some(next) = args.get(i + 1).copied() else {
                    app.set_warn("usage: :stack update [name] [--pull|--no-pull] [--services a,b]");
                    return;
                };
                services_filter = Some(
                    next.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
                i += 1;
            }
            _ if name.is_none() => name = Some(arg.to_string()),
            _ => {}
        }
        i += 1;
    }
    if all {
        if let Some(name) = name {
            app.set_warn(format!("unexpected target for --all: {name}"));
            return;
        }
        stack_update(app, ctx.action_req_tx, true, services_filter);
        return;
    }
    if let Some(target) = name
        && let Some(idx) = app.stacks.iter().position(|s| s.name == target)
    {
        app.stacks_selected = idx;
    }
    let target = app.selected_stack_entry().map(|s| s.name.clone());
    let Some(target) = target else {
        app.set_warn("no stack selected");
        return;
    };
    if app.stack_update_inflight.contains_key(&target) {
        app.set_warn(format!("stack '{target}' is already updating"));
        return;
    }
    let tpl_name = template_name_from_stack(app, &target);
    let compose_dirs = stack_compose_dirs(app, &target, tpl_name.as_deref());
    if compose_dirs.is_empty() {
        app.set_warn("stack update: no compose dirs");
        return;
    }
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    if docker.docker_cmd.is_empty() {
        app.set_warn("no server configured");
        return;
    }
    let runner = current_runner_from_app(app);
    let services = collect_stack_services(app, &target, services_filter.clone());
    if services.is_empty() && !all {
        app.set_warn("stack update: no services found");
        return;
    }
    app.stack_update_containers.insert(
        target.clone(),
        services
            .iter()
            .map(|svc| svc.container_id.clone())
            .collect(),
    );
    app.stack_update_inflight.insert(
        target.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.stack_update_error.remove(&target);
    let _ = ctx.action_req_tx.send(ActionRequest::StackUpdate {
        stack_name: target.clone(),
        runner,
        docker,
        compose_dirs,
        pull,
        dry,
        force: all,
        services,
    });
    let mut msg = format!("stack update {target}");
    if pull {
        msg.push_str(" [pull]");
    }
    if dry {
        msg.push_str(" [dry]");
    }
    if all {
        msg.push_str(" [all]");
    }
    if let Some(filter) = services_filter.as_ref() {
        msg.push_str(&format!(" [services={}]", filter.join(",")));
    }
    app.set_info(msg);
    app.log_msg(MsgLevel::Info, format!("stack update started: {target}"));
}

fn collect_stack_services(
    app: &App,
    target: &str,
    services_filter: Option<Vec<String>>,
) -> Vec<StackUpdateService> {
    let mut services: HashMap<String, StackUpdateService> = HashMap::new();
    for c in app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(target))
    {
        let svc = service_name_from_label_list(&c.labels, Some(target), &c.name);
        services.entry(svc.clone()).or_insert(StackUpdateService {
            name: svc,
            container_id: c.id.clone(),
            image: c.image.clone(),
        });
    }
    if let Some(filter) = services_filter.as_ref() {
        let allow: HashSet<String> = filter.iter().map(|s| s.to_string()).collect();
        services.retain(|name, _| allow.contains(name));
    }
    services.into_values().collect()
}
