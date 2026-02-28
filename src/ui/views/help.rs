//! Help view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::App;
use crate::ui::render::help::draw_shell_help_view;

pub fn render_help(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_help_view(f, app, area);
}
