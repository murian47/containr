#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::history::draw_shell_history_view;
use crate::ui::state::app::App;

pub fn render_history(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_history_view(f, app, area);
}
