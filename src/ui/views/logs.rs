//! Logs view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::logs::draw_shell_logs_view;
use crate::ui::state::app::App;

pub fn render_logs(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_logs_view(f, app, area);
}
