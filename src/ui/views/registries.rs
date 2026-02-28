//! Registries view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::ui::render::registries::draw_shell_registries_table;
use crate::ui::state::app::App;

pub fn render_registries(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_registries_table(f, app, area);
}
