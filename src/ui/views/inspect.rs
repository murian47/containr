//! Inspect view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::inspect::draw_shell_inspect_view;
use crate::ui::state::app::App;

pub fn render_inspect(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_inspect_view(f, app, area);
}
