#[derive(Debug, Default, Clone)]
pub(in crate::ui) struct CmdHistory {
    pub(in crate::ui) entries: Vec<String>,
    pos: Option<usize>,
    saved_current: String,
}

impl CmdHistory {
    pub(in crate::ui) fn new() -> Self {
        Self::default()
    }

    pub(in crate::ui) fn reset_nav(&mut self) {
        self.pos = None;
        self.saved_current.clear();
    }

    pub(in crate::ui) fn on_edit(&mut self) {
        if self.pos.is_some() {
            self.reset_nav();
        }
    }

    pub(in crate::ui) fn push(&mut self, cmd: &str, max: usize) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if self.entries.last().is_some_and(|x| x == cmd) {
            return;
        }
        self.entries.push(cmd.to_string());
        let max = max.max(1);
        if self.entries.len() > max {
            let drain = self.entries.len() - max;
            self.entries.drain(0..drain);
        }
    }

    pub(in crate::ui) fn prev(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let len = self.entries.len();
        let pos = match self.pos {
            None => {
                self.saved_current = current.to_string();
                len - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.pos = Some(pos);
        self.entries.get(pos).cloned()
    }

    pub(in crate::ui) fn next(&mut self) -> Option<String> {
        let pos = self.pos?;
        let len = self.entries.len();
        if pos + 1 >= len {
            self.pos = None;
            return Some(std::mem::take(&mut self.saved_current));
        }
        let pos = pos + 1;
        self.pos = Some(pos);
        self.entries.get(pos).cloned()
    }
}
