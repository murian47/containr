//! Containers view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::render::tables::draw_shell_containers_table;
use crate::ui::App;

pub fn render_containers(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_containers_table(f, app, area);
}
