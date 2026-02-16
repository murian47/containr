//! Networks view scaffold (Phase 1)
#![allow(dead_code)]

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::{draw_shell_networks_table, App};

pub fn render_networks(f: &mut Frame, app: &mut App, area: Rect) {
    draw_shell_networks_table(f, app, area);
}
