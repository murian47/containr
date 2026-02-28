//! Inspect view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::App;
use crate::ui::render::inspect::draw_shell_inspect_view;

pub fn render_inspect(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_inspect_view(f, app, area);
}
