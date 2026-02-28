//! Containers view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::App;
use crate::ui::render::tables::draw_shell_containers_table;

pub fn render_containers(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_containers_table(f, app, area);
}
