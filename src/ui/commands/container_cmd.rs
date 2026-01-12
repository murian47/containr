//! Container commands (`:container ...` / `:ctr ...`).

use super::super::{ActiveView, App, ListMode, ViewEntry, shell_begin_confirm};
use crate::docker::ContainerAction;
use std::collections::HashSet;
use tokio::sync::mpsc;

pub fn handle_container(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<super::super::ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    let rest = &args.get(1..).unwrap_or(&[]);
    match sub {
        "start" => crate::ui::state::actions::exec_container_action(app, ContainerAction::Start, action_req_tx),
        "stop" => crate::ui::state::actions::exec_container_action(app, ContainerAction::Stop, action_req_tx),
        "restart" => crate::ui::state::actions::exec_container_action(app, ContainerAction::Restart, action_req_tx),
        "rm" | "delete" | "remove" => {
            if force {
                crate::ui::state::actions::exec_container_action(app, ContainerAction::Remove, action_req_tx)
            } else {
                shell_begin_confirm(app, "container rm", cmdline_full);
            }
        }
        "console" => {
            let mut it = rest.iter().copied();
            let mut user: Option<String> = None;
            let mut shell: Option<String> = None;
            while let Some(tok) = it.next() {
                if tok == "-u" {
                    user = it.next().map(|s| s.to_string());
                    if user.is_none() {
                        app.set_warn("usage: :container console [-u USER] [bash|sh|SHELL]");
                        return true;
                    }
                } else if shell.is_none() {
                    shell = Some(tok.to_string());
                } else {
                    app.set_warn("usage: :container console [-u USER] [bash|sh|SHELL]");
                    return true;
                }
            }
            let shell = shell.unwrap_or_else(|| "bash".to_string());
            let user = user.as_deref().or(Some("root"));
            super::super::shell_open_console(app, user, &shell);
        }
        "tree" => {
            app.active_view = ActiveView::Containers;
            let anchor_id = app.selected_container().map(|c| c.id.clone());
            app.list_mode = match app.list_mode {
                ListMode::Flat => ListMode::Tree,
                ListMode::Tree => ListMode::Flat,
            };
            app.view_dirty = true;
            app.ensure_view();
            if let Some(id) = anchor_id {
                if app.list_mode == ListMode::Tree {
                    if let Some(idx) = app
                        .view
                        .iter()
                        .position(|e| matches!(e, ViewEntry::Container { id: cid, .. } if cid == &id))
                    {
                        app.selected = idx;
                    }
                } else if let Some(idx) = app.container_idx_by_id.get(&id).copied() {
                    app.selected = idx;
                }
            }
        }
        "check" | "updates" => {
            if !rest.is_empty() {
                app.set_warn("usage: :container check");
                return true;
            }
            let ids = app.container_ids_for_selection();
            if ids.is_empty() {
                app.set_warn("no containers selected");
                return true;
            }
            let mut images: HashSet<String> = HashSet::new();
            for id in ids {
                if let Some(idx) = app.container_idx_by_id.get(&id).copied() {
                    if let Some(c) = app.containers.get(idx) {
                        images.insert(c.image.clone());
                    }
                }
            }
            super::super::shell_check_image_updates(
                app,
                images.into_iter().collect(),
                action_req_tx,
            );
        }
        "recreate" => {
            let _ = force;
            let _ = cmdline_full;
            app.set_warn("use :template deploy --recreate [--pull] <name>");
        }
        _ => app.set_warn(
            "usage: :container (start|stop|restart|rm|console [bash|sh]|tree|check)  (uses selection/marked/stack)",
        ),
    }
    true
}
