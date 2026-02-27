//! Logging / status message helpers on App.

use crate::ui::core::clock::now_local;
use crate::ui::render::messages::format_session_ts;
use crate::ui::render::clipboard::copy_to_clipboard;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{MsgLevel, SessionMsg, ShellView};

impl App {
    pub(in crate::ui) fn log_msg(&mut self, level: MsgLevel, text: impl Into<String>) {
        let text = text.into();
        let at = now_local();
        self.session_msgs.push(SessionMsg { at, level, text });
        if self.log_dock_enabled || self.shell_view == ShellView::Messages {
            self.shell_msgs.scroll = usize::MAX;
            self.shell_msgs.hscroll = 0;
        }
    }

    pub(in crate::ui) fn mark_messages_seen(&mut self) {
        self.messages_seen_len = self.session_msgs.len();
    }

    pub(in crate::ui) fn unseen_error_count(&self) -> usize {
        self.session_msgs
            .iter()
            .skip(self.messages_seen_len.min(self.session_msgs.len()))
            .filter(|m| matches!(m.level, MsgLevel::Error))
            .count()
    }

    pub(in crate::ui) fn push_cmd_history(&mut self, cmd: &str) {
        let max = self.cmd_history_max_effective();
        self.shell_cmdline.history.push(cmd, max);
        // Keep all command modes in sync.
        let entries = self.shell_cmdline.history.entries.clone();
        self.logs.cmd_history.entries = entries.clone();
        self.inspect.cmd_history.entries = entries;
        self.shell_cmdline.history.reset_nav();
        self.logs.cmd_history.reset_nav();
        self.inspect.cmd_history.reset_nav();
        self.persist_config();
    }

    pub(in crate::ui) fn clear_last_error(&mut self) {
        self.last_error = None;
    }

    pub(in crate::ui) fn set_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.last_error = Some(t.clone());
        self.log_msg(MsgLevel::Error, t);
    }

    pub(in crate::ui) fn set_warn(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.last_error = Some(t.clone());
        self.log_msg(MsgLevel::Warn, t);
    }

    pub(in crate::ui) fn set_info(&mut self, text: impl Into<String>) {
        self.log_msg(MsgLevel::Info, text);
    }

    pub(in crate::ui) fn messages_copy_selected(&mut self) {
        if self.session_msgs.is_empty() {
            self.set_warn("no messages");
            return;
        }
        let idx = if self.shell_msgs.scroll == usize::MAX {
            self.session_msgs.len().saturating_sub(1)
        } else {
            self.shell_msgs
                .scroll
                .min(self.session_msgs.len().saturating_sub(1))
        };
        let m = &self.session_msgs[idx];
        let lvl = match m.level {
            MsgLevel::Info => "INFO",
            MsgLevel::Warn => "WARN",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let line = format!("{ts} {lvl} {}", m.text);
        if let Err(e) = copy_to_clipboard(&line) {
            self.set_error(format!("{e:#}"));
        } else {
            self.set_info("copied message to clipboard");
        }
    }

    pub(in crate::ui) fn clear_conn_error(&mut self) {
        self.conn_error = None;
    }

    pub(in crate::ui) fn set_conn_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.conn_error = Some(t.clone());
        self.set_error(t);
    }
}
