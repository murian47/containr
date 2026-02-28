//! Root renderer for the shell UI.
//!
//! This module lays out the top-level shell frame and delegates individual regions to specialized
//! renderers. It is the highest-level rendering entrypoint used by the event loop.

use std::time::Duration;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::Block;

use crate::ui::state::app::App;

use super::footer::draw_shell_footer;
use super::layout::draw_shell_body;
use super::shell::{draw_shell_cmdline, draw_shell_header};

pub(in crate::ui) fn draw(f: &mut ratatui::Frame, app: &mut App, refresh: Duration) {
    draw_shell(f, app, refresh);
}

pub(in crate::ui) fn draw_shell(f: &mut ratatui::Frame, app: &mut App, refresh: Duration) {
    // Shell UI: header + sidebar + main + footer + command line. No overlays/dialogs.
    let area = f.area();
    let bg = app.theme.background.to_style();
    f.render_widget(Block::default().style(bg), area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
            Constraint::Length(1), // cmdline
        ])
        .split(area);

    draw_shell_header(f, app, refresh, rows[0]);
    draw_shell_body(f, app, rows[1]);
    draw_shell_footer(f, app, rows[2]);
    draw_shell_cmdline(f, app, rows[3]);
}
