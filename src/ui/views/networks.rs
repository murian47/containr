//! Networks view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::tables::draw_shell_networks_table;
use crate::ui::state::app::App;

pub fn render_networks(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_networks_table(f, app, area);
}
