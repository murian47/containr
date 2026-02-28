//! Messages view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::messages::draw_shell_messages_view;
use crate::ui::state::app::App;

pub fn render_messages(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_messages_view(f, app, area);
}
