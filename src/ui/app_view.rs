//! View navigation and view-scoped helpers for App.

use crate::ui::render::sidebar::shell_sidebar_select_item;
use crate::ui::{
    theme, ActiveView, App, InspectTarget, ListMode, ShellFocus, ShellSidebarItem, ShellView,
    ViewEntry,
};
use tokio::sync::{mpsc, watch};

pub(in crate::ui) fn shell_cycle_focus(app: &mut App) {
    let mut order: Vec<ShellFocus> = Vec::new();
    if !app.shell_sidebar_hidden {
        order.push(ShellFocus::Sidebar);
    }
    order.push(ShellFocus::List);
    let has_details = matches!(
        app.shell_view,
        ShellView::Stacks
            | ShellView::Containers
            | ShellView::Images
            | ShellView::Volumes
            | ShellView::Networks
            | ShellView::Templates
            | ShellView::Registries
    );
    if has_details {
        order.push(ShellFocus::Details);
    }
    let dock_allowed = app.log_dock_enabled
        && !matches!(
            app.shell_view,
            ShellView::Logs
                | ShellView::Inspect
                | ShellView::Help
                | ShellView::Messages
                | ShellView::ThemeSelector
        );
    if dock_allowed {
        order.push(ShellFocus::Dock);
    }
    if order.is_empty() {
        app.shell_focus = ShellFocus::List;
        return;
    }
    let idx = order
        .iter()
        .position(|f| *f == app.shell_focus)
        .unwrap_or(0);
    let next = (idx + 1) % order.len();
    app.shell_focus = order[next];
}

impl App {
    pub(in crate::ui) fn set_main_view(&mut self, view: ShellView) {
        self.shell_view = view;
        if !matches!(
            view,
            ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages
        ) {
            self.shell_last_main_view = view;
        }
        if view == ShellView::Messages {
            self.mark_messages_seen();
        }
        self.shell_focus = ShellFocus::List;
        self.active_view = match view {
            ShellView::Dashboard => self.active_view,
            ShellView::Stacks => ActiveView::Stacks,
            ShellView::Containers => ActiveView::Containers,
            ShellView::Images => ActiveView::Images,
            ShellView::Volumes => ActiveView::Volumes,
            ShellView::Networks => ActiveView::Networks,
            ShellView::Templates => self.active_view,
            ShellView::Registries => self.active_view,
            ShellView::Inspect
            | ShellView::Logs
            | ShellView::Help
            | ShellView::Messages
            | ShellView::ThemeSelector => self.active_view,
        };
    }

    pub(in crate::ui) fn back_from_full_view(&mut self) {
        if matches!(
            self.shell_view,
            ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages
        ) {
            // Full-screen views should never keep command-line mode active in the background.
            self.shell_cmdline.mode = false;
            self.shell_cmdline.confirm = None;
            let fallback = if self.shell_last_main_view == ShellView::Messages {
                ShellView::Dashboard
            } else {
                self.shell_last_main_view
            };
            self.shell_view = if self.shell_view == ShellView::Help {
                if self.shell_help.return_view == ShellView::Help {
                    fallback
                } else {
                    self.shell_help.return_view
                }
            } else if self.shell_view == ShellView::Messages {
                if self.shell_msgs.return_view == ShellView::Messages {
                    fallback
                } else {
                    self.shell_msgs.return_view
                }
            } else {
                fallback
            };
            self.shell_focus = ShellFocus::List;
            shell_sidebar_select_item(self, ShellSidebarItem::Module(self.shell_view));
        }
    }

    pub(in crate::ui) fn refresh_now(
        &mut self,
        refresh_tx: &mpsc::UnboundedSender<()>,
        dash_refresh_tx: &mpsc::UnboundedSender<()>,
        dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
        refresh_pause_tx: &watch::Sender<bool>,
    ) {
        if self.server_all_selected {
            for host in &mut self.dashboard_all.hosts {
                host.loading = true;
            }
            let _ = dash_all_refresh_tx.send(());
            return;
        }
        if self.servers.is_empty() && self.current_target.trim().is_empty() {
            self.set_warn("no server configured");
            return;
        }
        if self.refresh_paused {
            self.refresh_paused = false;
            self.refresh_pause_reason = None;
            let _ = refresh_pause_tx.send(false);
        }
        if self.shell_view == ShellView::Dashboard {
            self.dashboard.loading = true;
            let _ = dash_refresh_tx.send(());
        } else {
            let _ = refresh_tx.send(());
        }
    }

    pub(in crate::ui) fn first_container_id(&mut self) -> Option<String> {
        if let Some(c) = self.selected_container() {
            return Some(c.id.clone());
        }
        if self.active_view != ActiveView::Containers {
            self.active_view = ActiveView::Containers;
        }
        if self.containers.is_empty() {
            return None;
        }
        if self.list_mode == ListMode::Tree {
            self.ensure_view();
            if let Some((idx, ViewEntry::Container { id, .. })) = self
                .view
                .iter()
                .enumerate()
                .find(|(_, e)| matches!(e, ViewEntry::Container { .. }))
            {
                self.selected = idx;
                return Some(id.clone());
            }
        }
        self.selected = self.selected.min(self.containers.len().saturating_sub(1));
        Some(self.containers.get(self.selected)?.id.clone())
    }

    pub(in crate::ui) fn enter_logs(
        &mut self,
        logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    ) {
        // Logs are container-only; always use the containers selection.
        self.set_main_view(ShellView::Containers);
        self.shell_view = ShellView::Logs;
        self.shell_focus = ShellFocus::List;

        let Some(id) = self.first_container_id() else {
            self.logs.loading = false;
            self.logs.error = Some("no container selected".to_string());
            self.logs.text = None;
            return;
        };
        self.open_logs_state(id.clone());
        let _ = logs_req_tx.send((id, self.logs.tail.max(1)));
    }

    pub(in crate::ui) fn enter_inspect(
        &mut self,
        inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    ) {
        // Inspect follows the current main view selection.
        if matches!(self.shell_view, ShellView::Logs | ShellView::Inspect) {
            self.shell_view = self.shell_last_main_view;
        }
        self.shell_view = ShellView::Inspect;
        self.shell_focus = ShellFocus::List;

        let Some(target) = self.selected_inspect_target() else {
            self.inspect.loading = false;
            self.inspect.error = Some("nothing selected".to_string());
            self.inspect.value = None;
            self.inspect.lines.clear();
            return;
        };
        self.open_inspect_state(target.clone());
        let _ = inspect_req_tx.send(target);
    }

    pub(in crate::ui) fn open_theme_selector(&mut self) {
        let names = match theme::list_theme_names(&self.config_path) {
            Ok(mut list) => {
                if !list.iter().any(|n| n == "default") {
                    list.insert(0, "default".to_string());
                }
                list
            }
            Err(e) => {
                self.set_error(format!("theme list failed: {e:#}"));
                vec![self.theme_name.clone()]
            }
        };
        let selected = names
            .iter()
            .position(|n| n == &self.theme_name)
            .unwrap_or(0);

        self.theme_selector.names = names;
        self.theme_selector.selected = selected;
        self.theme_selector.scroll = 0;
        self.theme_selector.page_size = 0;
        self.theme_selector.center_on_open = true;
        let return_view = if self.shell_view == ShellView::ThemeSelector {
            self.theme_selector.return_view
        } else {
            self.shell_view
        };
        self.theme_selector.return_view = return_view;
        self.shell_view = ShellView::ThemeSelector;
        self.shell_focus = ShellFocus::List;
    }
}
