//! Dashboard view scaffold (Phase 1)
//! Die eigentliche Render-Logik bleibt vorerst in `render.inc.rs`.

#![allow(dead_code)]

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::{draw_shell_dashboard, App};

/// Thin wrapper to call existing render logic (Phase 1).
pub fn render_dashboard(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_dashboard(f, app, area);
}
