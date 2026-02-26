use std::collections::HashSet;

use crate::ui::render::clipboard::copy_to_clipboard;
use crate::ui::render::inspect::{
    ancestors_of_pointer, build_inspect_lines, collect_expandable_paths, collect_match_paths,
    collect_path_rank,
};

use crate::ui::{App, InspectMode, InspectTarget};

impl App {
    pub(in crate::ui) fn open_inspect_state(&mut self, target: InspectTarget) {
        self.inspect.loading = true;
        self.inspect.error = None;
        self.inspect.value = None;
        self.inspect.target = Some(target.clone());
        self.inspect.for_id = Some(target.key);
        self.inspect.lines.clear();
        self.inspect.selected = 0;
        self.inspect.scroll_top = 0;
        self.inspect.scroll = 0;
        self.inspect.query.clear();
        self.inspect.expanded.clear();
        self.inspect.expanded.insert("".to_string()); // root expanded by default
        self.inspect.match_paths.clear();
        self.inspect.path_rank.clear();
        self.inspect.mode = InspectMode::Normal;
        self.inspect.input.clear();
    }

    pub(in crate::ui) fn rebuild_inspect_lines(&mut self) {
        self.inspect.path_rank = collect_path_rank(self.inspect.value.as_ref());
        let effective_query = self.inspect_effective_query().to_string();
        self.inspect.match_paths = collect_match_paths(self.inspect.value.as_ref(), &effective_query);
        let match_set: HashSet<String> = self.inspect.match_paths.iter().cloned().collect();
        self.inspect.lines = build_inspect_lines(
            self.inspect.value.as_ref(),
            &self.inspect.expanded,
            &match_set,
            &effective_query,
        );
        if self.inspect.selected >= self.inspect.lines.len() {
            self.inspect.selected = self.inspect.lines.len().saturating_sub(1);
        }
        if self.inspect.scroll > self.inspect.selected {
            self.inspect.scroll = self.inspect.selected;
        }
    }

    pub(in crate::ui) fn inspect_move_up(&mut self, by: usize) {
        if self.inspect.lines.is_empty() {
            self.inspect.selected = 0;
            self.inspect.scroll = 0;
            return;
        }
        self.inspect.selected = self.inspect.selected.saturating_sub(by);
        if self.inspect.selected < self.inspect.scroll {
            self.inspect.scroll = self.inspect.selected;
        }
    }

    pub(in crate::ui) fn inspect_move_down(&mut self, by: usize) {
        if self.inspect.lines.is_empty() {
            self.inspect.selected = 0;
            self.inspect.scroll = 0;
            return;
        }
        self.inspect.selected = self
            .inspect
            .selected
            .saturating_add(by)
            .min(self.inspect.lines.len() - 1);
    }

    pub(in crate::ui) fn inspect_toggle_selected(&mut self) {
        let Some(line) = self.inspect.lines.get(self.inspect.selected) else {
            return;
        };
        if !line.expandable {
            return;
        }
        if self.inspect.expanded.contains(&line.path) {
            self.inspect.expanded.remove(&line.path);
        } else {
            self.inspect.expanded.insert(line.path.clone());
        }
        self.rebuild_inspect_lines();
    }

    pub(in crate::ui) fn inspect_expand_all(&mut self) {
        let Some(root) = self.inspect.value.as_ref() else {
            return;
        };
        self.inspect.expanded = collect_expandable_paths(root);
        self.inspect.expanded.insert("".to_string());
        self.rebuild_inspect_lines();
    }

    pub(in crate::ui) fn inspect_collapse_all(&mut self) {
        self.inspect.expanded.clear();
        self.inspect.expanded.insert("".to_string());
        self.rebuild_inspect_lines();
    }

    pub(in crate::ui) fn inspect_jump_next_match(&mut self) {
        if self.inspect.mode != InspectMode::Normal {
            return;
        }
        if self.inspect.match_paths.is_empty() {
            return;
        }
        let current_path = self
            .inspect
            .lines
            .get(self.inspect.selected)
            .map(|l| l.path.as_str())
            .unwrap_or("");

        let current_rank = self.inspect.path_rank.get(current_path).copied().unwrap_or(0);

        let mut best: Option<(usize, String)> = None;
        for p in &self.inspect.match_paths {
            let r = self.inspect.path_rank.get(p).copied().unwrap_or(usize::MAX);
            if r > current_rank && best.as_ref().map(|(br, _)| r < *br).unwrap_or(true) {
                best = Some((r, p.clone()));
            }
        }
        let target = best
            .map(|(_, p)| p)
            .or_else(|| self.inspect.match_paths.first().cloned());
        if let Some(target) = target {
            self.inspect_focus_path(&target);
        }
    }

    pub(in crate::ui) fn inspect_jump_prev_match(&mut self) {
        if self.inspect.mode != InspectMode::Normal {
            return;
        }
        if self.inspect.match_paths.is_empty() {
            return;
        }
        let current_path = self
            .inspect
            .lines
            .get(self.inspect.selected)
            .map(|l| l.path.as_str())
            .unwrap_or("");

        let current_rank = self.inspect.path_rank.get(current_path).copied().unwrap_or(0);

        let mut best: Option<(usize, String)> = None;
        for p in &self.inspect.match_paths {
            let r = self.inspect.path_rank.get(p).copied().unwrap_or(0);
            if r < current_rank && best.as_ref().map(|(br, _)| r > *br).unwrap_or(true) {
                best = Some((r, p.clone()));
            }
        }
        let target = best
            .map(|(_, p)| p)
            .or_else(|| self.inspect.match_paths.last().cloned());
        if let Some(target) = target {
            self.inspect_focus_path(&target);
        }
    }

    pub(in crate::ui) fn inspect_focus_path(&mut self, path: &str) {
        for parent in ancestors_of_pointer(path) {
            self.inspect.expanded.insert(parent);
        }
        self.rebuild_inspect_lines();
        if let Some(idx) = self.inspect.lines.iter().position(|l| l.path == path) {
            self.inspect.selected = idx;
        }
    }

    pub(in crate::ui) fn inspect_effective_query(&self) -> &str {
        match self.inspect.mode {
            InspectMode::Search => &self.inspect.input,
            _ => &self.inspect.query,
        }
    }

    pub(in crate::ui) fn inspect_enter_search(&mut self) {
        self.inspect.mode = InspectMode::Search;
        self.inspect.input = self.inspect.query.clone();
        self.inspect.input_cursor = self.inspect.input.chars().count();
        self.rebuild_inspect_lines();
    }

    pub(in crate::ui) fn inspect_enter_command(&mut self) {
        self.inspect.mode = InspectMode::Command;
        self.inspect.input.clear();
        self.inspect.input_cursor = 0;
    }

    pub(in crate::ui) fn inspect_exit_input(&mut self) {
        self.inspect.mode = InspectMode::Normal;
        self.inspect.input.clear();
        self.inspect.input_cursor = 0;
        self.rebuild_inspect_lines();
    }

    pub(in crate::ui) fn inspect_commit_search(&mut self) {
        self.inspect.query = self.inspect.input.clone();
        self.inspect.mode = InspectMode::Normal;
        self.inspect.input.clear();
        self.inspect.input_cursor = 0;
        self.rebuild_inspect_lines();
        if let Some(first) = self.inspect.match_paths.first().cloned() {
            self.inspect_focus_path(&first);
        }
    }

    pub(in crate::ui) fn inspect_copy_selected_value(&mut self, pretty: bool) {
        let Some(root) = self.inspect.value.as_ref() else {
            return;
        };
        let Some(line) = self.inspect.lines.get(self.inspect.selected) else {
            return;
        };
        let Some(value) = root.pointer(&line.path) else {
            self.inspect.error = Some("failed to locate selected value".to_string());
            return;
        };

        let text = if pretty {
            match serde_json::to_string_pretty(value) {
                Ok(s) => s,
                Err(e) => {
                    self.inspect.error = Some(format!("failed to serialize value: {:#}", e));
                    return;
                }
            }
        } else {
            value.to_string()
        };

        if let Err(e) = copy_to_clipboard(&text) {
            self.inspect.error = Some(format!("{:#}", e));
        }
    }

    pub(in crate::ui) fn inspect_copy_selected_path(&mut self) {
        let Some(line) = self.inspect.lines.get(self.inspect.selected) else {
            return;
        };
        if let Err(e) = copy_to_clipboard(&line.path) {
            self.inspect.error = Some(format!("{:#}", e));
        }
    }
}
