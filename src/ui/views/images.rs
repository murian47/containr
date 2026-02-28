//! Images view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::tables::draw_shell_images_table;
use crate::ui::state::app::App;

pub fn render_images(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_images_table(f, app, area);
}
