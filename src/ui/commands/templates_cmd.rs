//! Template commands (`:templates ...`, `:template ...`, `:nettemplate ...`).

use super::super::{
    ActionRequest, App, ShellSidebarItem, ShellView, TemplatesKind, shell_begin_confirm,
    shell_set_main_view, shell_sidebar_select_item,
};
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
    shell_set_main_view(app, ShellView::Templates);
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
            shell_set_main_view(app, ShellView::Templates);
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
                        shell_set_main_view(app, ShellView::Templates);
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
                        shell_set_main_view(app, ShellView::Templates);
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
        "deploy" => {
            let name = if let Some(v) = args.get(1).copied() {
                v.to_string()
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
            match app.templates_state.kind {
                TemplatesKind::Stacks => {
                    super::super::shell_deploy_template(app, &name, action_req_tx)
                }
                TemplatesKind::Networks => {
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
                        shell_set_main_view(app, ShellView::Templates);
                        shell_sidebar_select_item(
                            app,
                            ShellSidebarItem::Module(ShellView::Templates),
                        );
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
                TemplatesKind::Networks => match super::super::delete_net_template(app, &name) {
                    Ok(()) => {
                        app.refresh_net_templates();
                        app.set_info(format!("deleted network template {name}"));
                        shell_set_main_view(app, ShellView::Templates);
                        shell_sidebar_select_item(
                            app,
                            ShellSidebarItem::Module(ShellView::Templates),
                        );
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
            }
            true
        }
        _ => {
            app.set_warn("usage: :template add <name> | :template deploy[!] [name] | :template rm[!] [name] | :templates kind (stacks|networks|toggle)");
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
            shell_set_main_view(app, ShellView::Templates);
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
                    shell_set_main_view(app, ShellView::Templates);
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
                    shell_set_main_view(app, ShellView::Templates);
                    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
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
