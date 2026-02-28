//! Generic App state helpers.

use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellSplitMode, ShellView};

impl App {
    pub(in crate::ui) fn editor_cmd(&self) -> String {
        let configured = self.editor_cmd.trim();
        if !configured.is_empty() {
            return configured.to_string();
        }
        std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
    }

    pub(in crate::ui) fn get_view_split_mode(&self, view: ShellView) -> Option<ShellSplitMode> {
        self.shell_split_by_view.get(view.slug()).copied()
    }

    pub(in crate::ui) fn set_view_split_mode(&mut self, view: ShellView, mode: ShellSplitMode) {
        self.shell_split_by_view
            .insert(view.slug().to_string(), mode);
    }

    pub(in crate::ui) fn cmd_history_max_effective(&self) -> usize {
        self.cmd_history_max.clamp(1, 5000)
    }

    pub(in crate::ui) fn is_stack_update_container(&self, id: &str) -> bool {
        self.stack_update_containers
            .values()
            .any(|ids| ids.iter().any(|v| v == id))
    }

    pub(in crate::ui) fn set_cmd_history_entries(&mut self, mut entries: Vec<String>) {
        entries.retain(|s| !s.trim().is_empty());
        let max = self.cmd_history_max_effective();
        if entries.len() > max {
            let drain = entries.len() - max;
            entries.drain(0..drain);
        }
        self.shell_cmdline.history.entries = entries.clone();
        self.shell_cmdline.history.reset_nav();
        self.logs.cmd_history.entries = entries.clone();
        self.logs.cmd_history.reset_nav();
        self.inspect.cmd_history.entries = entries;
        self.inspect.cmd_history.reset_nav();
    }
}
