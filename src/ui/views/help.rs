//! Help view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::{draw_shell_help_view, App};

pub fn render_help(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_help_view(f, app, area);
}
