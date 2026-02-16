#![allow(dead_code)]

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::{draw_shell_dashboard, App};

/// Thin wrapper to call existing render logic (Phase 2).
pub fn render_dashboard(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_dashboard(f, app, area);
}
