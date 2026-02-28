//! Command-line dispatcher (":" commands).

use super::{
    container_cmd, dashboard_cmd, git_cmd, image_cmd, keymap_cmd, layout_cmd, logs_cmd,
    network_cmd, registry_cmd, server_cmd, set_cmd, sidebar_cmd, templates_cmd, theme_cmd,
    volume_cmd,
};
use crate::docker::DockerCfg;
use crate::ui::actions::{
    service_name_from_label_list, stack_compose_dirs, template_name_from_stack,
};
use crate::ui::core::requests::{ActionRequest, Connection, ShellConfirm};
use crate::ui::core::runtime::{current_docker_cmd_from_app, current_runner_from_app};
use crate::ui::core::types::{DeployMarker, InspectTarget, StackUpdateService};
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{
    ActiveView, MsgLevel, ShellFocus, ShellView, TemplatesKind, shell_begin_confirm,
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

pub(crate) fn parse_cmdline_tokens(input: &str) -> Result<Vec<String>, String> {
    crate::shell_parse::parse_shell_tokens(input)
}

pub(in crate::ui) fn stack_update(
    app: &mut App,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
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
    let mut services: HashMap<String, StackUpdateService> = HashMap::new();
    for c in app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(target.as_str()))
    {
        let svc = service_name_from_label_list(&c.labels, Some(target.as_str()), &c.name);
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
    let services: Vec<StackUpdateService> = services.into_values().collect();
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

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn execute_cmdline(
    app: &mut App,
    cmdline: &str,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
    refresh_interval_tx: &watch::Sender<Duration>,
    refresh_pause_tx: &watch::Sender<bool>,
    image_update_limit_tx: &watch::Sender<usize>,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return;
    }
    let cmdline = cmdline.trim_start_matches(':').trim();
    let cmdline_full = cmdline.to_string();

    let tokens = match parse_cmdline_tokens(cmdline) {
        Ok(v) => v,
        Err(e) => {
            app.set_warn(format!("invalid command line: {e}"));
            return;
        }
    };
    let mut it = tokens.iter().map(|s| s.as_str());
    let Some(cmd_raw) = it.next() else {
        return;
    };
    let (cmd, force) = if cmd_raw == "!" {
        let Some(next) = it.next() else {
            app.set_warn("usage: :! <command>");
            return;
        };
        (next, true)
    } else if let Some(rest) = cmd_raw.strip_prefix('!') {
        if rest.is_empty() {
            app.set_warn("usage: :! <command>");
            return;
        }
        (rest, true)
    } else if let Some(stripped) = cmd_raw.strip_suffix('!') {
        (stripped, true)
    } else {
        (cmd_raw, false)
    };

    match cmd {
        "q" => {
            if force {
                app.should_quit = true;
            } else {
                app.shell_cmdline.mode = true;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.shell_cmdline.confirm = Some(ShellConfirm {
                    label: "quit".to_string(),
                    cmdline: cmdline_full,
                });
            }
            return;
        }
        "?" | "help" => {
            // Ensure we don't get "stuck" in command-line mode while the Help view is active.
            // Otherwise 'q' is treated as input and won't close Help.
            app.shell_cmdline.mode = false;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            app.shell_help.return_view = app.shell_view;
            app.shell_view = ShellView::Help;
            app.shell_focus = ShellFocus::List;
            app.shell_help.scroll = 0;
            return;
        }
        "messages" | "msgs" => {
            let sub = it.next().unwrap_or("");
            if sub == "copy" {
                app.messages_copy_selected();
                return;
            }
            let (force, wants_save) = if sub == "save!" {
                (true, true)
            } else if sub == "save" {
                (false, true)
            } else {
                (false, false)
            };
            if wants_save {
                let rest: Vec<&str> = it.collect();
                let path = rest.join(" ").trim().to_string();
                if path.is_empty() {
                    app.set_warn("usage: :messages save <file>");
                } else {
                    app.messages_save(&path, force);
                }
                return;
            }
            // Messages is a full-screen view; leaving cmdline mode avoids confusing key handling.
            app.shell_cmdline.mode = false;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            if app.shell_view == ShellView::Messages {
                app.back_from_full_view();
            } else {
                app.mark_messages_seen();
                app.shell_msgs.return_view = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs.scroll = usize::MAX;
                app.shell_msgs.hscroll = 0;
            }
            return;
        }
        "log" => {
            let sub = it.next().unwrap_or("");
            if sub != "dock" {
                app.set_warn("usage: :log dock");
                return;
            }
            let height_raw = it.next().unwrap_or("");
            if !height_raw.is_empty() {
                match height_raw.parse::<u16>() {
                    Ok(h) if (3..=12).contains(&h) => {
                        app.log_dock_height = h;
                        app.log_dock_enabled = true;
                    }
                    _ => {
                        app.set_warn("usage: :log dock [3..12]");
                        return;
                    }
                }
            } else {
                app.log_dock_enabled = !app.log_dock_enabled;
            }
            if app.log_dock_enabled {
                app.shell_msgs.scroll = usize::MAX;
                app.shell_msgs.hscroll = 0;
            }
            if app.shell_view == ShellView::Messages {
                app.back_from_full_view();
            } else if !app.log_dock_enabled && app.shell_focus == ShellFocus::Dock {
                app.shell_focus = ShellFocus::List;
            }
            app.persist_config();
            return;
        }
        "ack" => {
            let sub = it.next().unwrap_or("");
            if sub == "all" {
                app.container_action_error.clear();
                app.image_action_error.clear();
                app.volume_action_error.clear();
                app.network_action_error.clear();
                app.template_action_error.clear();
                app.net_template_action_error.clear();
                app.conn_error = None;
                app.last_error = None;
                app.dashboard.error = None;
                app.logs.error = None;
                app.inspect.error = None;
                app.refresh_error_streak = 0;
                app.refresh_pause_reason = None;
                app.mark_messages_seen();
                app.set_info("cleared all action error markers");
                return;
            }
            match app.shell_view {
                ShellView::Dashboard => {}
                ShellView::Stacks => {}
                ShellView::Containers => {
                    let ids: Vec<String> = if !app.marked.is_empty() {
                        app.marked.iter().cloned().collect()
                    } else {
                        app.selected_container()
                            .map(|c| vec![c.id.clone()])
                            .unwrap_or_default()
                    };
                    for id in ids {
                        app.container_action_error.remove(&id);
                    }
                }
                ShellView::Images => {
                    let keys: Vec<String> = if !app.marked_images.is_empty() {
                        app.marked_images.iter().cloned().collect()
                    } else {
                        app.selected_image()
                            .map(|img| vec![App::image_row_key(img)])
                            .unwrap_or_default()
                    };
                    for k in keys {
                        app.image_action_error.remove(&k);
                    }
                }
                ShellView::Volumes => {
                    let names: Vec<String> = if !app.marked_volumes.is_empty() {
                        app.marked_volumes.iter().cloned().collect()
                    } else {
                        app.selected_volume()
                            .map(|v| vec![v.name.clone()])
                            .unwrap_or_default()
                    };
                    for n in names {
                        app.volume_action_error.remove(&n);
                    }
                }
                ShellView::Networks => {
                    let ids: Vec<String> = if !app.marked_networks.is_empty() {
                        app.marked_networks.iter().cloned().collect()
                    } else {
                        app.selected_network()
                            .map(|n| vec![n.id.clone()])
                            .unwrap_or_default()
                    };
                    for id in ids {
                        app.network_action_error.remove(&id);
                    }
                }
                ShellView::Templates => match app.templates_state.kind {
                    TemplatesKind::Stacks => {
                        let name = app.selected_template().map(|t| t.name.clone());
                        if let Some(name) = name {
                            app.template_action_error.remove(&name);
                        }
                    }
                    TemplatesKind::Networks => {
                        let name = app.selected_net_template().map(|t| t.name.clone());
                        if let Some(name) = name {
                            app.net_template_action_error.remove(&name);
                        }
                    }
                },
                ShellView::Logs
                | ShellView::Inspect
                | ShellView::Help
                | ShellView::Messages
                | ShellView::Registries
                | ShellView::ThemeSelector => {}
            }
            app.set_info("cleared action error marker(s) for selection");
            return;
        }
        "refresh" => {
            if app.shell_view == ShellView::Templates {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => app.refresh_templates(),
                    TemplatesKind::Networks => app.refresh_net_templates(),
                }
            } else {
                app.refresh_now(
                    refresh_tx,
                    dash_refresh_tx,
                    dash_all_refresh_tx,
                    refresh_pause_tx,
                );
            }
            return;
        }
        "theme" => {
            let sub = it.next().unwrap_or("");
            if sub.is_empty() || sub == "help" {
                app.set_info(format!("active theme: {}", app.theme_name));
                app.set_info("usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>");
                if sub.is_empty() {
                    return;
                }
            }
            match sub {
                "list" => {
                    app.open_theme_selector();
                }
                "use" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme use <name>");
                        return;
                    };
                    if let Err(e) = theme_cmd::set_theme(app, name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                "new" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme new <name>");
                        return;
                    };
                    if let Err(e) = theme_cmd::new_theme(app, name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                "edit" => {
                    let name = it
                        .next()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| app.theme_name.clone());
                    if let Err(e) = theme_cmd::edit_theme(app, &name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                "rm" | "del" | "delete" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme rm <name>");
                        return;
                    };
                    if !force {
                        shell_begin_confirm(app, format!("theme rm {name}"), cmdline_full.clone());
                        return;
                    }
                    if let Err(e) = theme_cmd::delete_theme(app, name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                _ => {
                    app.set_warn(
                        "usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>",
                    );
                }
            }
            return;
        }
        "git" => {
            let args: Vec<&str> = it.collect();
            let _ = git_cmd::handle_git(app, &args);
            return;
        }
        "map" | "bind" => {
            let first = it.next().unwrap_or("");
            let rest: Vec<&str> = it.collect();
            let _ = keymap_cmd::handle_map(app, first, &rest);
            return;
        }
        "unmap" | "unbind" => {
            let first = it.next().unwrap_or("");
            let rest: Vec<&str> = it.collect();
            let _ = keymap_cmd::handle_unmap(app, first, &rest);
            return;
        }
        "container" | "ctr" | "containers" => {
            let sub = it.next().unwrap_or("");
            let mut args: Vec<&str> = Vec::new();
            if !sub.is_empty() {
                args.push(sub);
            }
            args.extend(it);
            let _ = container_cmd::handle_container(
                app,
                force,
                cmdline_full.clone(),
                &args,
                action_req_tx,
            );
            return;
        }
        "stack" | "stacks" => {
            let mut args: Vec<&str> = it.collect();
            if args.is_empty() {
                app.set_warn("usage: :stack [start|stop|restart|rm|check|update] [name] | :stacks running|all");
                return;
            }
            let sub = args.remove(0);
            match sub {
                "running" => {
                    app.set_main_view(ShellView::Stacks);
                    app.shell_focus = ShellFocus::List;
                    app.active_view = ActiveView::Stacks;
                    app.stacks_only_running = true;
                    app.rebuild_stacks();
                    return;
                }
                "all" => {
                    app.set_main_view(ShellView::Stacks);
                    app.shell_focus = ShellFocus::List;
                    app.active_view = ActiveView::Stacks;
                    app.stacks_only_running = false;
                    app.rebuild_stacks();
                    return;
                }
                "start" | "stop" | "restart" | "rm" | "remove" | "delete" | "check" | "updates" => {
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
                                action_req_tx,
                            );
                        }
                        "stop" => {
                            crate::ui::state::actions::exec_stack_action(
                                app,
                                crate::docker::ContainerAction::Stop,
                                name,
                                action_req_tx,
                            );
                        }
                        "restart" => {
                            crate::ui::state::actions::exec_stack_action(
                                app,
                                crate::docker::ContainerAction::Restart,
                                name,
                                action_req_tx,
                            );
                        }
                        "rm" | "remove" | "delete" => {
                            if force {
                                crate::ui::state::actions::exec_stack_action(
                                    app,
                                    crate::docker::ContainerAction::Remove,
                                    name,
                                    action_req_tx,
                                );
                            } else if let Some(name) = name {
                                shell_begin_confirm(app, format!("stack rm {name}"), cmdline_full);
                            } else if let Some(sel) =
                                app.selected_stack_entry().map(|s| s.name.clone())
                            {
                                shell_begin_confirm(app, format!("stack rm {sel}"), cmdline_full);
                            } else {
                                app.set_warn("no stack selected");
                            }
                        }
                        "check" | "updates" => {
                            let stack = if let Some(name) = name {
                                name.to_string()
                            } else if let Some(sel) =
                                app.selected_stack_entry().map(|s| s.name.clone())
                            {
                                sel
                            } else {
                                app.set_warn("no stack selected");
                                return;
                            };
                            let mut images: HashSet<String> = HashSet::new();
                            for c in app.containers.iter().filter(|c| {
                                stack_name_from_labels(&c.labels).as_deref() == Some(stack.as_str())
                            }) {
                                images.insert(c.image.clone());
                            }
                            crate::ui::actions::check_image_updates(
                                app,
                                images.into_iter().collect(),
                                action_req_tx,
                            );
                        }
                        _ => {}
                    }
                    return;
                }
                "update" | "up" => {
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
                        stack_update(app, action_req_tx, true, services_filter);
                        return;
                    }
                    if let Some(target) = name {
                        // Temporarily select the target stack if it exists.
                        if let Some(idx) = app.stacks.iter().position(|s| s.name == target) {
                            app.stacks_selected = idx;
                        }
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
                    let mut services: HashMap<String, StackUpdateService> = HashMap::new();
                    for c in app.containers.iter().filter(|c| {
                        stack_name_from_labels(&c.labels).as_deref() == Some(target.as_str())
                    }) {
                        let svc =
                            service_name_from_label_list(&c.labels, Some(target.as_str()), &c.name);
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
                    let services: Vec<StackUpdateService> = services.into_values().collect();
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
                    let _ = action_req_tx.send(ActionRequest::StackUpdate {
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
                    return;
                }
                _ => {
                    app.set_warn("usage: :stack [start|stop|restart|rm|check|update] [name] | :stacks running|all");
                    return;
                }
            }
        }
        _ => {}
    }

    if cmd == "image" || cmd == "img" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = image_cmd::handle_image(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "volume" || cmd == "vol" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = volume_cmd::handle_volume(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "network" || cmd == "net" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = network_cmd::handle_network(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "sidebar" {
        let sub = it.next().unwrap_or("toggle");
        let mut args: Vec<&str> = Vec::new();
        args.push(sub);
        args.extend(it);
        let _ = sidebar_cmd::handle_sidebar(app, &args);
        return;
    }

    if cmd == "ai" {
        if it.next().is_some() {
            app.set_warn("usage: :ai");
            return;
        }
        let _ = templates_cmd::handle_template_ai(app);
        return;
    }

    if cmd == "inspect" {
        app.enter_inspect(inspect_req_tx);
        return;
    }

    if cmd == "logs" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        if args.is_empty() && app.shell_view != ShellView::Logs {
            app.enter_logs(logs_req_tx);
            return;
        }
        let _ = logs_cmd::handle_logs(app, &args, logs_req_tx);
        return;
    }

    if cmd == "set" {
        let args: Vec<&str> = it.collect();
        let _ = set_cmd::handle_set(
            app,
            &args,
            refresh_interval_tx,
            image_update_limit_tx,
            logs_req_tx,
        );
        return;
    }

    if cmd == "layout" {
        let sub = it.next().unwrap_or("toggle");
        let mut args: Vec<&str> = Vec::new();
        args.push(sub);
        args.extend(it);
        let _ = layout_cmd::handle_layout(app, &args);
        return;
    }

    if cmd == "templates" {
        let args: Vec<&str> = it.collect();
        let _ = templates_cmd::handle_templates(app, &args);
        return;
    }

    if cmd == "registries" {
        let args: Vec<&str> = it.collect();
        let _ = registry_cmd::handle_registries(app, &args);
        return;
    }

    if cmd == "template" || cmd == "tpl" {
        let args: Vec<&str> = it.collect();
        let _ =
            templates_cmd::handle_template(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "registry" || cmd == "reg" {
        let args: Vec<&str> = it.collect();
        let _ = registry_cmd::handle_registry(app, force, &args, action_req_tx);
        return;
    }

    if matches!(cmd, "nettemplate" | "nettpl" | "ntpl" | "nt") {
        let args: Vec<&str> = it.collect();
        let _ = templates_cmd::handle_nettemplate(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "server" {
        let args: Vec<&str> = it.collect();
        let _ = server_cmd::handle_server(
            app,
            force,
            cmdline_full.clone(),
            &args,
            conn_tx,
            refresh_tx,
            dash_refresh_tx,
            dash_all_enabled_tx,
        );
        return;
    }

    if cmd == "dashboard" {
        let args: Vec<&str> = it.collect();
        let _ = dashboard_cmd::handle_dashboard(
            app,
            &args,
            conn_tx,
            refresh_tx,
            dash_refresh_tx,
            dash_all_refresh_tx,
            dash_all_enabled_tx,
        );
        return;
    }

    app.set_error(format!("unknown command: {cmd}"));
}
