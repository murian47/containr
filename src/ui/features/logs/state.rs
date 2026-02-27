use crate::ui::render::clipboard::copy_to_clipboard;
use regex::RegexBuilder;

use crate::ui::core::types::LogsMode;
use crate::ui::state::app::App;

impl App {
    pub(in crate::ui) fn open_logs_state(&mut self, id: String) {
        self.logs.loading = true;
        self.logs.error = None;
        self.logs.text = None;
        self.logs.for_id = Some(id);
        self.logs.cursor = 0;
        self.logs.scroll_top = 0;
        self.logs.select_anchor = None;
        self.logs.hscroll = 0;
        self.logs.max_width = 0;
        self.logs.mode = LogsMode::Normal;
        self.logs.input.clear();
        self.logs.query.clear();
        self.logs.command.clear();
        self.logs.regex = None;
        self.logs.regex_error = None;
        self.logs.match_lines.clear();
        self.logs.show_line_numbers = false;
    }

    pub(in crate::ui) fn logs_move_up(&mut self, by: usize) {
        self.logs.cursor = self.logs.cursor.saturating_sub(by);
    }

    pub(in crate::ui) fn logs_move_down(&mut self, by: usize) {
        let total = self.logs_total_lines();
        if total == 0 {
            self.logs.cursor = 0;
            return;
        }
        self.logs.cursor = self.logs.cursor.saturating_add(by).min(total - 1);
    }

    pub(in crate::ui) fn logs_total_lines(&self) -> usize {
        self.logs
            .text
            .as_ref()
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    pub(in crate::ui) fn logs_toggle_selection(&mut self) {
        if self.logs.select_anchor.take().is_none() {
            self.logs.select_anchor = Some(self.logs.cursor);
        }
    }

    pub(in crate::ui) fn logs_clear_selection(&mut self) {
        self.logs.select_anchor = None;
    }

    pub(in crate::ui) fn logs_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.logs.select_anchor?;
        let a = anchor.min(self.logs.cursor);
        let b = anchor.max(self.logs.cursor);
        Some((a, b))
    }

    pub(in crate::ui) fn logs_copy_selection(&mut self) {
        let Some(text) = self.logs.text.as_deref() else {
            self.set_warn("no logs loaded");
            return;
        };

        let total = self.logs_total_lines();
        if total == 0 {
            self.set_warn("no logs loaded");
            return;
        }

        let (start, end) = self
            .logs_selection_range()
            .unwrap_or((self.logs.cursor, self.logs.cursor));
        let start = start.min(total.saturating_sub(1));
        let end = end.min(total.saturating_sub(1));

        let mut out = String::new();
        for (i, line) in text.lines().enumerate() {
            if i < start {
                continue;
            }
            if i > end {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }

        if out.is_empty() {
            self.set_warn("nothing to copy");
            return;
        }

        if let Err(e) = copy_to_clipboard(&out) {
            self.set_error(format!("{e:#}"));
        } else {
            let count = end.saturating_sub(start) + 1;
            self.set_info(format!("copied {count} line(s) to clipboard"));
            self.logs_clear_selection();
        }
    }

    pub(in crate::ui) fn logs_rebuild_matches(&mut self) {
        let q = match self.logs.mode {
            LogsMode::Search => self.logs.input.trim(),
            LogsMode::Normal | LogsMode::Command => self.logs.query.trim(),
        };
        if q.is_empty() {
            self.logs.match_lines.clear();
            self.logs.regex = None;
            self.logs.regex_error = None;
            return;
        }

        let Some(text) = &self.logs.text else {
            self.logs.match_lines.clear();
            return;
        };

        if self.logs.use_regex {
            match RegexBuilder::new(q).case_insensitive(true).build() {
                Ok(re) => {
                    self.logs.regex = Some(re);
                    self.logs.regex_error = None;
                }
                Err(e) => {
                    self.logs.regex = None;
                    self.logs.regex_error = Some(format!("{e}"));
                    self.logs.match_lines.clear();
                    return;
                }
            }

            let Some(re) = self.logs.regex.as_ref() else {
                self.logs.match_lines.clear();
                return;
            };
            self.logs.match_lines = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| if re.is_match(line) { Some(i) } else { None })
                .collect();
        } else {
            self.logs.regex = None;
            self.logs.regex_error = None;
            let q_lc = q.to_ascii_lowercase();
            self.logs.match_lines = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    if line.to_ascii_lowercase().contains(&q_lc) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
        }
    }

    pub(in crate::ui) fn logs_commit_search(&mut self) {
        self.logs.query = self.logs.input.clone();
        self.logs.mode = LogsMode::Normal;
        self.logs.input.clear();
        self.logs.input_cursor = 0;
        self.logs_rebuild_matches();
        if let Some(first) = self.logs.match_lines.first().copied() {
            self.logs.cursor = first;
        }
    }

    pub(in crate::ui) fn logs_cancel_search(&mut self) {
        self.logs.mode = LogsMode::Normal;
        self.logs.input.clear();
        self.logs.input_cursor = 0;
        self.logs_rebuild_matches();
    }

    pub(in crate::ui) fn logs_next_match(&mut self) {
        if self.logs.mode != LogsMode::Normal {
            return;
        }
        if self.logs.match_lines.is_empty() {
            return;
        }
        let cur = self.logs.cursor;
        let next = self
            .logs
            .match_lines
            .iter()
            .copied()
            .find(|&i| i > cur)
            .or_else(|| self.logs.match_lines.first().copied())
            .unwrap();
        self.logs.cursor = next;
    }

    pub(in crate::ui) fn logs_prev_match(&mut self) {
        if self.logs.mode != LogsMode::Normal {
            return;
        }
        if self.logs.match_lines.is_empty() {
            return;
        }
        let cur = self.logs.cursor;
        let prev = self
            .logs
            .match_lines
            .iter()
            .copied()
            .rfind(|&i| i < cur)
            .or_else(|| self.logs.match_lines.last().copied())
            .unwrap();
        self.logs.cursor = prev;
    }
}
