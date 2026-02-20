//! Template commands (`:templates ...`, `:template ...`, `:nettemplate ...`).

use super::super::{
    ActionRequest, App, ShellInteractive, ShellSidebarItem, ShellView, TemplatesKind, shell_begin_confirm,
    shell_escape_sh_arg, shell_sidebar_select_item,
};
use std::path::PathBuf;
use tokio::sync::mpsc;

fn begin_new_prompt(app: &mut App) {
    app.shell_cmdline.mode = true;
    super::super::set_text_and_cursor(
        &mut app.shell_cmdline.input,
        &mut app.shell_cmdline.cursor,
        match app.templates_state.kind {
            TemplatesKind::Stacks => "template add ".to_string(),
            TemplatesKind::Networks => "nettemplate add ".to_string(),
        },
    );
    app.shell_cmdline.confirm = None;
}

fn begin_export_prompt(app: &mut App, sub: &str) {
    app.shell_cmdline.mode = true;
    super::super::set_text_and_cursor(
        &mut app.shell_cmdline.input,
        &mut app.shell_cmdline.cursor,
        format!("template {sub} "),
    );
    app.shell_cmdline.confirm = None;
}

fn set_templates_kind(app: &mut App, v: &str) -> bool {
    match v.to_ascii_lowercase().as_str() {
        "stacks" | "stack" | "compose" => app.templates_state.kind = TemplatesKind::Stacks,
        "networks" | "network" | "net" => app.templates_state.kind = TemplatesKind::Networks,
        "toggle" => {
            app.templates_state.kind = match app.templates_state.kind {
                TemplatesKind::Stacks => TemplatesKind::Networks,
                TemplatesKind::Networks => TemplatesKind::Stacks,
            }
        }
        _ => {
            app.set_warn("usage: :templates kind (stacks|networks|toggle)");
            return false;
        }
    }
    if app.templates_state.kind == TemplatesKind::Stacks {
        app.refresh_templates();
    } else {
        app.refresh_net_templates();
    }
    app.set_main_view(ShellView::Templates);
    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
    true
}

pub fn handle_templates(app: &mut App, args: &[&str]) -> bool {
    let sub = args.first().copied().unwrap_or("");
    if sub.is_empty() {
        app.set_info(format!(
            "templates kind: {}",
            match app.templates_state.kind {
                TemplatesKind::Stacks => "stacks",
                TemplatesKind::Networks => "networks",
            }
        ));
        return true;
    }
    if sub == "toggle" {
        return set_templates_kind(app, "toggle");
    }
    if sub == "kind" {
        // If no argument is provided, behave like "toggle" (convenient in command-line mode).
        let v = args.get(1).copied().unwrap_or("toggle");
        return set_templates_kind(app, v);
    }
    app.set_warn("usage: :templates kind (stacks|networks|toggle)");
    true
}

pub fn handle_template(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");

    match sub {
        "kind" => {
            // Alias for :templates kind ...
            let v = args.get(1).copied().unwrap_or("toggle");
            set_templates_kind(app, v)
        }
        "toggle" => set_templates_kind(app, "toggle"),
        "edit" => {
            app.set_main_view(ShellView::Templates);
            shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
            super::super::shell_edit_selected_template(app);
            true
        }
        "new" => {
            begin_new_prompt(app);
            true
        }
        "add" => {
            let Some(name) = args.get(1).copied() else {
                begin_new_prompt(app);
                return true;
            };
            match app.templates_state.kind {
                TemplatesKind::Stacks => match super::super::create_template(app, name) {
                    Ok(()) => {
                        app.refresh_templates();
                        if let Some(idx) = app
                            .templates_state
                            .templates
                            .iter()
                            .position(|t| t.name == name)
                        {
                            app.templates_state.templates_selected = idx;
                        }
                        app.set_main_view(ShellView::Templates);
                        shell_sidebar_select_item(
                            app,
                            ShellSidebarItem::Module(ShellView::Templates),
                        );
                        super::super::shell_edit_selected_template(app);
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
                TemplatesKind::Networks => match super::super::create_net_template(app, name) {
                    Ok(()) => {
                        app.refresh_net_templates();
                        if let Some(idx) = app
                            .templates_state
                            .net_templates
                            .iter()
                            .position(|t| t.name == name)
                        {
                            app.templates_state.net_templates_selected = idx;
                        }
                        app.set_main_view(ShellView::Templates);
                        shell_sidebar_select_item(
                            app,
                            ShellSidebarItem::Module(ShellView::Templates),
                        );
                        super::super::shell_edit_selected_net_template(app);
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
            }
            true
        }
        "from-stack" => {
            let Some(name) = args.get(1).copied() else {
                begin_export_prompt(app, "from-stack");
                return true;
            };
            let stack_name = if app.shell_view == ShellView::Stacks
                || app.active_view == super::super::ActiveView::Stacks
            {
                app.selected_stack_entry().map(|s| s.name.clone())
            } else {
                app.selected_stack().map(|(name, ..)| name.to_string())
            };
            let Some(stack_name) = stack_name else {
                app.set_warn("no stack selected");
                return true;
            };
            let ids = app.stack_container_ids(&stack_name);
            if ids.is_empty() {
                app.set_warn("no containers in stack");
                return true;
            }
            let templates_dir = app.stack_templates_dir();
            let source = format!("stack {stack_name}");
            app.set_info(format!(
                "exporting template {name} from stack {stack_name}"
            ));
            let _ = action_req_tx.send(ActionRequest::TemplateFromStack {
                name: name.to_string(),
                stack_name,
                source,
                container_ids: ids,
                templates_dir,
            });
            true
        }
        "from-container" => {
            let Some(name) = args.get(1).copied() else {
                begin_export_prompt(app, "from-container");
                return true;
            };
            let Some(container) = app.selected_container() else {
                app.set_warn("no container selected");
                return true;
            };
            let container_id = container.id.clone();
            let container_name = container.name.clone();
            let templates_dir = app.stack_templates_dir();
            let source = format!(
                "container {}",
                container_name.trim_start_matches('/')
            );
            app.set_info(format!(
                "exporting template {name} from container {}",
                container_name
            ));
            let _ = action_req_tx.send(ActionRequest::TemplateFromContainer {
                name: name.to_string(),
                source,
                container_id,
                templates_dir,
            });
            true
        }
        "from-network" => {
            let Some(arg1) = args.get(1).copied() else {
                begin_export_prompt(app, "from-network");
                return true;
            };
            let (network_id, name, source) = if let Some(name) = args.get(2).copied() {
                let display = app
                    .networks
                    .iter()
                    .find(|n| n.id == arg1 || n.name == arg1)
                    .map(|n| n.name.clone())
                    .unwrap_or_else(|| arg1.to_string());
                (arg1.to_string(), name, format!("network {display}"))
            } else {
                let Some(net) = app.selected_network() else {
                    app.set_warn("no network selected");
                    return true;
                };
                (net.id.clone(), arg1, format!("network {}", net.name))
            };
            let templates_dir = app.net_templates_dir();
            app.set_info(format!(
                "exporting network template {name} from {source}"
            ));
            let _ = action_req_tx.send(ActionRequest::TemplateFromNetwork {
                name: name.to_string(),
                source,
                network_id,
                templates_dir,
            });
            true
        }
        "from" => {
            let kind = args.get(1).copied().unwrap_or("");
            let name = args.get(2).copied().unwrap_or("");
            if kind.is_empty() {
                app.set_warn("usage: :template from (stack|container|network) <name>");
                return true;
            }
            if name.is_empty() {
                begin_export_prompt(app, &format!("from {kind}"));
                return true;
            }
            match kind {
                "stack" => {
                    let _ = handle_template(
                        app,
                        force,
                        cmdline_full,
                        &["from-stack", name],
                        action_req_tx,
                    );
                    true
                }
                "container" => {
                    let _ = handle_template(
                        app,
                        force,
                        cmdline_full,
                        &["from-container", name],
                        action_req_tx,
                    );
                    true
                }
                "network" => {
                    let _ = handle_template(
                        app,
                        force,
                        cmdline_full,
                        &["from-network", name],
                        action_req_tx,
                    );
                    true
                }
                _ => {
                    app.set_warn("usage: :template from (stack|container|network) <name>");
                    true
                }
            }
        }
        "deploy" => {
            let mut pull = false;
            let mut recreate = false;
            let mut name = None;
            for arg in args.iter().skip(1) {
                match *arg {
                    "--pull" | "pull" => pull = true,
                    "--recreate" | "recreate" | "--force-recreate" => recreate = true,
                    _ if name.is_none() => name = Some((*arg).to_string()),
                    _ => {
                        app.set_warn("usage: :template deploy [--pull] [--recreate] [name]");
                        return true;
                    }
                }
            }
            let name = name.unwrap_or_else(|| {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                    TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
                }
                .unwrap_or_default()
            });
            if name.trim().is_empty() {
                app.set_warn("no template selected");
                return true;
            }
            match app.templates_state.kind {
                TemplatesKind::Stacks => {
                    if recreate && !force {
                        let label = format!("template recreate {name}");
                        shell_begin_confirm(app, label, cmdline_full);
                        return true;
                    }
                    super::super::shell_deploy_template(app, &name, pull, recreate, action_req_tx)
                }
                TemplatesKind::Networks => {
                    if pull || recreate {
                        app.set_warn("usage: :nettemplate deploy[!] [name]");
                        return true;
                    }
                    super::super::shell_deploy_net_template(app, &name, force, action_req_tx)
                }
            }
            true
        }
        "rm" | "del" | "delete" => {
            let name = if let Some(n) = args.get(1).copied() {
                n.to_string()
            } else {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                    TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
                }
                .unwrap_or_default()
            };
            if name.trim().is_empty() {
                app.set_warn("no template selected");
                return true;
            }
            if !force {
                shell_begin_confirm(
                    app,
                    format!(
                        "{} rm {name}",
                        match app.templates_state.kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                    cmdline_full,
                );
                return true;
            }
            match app.templates_state.kind {
                TemplatesKind::Stacks => match super::super::delete_template(app, &name) {
                    Ok(()) => {
                        app.refresh_templates();
                        app.set_info(format!("deleted template {name}"));
                        app.set_main_view(ShellView::Templates);
                        shell_sidebar_select_item(
                            app,
                            ShellSidebarItem::Module(ShellView::Templates),
                        );
                        super::super::maybe_autocommit_templates(
                            app,
                            TemplatesKind::Stacks,
                            "delete",
                            &name,
                        );
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
                TemplatesKind::Networks => match super::super::delete_net_template(app, &name) {
                    Ok(()) => {
                        app.refresh_net_templates();
                        app.set_info(format!("deleted network template {name}"));
                        app.set_main_view(ShellView::Templates);
                        shell_sidebar_select_item(
                            app,
                            ShellSidebarItem::Module(ShellView::Templates),
                        );
                        super::super::maybe_autocommit_templates(
                            app,
                            TemplatesKind::Networks,
                            "delete",
                            &name,
                        );
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
            }
            true
        }
        _ => {
            app.set_warn("usage: :template add <name> | :template from (stack|container|network) <name> | :template from-network <name> [network] | :template deploy[!] [--pull] [--recreate] [name] | :template rm[!] [name] | :templates kind (stacks|networks|toggle)");
            true
        }
    }
}

pub fn handle_nettemplate(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "edit" => {
            app.templates_state.kind = TemplatesKind::Networks;
            app.set_main_view(ShellView::Templates);
            shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
            super::super::shell_edit_selected_net_template(app);
            true
        }
        "new" => {
            app.shell_cmdline.mode = true;
            super::super::set_text_and_cursor(
                &mut app.shell_cmdline.input,
                &mut app.shell_cmdline.cursor,
                "nettemplate add ".to_string(),
            );
            app.shell_cmdline.confirm = None;
            true
        }
        "add" => {
            let Some(name) = args.get(1).copied() else {
                app.shell_cmdline.mode = true;
                super::super::set_text_and_cursor(
                    &mut app.shell_cmdline.input,
                    &mut app.shell_cmdline.cursor,
                    "nettemplate add ".to_string(),
                );
                app.shell_cmdline.confirm = None;
                return true;
            };
            match super::super::create_net_template(app, name) {
                Ok(()) => {
                    app.refresh_net_templates();
                    if let Some(idx) = app
                        .templates_state
                        .net_templates
                        .iter()
                        .position(|t| t.name == name)
                    {
                        app.templates_state.net_templates_selected = idx;
                    }
                    app.templates_state.kind = TemplatesKind::Networks;
                    app.set_main_view(ShellView::Templates);
                    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                    super::super::shell_edit_selected_net_template(app);
                }
                Err(e) => app.set_error(format!("{e:#}")),
            }
            true
        }
        "deploy" => {
            let name = if let Some(v) = args.get(1).copied() {
                v.to_string()
            } else if let Some(t) = app.selected_net_template().map(|t| t.name.clone()) {
                t
            } else {
                app.set_warn("usage: :nettemplate deploy <name>");
                return true;
            };
            super::super::shell_deploy_net_template(app, &name, force, action_req_tx);
            true
        }
        "rm" | "del" | "delete" => {
            let name = if let Some(n) = args.get(1).copied() {
                n.to_string()
            } else if let Some(t) = app.selected_net_template().map(|t| t.name.clone()) {
                t
            } else {
                app.set_warn("usage: :nettemplate rm <name>");
                return true;
            };
            if !force {
                shell_begin_confirm(app, format!("nettemplate rm {name}"), cmdline_full);
                return true;
            }
            match super::super::delete_net_template(app, &name) {
                Ok(()) => {
                    app.refresh_net_templates();
                    app.set_info(format!("deleted network template {name}"));
                    app.templates_state.kind = TemplatesKind::Networks;
                    app.set_main_view(ShellView::Templates);
                    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                    super::super::maybe_autocommit_templates(
                        app,
                        TemplatesKind::Networks,
                        "delete",
                        &name,
                    );
                }
                Err(e) => app.set_error(format!("{e:#}")),
            }
            true
        }
        _ => {
            app.set_warn(
                "usage: :nettemplate add <name> | :nettemplate deploy <name> | :nettemplate rm <name>",
            );
            true
        }
    }
}

pub fn handle_template_ai(app: &mut App) -> bool {
    if app.shell_view != ShellView::Templates {
        app.set_warn("AI is only available in Templates");
        return true;
    }
    let cmd_raw = match std::env::var("CONTAINR_AI_CMD") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            app.set_warn("AI command not configured (set CONTAINR_AI_CMD)");
            return true;
        }
    };
    let (kind, name, path, has_file) = match app.templates_state.kind {
        TemplatesKind::Stacks => app
            .selected_template()
            .map(|t| ("stack", t.name.clone(), t.compose_path.clone(), t.has_compose))
            .unwrap_or(("stack", String::new(), PathBuf::new(), false)),
        TemplatesKind::Networks => app
            .selected_net_template()
            .map(|t| ("network", t.name.clone(), t.cfg_path.clone(), t.has_cfg))
            .unwrap_or(("network", String::new(), PathBuf::new(), false)),
    };
    if name.trim().is_empty() {
        app.set_warn("no template selected");
        return true;
    }
    if !has_file {
        app.set_warn("template has no config file");
        return true;
    }
    let file = path.to_string_lossy().to_string();
    app.capture_template_ai_snapshot(app.templates_state.kind, name.clone(), path.clone());
    let cmd = format!(
        "CONTAINR_AI_FILE={} CONTAINR_AI_KIND={} CONTAINR_AI_NAME={} {}",
        shell_escape_sh_arg(&file),
        shell_escape_sh_arg(kind),
        shell_escape_sh_arg(&name),
        cmd_raw
    );
    match app.templates_state.kind {
        TemplatesKind::Stacks => {
            app.templates_state.templates_refresh_after_edit = Some(name);
        }
        TemplatesKind::Networks => {
            app.templates_state.net_templates_refresh_after_edit = Some(name);
        }
    }
    app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
    true
}
