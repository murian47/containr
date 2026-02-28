use crate::ui::commands;
use crate::ui::render::sidebar::shell_sidebar_select_item;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellSidebarItem, ShellView};
use crate::ui::theme;

impl App {
    pub(in crate::ui) fn theme_selector_selected_name(&self) -> Option<&str> {
        self.theme_selector
            .names
            .get(self.theme_selector.selected)
            .map(|s| s.as_str())
    }

    pub(in crate::ui) fn theme_selector_update_preview(&mut self) {
        let Some(name) = self.theme_selector_selected_name().map(|s| s.to_string()) else {
            self.theme_selector.preview_theme = self.theme.clone();
            self.theme_selector.error = Some("no themes available".to_string());
            return;
        };
        match theme::load_theme(&self.config_path, &name) {
            Ok(spec) => {
                self.theme_selector.preview_theme = spec;
                self.theme_selector.error = None;
            }
            Err(e) => {
                self.theme_selector.preview_theme = self.theme.clone();
                self.theme_selector.error = Some(format!("failed to load theme '{name}': {e:#}"));
            }
        }
    }

    pub(in crate::ui) fn theme_selector_search(&mut self, query: &str) {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return;
        }
        if let Some((idx, _)) = self
            .theme_selector
            .names
            .iter()
            .enumerate()
            .find(|(_, name)| name.to_ascii_lowercase().contains(&query))
        {
            if idx != self.theme_selector.selected {
                self.theme_selector.selected = idx;
                self.theme_selector_update_preview();
            }
            self.theme_selector_adjust_scroll(true);
        }
    }

    pub(in crate::ui) fn theme_selector_adjust_scroll(&mut self, center: bool) {
        let total = self.theme_selector.names.len();
        if total == 0 {
            self.theme_selector.scroll = 0;
            self.theme_selector.selected = 0;
            return;
        }
        let view = self.theme_selector.page_size.max(1);
        let max_scroll = total.saturating_sub(view);
        let selected = self.theme_selector.selected.min(total.saturating_sub(1));

        if center {
            let mut scroll = selected.saturating_sub(view / 2);
            if scroll > max_scroll {
                scroll = max_scroll;
            }
            self.theme_selector.scroll = scroll;
            return;
        }

        let mut scroll = self.theme_selector.scroll.min(max_scroll);
        if selected < scroll {
            scroll = selected;
        } else if selected >= scroll + view {
            scroll = selected.saturating_sub(view.saturating_sub(1));
        }
        self.theme_selector.scroll = scroll;
    }

    pub(in crate::ui) fn theme_selector_move(&mut self, delta: i32) {
        if self.theme_selector.names.is_empty() {
            self.theme_selector.selected = 0;
            return;
        }
        let len = self.theme_selector.names.len();
        let cur = self.theme_selector.selected;
        let step = if delta < 0 {
            (-delta) as usize
        } else {
            delta as usize
        };
        let next = if delta < 0 {
            cur.saturating_sub(step)
        } else {
            cur.saturating_add(step)
        }
        .min(len.saturating_sub(1));
        if next != cur {
            self.theme_selector.selected = next;
            self.theme_selector_update_preview();
            self.theme_selector_adjust_scroll(false);
        }
    }

    pub(in crate::ui) fn theme_selector_page_move(&mut self, delta: i32) {
        let step = self.theme_selector.page_size.max(1);
        self.theme_selector_move(delta.saturating_mul(step as i32));
    }

    pub(in crate::ui) fn theme_selector_apply(&mut self) {
        let Some(name) = self.theme_selector_selected_name().map(|s| s.to_string()) else {
            self.set_warn("no theme selected");
            return;
        };
        if let Err(e) = commands::theme_cmd::set_theme(self, &name) {
            self.set_error(format!("{e:#}"));
            return;
        }
        self.reset_dashboard_image();
        let return_view = if self.theme_selector.return_view == ShellView::ThemeSelector {
            ShellView::Dashboard
        } else {
            self.theme_selector.return_view
        };
        self.shell_view = return_view;
        self.shell_focus = ShellFocus::List;
        if return_view != ShellView::ThemeSelector {
            shell_sidebar_select_item(self, ShellSidebarItem::Module(return_view));
        }
    }

    pub(in crate::ui) fn theme_selector_cancel(&mut self) {
        let base = self.theme_selector.base_theme_name.clone();
        if let Err(e) = commands::theme_cmd::set_theme(self, &base) {
            self.set_error(format!("{e:#}"));
        } else {
            self.reset_dashboard_image();
        }
        self.shell_view = if self.theme_selector.return_view == ShellView::ThemeSelector {
            ShellView::Dashboard
        } else {
            self.theme_selector.return_view
        };
        self.shell_focus = ShellFocus::List;
        if self.shell_view != ShellView::ThemeSelector {
            shell_sidebar_select_item(self, ShellSidebarItem::Module(self.shell_view));
        }
    }
}
