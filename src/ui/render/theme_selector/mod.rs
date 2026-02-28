mod preview;
mod sidebar;

use crate::ui::state::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Block;

pub(in crate::ui) fn draw_theme_selector(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if app.shell_sidebar_collapsed { 18 } else { 28 }),
            Constraint::Min(1),
        ])
        .split(area);

    sidebar::draw_theme_selector_sidebar(f, app, cols[0]);
    preview::draw_theme_selector_preview(f, app, cols[1]);
}
