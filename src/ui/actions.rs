//! Imperative UI actions invoked by shortcuts and menus.

use crate::docker::{ContainerAction, DockerCfg};
use crate::ui::core::requests::ActionRequest;
use crate::ui::core::runtime::{
    current_docker_cmd_from_app, current_runner_from_app, current_server_label,
};
use crate::ui::core::types::{DeployMarker, InspectTarget};
use crate::ui::helpers::{ensure_template_id, shell_single_quote};
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::render::utils::{is_container_stopped, shell_escape_sh_arg};
use crate::ui::state::actions as state_actions;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{
    MsgLevel, ShellAction, ShellInteractive, ShellView, TemplatesKind, shell_begin_confirm,
};
use std::time::Instant;
use tokio::sync::mpsc;

fn shell_is_safe_token(s: &str) -> bool {
    // For interactive shells we only accept simple command tokens.
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
}

fn shell_escape_double_quoted(s: &str) -> String {
    // Escape for inclusion inside double quotes in a POSIX shell script.
    // We escape: backslash, double quote, dollar, backtick.
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' | '"' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

pub(in crate::ui) fn open_console(app: &mut App, user: Option<&str>, shell: &str) {
    let Some(c) = app.selected_container() else {
        app.set_warn("no container selected");
        return;
    };
    if is_container_stopped(&c.status) {
        app.set_warn("container is not running");
        return;
    }
    if !shell_is_safe_token(shell) {
        app.set_warn("invalid shell");
        return;
    }
    let docker_cmd = current_docker_cmd_from_app(app).to_shell();
    let id = shell_single_quote(&c.id);
    let server = current_server_label(app);
    // Bash interprets prompt escapes like \\e and needs \\[ \\] wrappers for correct line editing.
    let ps1_bash = format!(
        "\\[\\e[37m\\]docker:\\[\\e[0m\\]\\[\\e[32m\\]{}\\[\\e[37m\\]@{}\\[\\e[0m\\]$ ",
        c.name, server
    );
    let ps1_bash = shell_single_quote(&ps1_bash);

    let user_part = user
        .filter(|u| !u.trim().is_empty())
        .map(|u| format!("-u {}", shell_single_quote(u.trim())))
        .unwrap_or_default();

    let shell_cmd = if shell == "bash" {
        format!("env PS1={ps1_bash} bash --noprofile --norc -i")
    } else if shell == "sh" {
        // POSIX sh typically does NOT interpret \\e-style escapes in PS1. We set PS1 via printf
        // using %b so that \\033 sequences become real ESC bytes, then exec an interactive sh.
        // Important: avoid nested single quotes here, because this command is embedded into other
        // shell layers (ssh/sh -lc).
        let ps1_sh_raw = format!(
            "\\033[37mdocker:\\033[0m\\033[32m{}\\033[37m@{}\\033[0m\\$ ",
            c.name, server
        );
        let ps1_sh = shell_escape_double_quoted(&ps1_sh_raw);
        format!("sh -lc 'export PS1=\"$(printf \"%b\" \"{ps1_sh}\")\"; exec sh -i'")
    } else {
        // Best-effort generic token. Prompt coloring depends on the shell.
        format!("env PS1={ps1_bash} {shell}")
    };

    let cmd = if user_part.is_empty() {
        format!("{docker_cmd} exec -it {id} {shell_cmd}")
    } else {
        format!("{docker_cmd} exec -it {user_part} {id} {shell_cmd}")
    };
    app.shell_pending_interactive = Some(ShellInteractive::RunCommand { cmd });
}

pub(in crate::ui) fn exec_container_action(
    app: &mut App,
    action: ContainerAction,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    state_actions::exec_container_action(app, action, action_req_tx)
}

pub(in crate::ui) fn check_image_updates(
    app: &mut App,
    images: Vec<String>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let mut queued = 0usize;
    for image in images {
        let Some(normalized) =
            crate::ui::state::image_updates::resolve_image_ref_for_updates(app, &image)
        else {
            app.log_msg(
                MsgLevel::Warn,
                format!("image update skipped (unresolved ref): {image}"),
            );
            continue;
        };
        let key = normalized.reference.clone();
        if app.image_updates_inflight.contains(&key) {
            continue;
        }
        app.note_rate_limit_request(&key);
        app.image_updates_inflight.insert(key.clone());
        let _ = action_req_tx.send(ActionRequest::ImageUpdateCheck {
            image: key.clone(),
            debug: app.image_update_debug,
        });
        app.log_msg(MsgLevel::Info, format!("image update queued: {key}"));
        queued += 1;
    }
    if queued == 0 {
        app.set_warn("no images to check");
    } else {
        app.set_info(format!("checking {queued} image(s)"));
    }
    app.save_local_state();
}

pub(in crate::ui) fn registry_test_selected(
    app: &mut App,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let Some(entry) = app.registries_cfg.registries.get(app.registries_selected) else {
        app.set_warn("no registry selected");
        return;
    };
    let host = entry.host.clone();
    let test_repo = entry.test_repo.clone();
    let auth = match app.registry_auth_for_host(&host) {
        Ok(v) => v,
        Err(e) => {
            app.set_warn(format!("{e:#}"));
            return;
        }
    };
    app.set_info(format!("testing registry {host}"));
    let _ = action_req_tx.send(ActionRequest::RegistryTest {
        host,
        auth,
        test_repo,
    });
}

pub(in crate::ui) fn execute_action(
    app: &mut App,
    a: ShellAction,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    match a {
        ShellAction::Inspect => {
            app.enter_inspect(inspect_req_tx);
        }
        ShellAction::Logs => {
            app.enter_logs(logs_req_tx);
        }
        ShellAction::Start => {
            if app.shell_view == ShellView::Stacks {
                state_actions::exec_stack_action(app, ContainerAction::Start, None, action_req_tx);
            } else {
                exec_container_action(app, ContainerAction::Start, action_req_tx);
            }
        }
        ShellAction::Stop => {
            if app.shell_view == ShellView::Stacks {
                state_actions::exec_stack_action(app, ContainerAction::Stop, None, action_req_tx);
            } else {
                exec_container_action(app, ContainerAction::Stop, action_req_tx);
            }
        }
        ShellAction::Restart => {
            if app.shell_view == ShellView::Stacks {
                state_actions::exec_stack_action(
                    app,
                    ContainerAction::Restart,
                    None,
                    action_req_tx,
                );
            } else {
                exec_container_action(app, ContainerAction::Restart, action_req_tx);
            }
        }
        ShellAction::Delete => {
            if app.shell_view == ShellView::Stacks {
                let name = app.selected_stack_entry().map(|s| s.name.clone());
                if let Some(name) = name {
                    shell_begin_confirm(
                        app,
                        format!("stack rm {name}"),
                        format!("stack rm {name}"),
                    );
                } else {
                    app.set_warn("no stack selected");
                }
            } else {
                shell_begin_confirm(app, "container rm", "container rm");
            }
        }
        ShellAction::StackUpdate => {
            crate::ui::commands::cmdline_cmd::stack_update(app, action_req_tx, false, None);
        }
        ShellAction::StackUpdateAll => {
            crate::ui::commands::cmdline_cmd::stack_update(app, action_req_tx, true, None);
        }
        ShellAction::Console => {
            open_console(app, Some("root"), "bash");
        }
        ShellAction::ImageUntag => shell_begin_confirm(app, "image untag", "image untag"),
        ShellAction::ImageForceRemove => shell_begin_confirm(app, "image rm", "image rm"),
        ShellAction::VolumeRemove => shell_begin_confirm(app, "volume rm", "volume rm"),
        ShellAction::NetworkRemove => {
            let any_removable = if !app.marked_networks.is_empty() {
                app.marked_networks
                    .iter()
                    .any(|id| !app.is_system_network_id(id))
            } else {
                app.selected_network()
                    .map(|n| !App::is_system_network(n))
                    .unwrap_or(false)
            };
            if !any_removable {
                app.set_warn("no removable networks selected");
                return;
            }
            shell_begin_confirm(app, "network rm", "network rm");
        }
        ShellAction::TemplateEdit => {
            edit_selected_template(app);
        }
        ShellAction::TemplateNew => {
            app.shell_cmdline.mode = true;
            let prompt = match app.templates_state.kind {
                TemplatesKind::Stacks => "template add ",
                TemplatesKind::Networks => "nettemplate add ",
            };
            crate::ui::set_text_and_cursor(
                &mut app.shell_cmdline.input,
                &mut app.shell_cmdline.cursor,
                prompt.to_string(),
            );
            app.shell_cmdline.confirm = None;
        }
        ShellAction::TemplateDelete => {
            let name = match app.templates_state.kind {
                TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
            };
            if let Some(name) = name {
                shell_begin_confirm(
                    app,
                    format!(
                        "{} rm {name}",
                        match app.templates_state.kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                    format!(
                        "{} rm {name}",
                        match app.templates_state.kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                );
            } else {
                app.set_warn("no template selected");
            }
        }
        ShellAction::TemplateDeploy => match app.templates_state.kind {
            TemplatesKind::Stacks => {
                if let Some(name) = app.selected_template().map(|t| t.name.clone()) {
                    deploy_template(app, &name, false, false, action_req_tx);
                } else {
                    app.set_warn("no template selected");
                }
            }
            TemplatesKind::Networks => {
                if let Some(name) = app.selected_net_template().map(|t| t.name.clone()) {
                    deploy_net_template(app, &name, false, action_req_tx);
                } else {
                    app.set_warn("no template selected");
                }
            }
        },
        ShellAction::TemplateRedeploy => match app.templates_state.kind {
            TemplatesKind::Stacks => {
                if let Some(name) = app.selected_template().map(|t| t.name.clone()) {
                    deploy_template(app, &name, true, true, action_req_tx);
                } else {
                    app.set_warn("no template selected");
                }
            }
            TemplatesKind::Networks => {
                app.set_warn("redeploy is only available for stack templates");
            }
        },
        ShellAction::TemplateAi => {
            let _ = crate::ui::commands::templates_cmd::handle_template_ai(app);
        }
        ShellAction::RegistryTest => {
            registry_test_selected(app, action_req_tx);
        }
    }
}

pub(in crate::ui) fn deploy_template(
    app: &mut App,
    name: &str,
    pull: bool,
    force_recreate: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if app
        .templates_state
        .template_deploy_inflight
        .contains_key(name)
    {
        app.set_warn(format!("template '{name}' is already deploying"));
        return;
    }
    let Some(tpl) = app
        .templates_state
        .templates
        .iter()
        .find(|t| t.name == name)
        .cloned()
    else {
        app.set_warn(format!("unknown template: {name}"));
        return;
    };
    if !tpl.has_compose {
        app.set_warn("template has no compose.yaml");
        return;
    }
    let template_id = match ensure_template_id(&tpl.compose_path) {
        Ok(id) => id,
        Err(e) => {
            app.set_error(format!("template id create failed: {e:#}"));
            return;
        }
    };
    if tpl.template_id.as_deref() != Some(&template_id) {
        app.refresh_templates();
    }
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let server_name = app.active_server.clone().unwrap_or_default();
    let template_commit = crate::ui::commands::git_cmd::git_head(&tpl.dir);
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let _ = action_req_tx.send(ActionRequest::TemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_compose: tpl.compose_path.clone(),
        pull,
        force_recreate,
        server_name,
        template_id,
        template_commit,
    });
    app.templates_state.template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    let mut msg = format!("deploying template {name}");
    if force_recreate {
        msg.push_str(" (recreate)");
    }
    if pull {
        msg.push_str(" [pull]");
    }
    app.set_info(msg);
}

pub(in crate::ui) fn deploy_net_template(
    app: &mut App,
    name: &str,
    force: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if app
        .templates_state
        .net_template_deploy_inflight
        .contains_key(name)
    {
        app.set_warn(format!("network template '{name}' is already deploying"));
        return;
    }
    let Some(tpl) = app
        .templates_state
        .net_templates
        .iter()
        .find(|t| t.name == name)
        .cloned()
    else {
        app.set_warn(format!("unknown network template: {name}"));
        return;
    };
    if !tpl.has_cfg {
        app.set_warn("template has no network.json");
        return;
    }
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let server_name = app.active_server.clone().unwrap_or_default();
    let _ = action_req_tx.send(ActionRequest::NetTemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_cfg: tpl.cfg_path.clone(),
        force,
        server_name,
    });
    app.templates_state.net_template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.set_info(format!("deploying network template {name}"));
}

pub(in crate::ui) fn edit_selected_template(app: &mut App) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => {
            let Some((name, has_compose, compose_path, dir)) = app.selected_template().map(|t| {
                (
                    t.name.clone(),
                    t.has_compose,
                    t.compose_path.clone(),
                    t.dir.clone(),
                )
            }) else {
                app.set_warn("no template selected");
                return;
            };
            app.templates_state.templates_refresh_after_edit = Some(name);
            let editor = app.editor_cmd();
            let target = if has_compose { compose_path } else { dir };
            let cmd = format!(
                "{} {}",
                editor,
                shell_escape_sh_arg(&target.to_string_lossy())
            );
            app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
        }
        TemplatesKind::Networks => edit_selected_net_template(app),
    }
}

pub(in crate::ui) fn edit_selected_net_template(app: &mut App) {
    let Some((name, has_cfg, cfg_path, dir)) = app
        .selected_net_template()
        .map(|t| (t.name.clone(), t.has_cfg, t.cfg_path.clone(), t.dir.clone()))
    else {
        app.set_warn("no network template selected");
        return;
    };
    app.templates_state.net_templates_refresh_after_edit = Some(name);
    let editor = app.editor_cmd();
    let target = if has_cfg { cfg_path } else { dir };
    let cmd = format!(
        "{} {}",
        editor,
        shell_escape_sh_arg(&target.to_string_lossy())
    );
    app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
}

pub(in crate::ui) fn template_name_from_stack(app: &App, stack_name: &str) -> Option<String> {
    let id = app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack_name))
        .filter_map(|c| crate::ui::template_id_from_labels(&c.labels))
        .next()?;
    app.templates_state
        .templates
        .iter()
        .find(|t| t.template_id.as_deref() == Some(id.as_str()))
        .map(|t| t.name.clone())
}

pub(in crate::ui) fn stack_compose_dirs(
    app: &App,
    stack_name: &str,
    template_name: Option<&str>,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let stack_name = stack_name.trim();
    if let Some(name) = template_name {
        let name = name.trim();
        if !name.is_empty() && name != stack_name {
            names.push(name.to_string());
        }
    }
    if !stack_name.is_empty() {
        names.push(stack_name.to_string());
    }
    names.dedup();
    let runner = current_runner_from_app(app);
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let path = match runner {
            crate::ui::Runner::Local => {
                let home = std::env::var("HOME").unwrap_or_default();
                format!("{home}/.config/containr/apps/{name}")
            }
            crate::ui::Runner::Ssh(_) => format!("$HOME/.config/containr/apps/{name}"),
        };
        out.push(path);
    }
    out
}

fn label_value_from_list(labels: &str, key: &str) -> Option<String> {
    for part in labels.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.splitn(2, '=');
        let k = it.next().unwrap_or("").trim();
        if k == key {
            let v = it.next().unwrap_or("").trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub(in crate::ui) fn service_name_from_label_list(
    labels: &str,
    stack_name: Option<&str>,
    container_name: &str,
) -> String {
    if let Some(name) = label_value_from_list(labels, "com.docker.compose.service") {
        return name;
    }
    if let Some(name) = label_value_from_list(labels, "com.docker.swarm.service.name") {
        if let Some(stack) = stack_name {
            for sep in ['_', '-', '.'] {
                let prefix = format!("{stack}{sep}");
                if name.starts_with(&prefix) {
                    return name[prefix.len()..].to_string();
                }
            }
        }
        return name;
    }
    let mut name = container_name.trim().trim_start_matches('/').to_string();
    if let Some(stack) = stack_name {
        for sep in ['_', '-', '.'] {
            let prefix = format!("{stack}{sep}");
            if name.starts_with(&prefix) {
                name = name[prefix.len()..].to_string();
                break;
            }
        }
    }
    if name.is_empty() {
        "service".to_string()
    } else {
        name
    }
}
