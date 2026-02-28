mod sections;

use crate::ui::state::app::App;
use ratatui::layout::{Margin, Rect};
use ratatui::widgets::{Block, Paragraph, Wrap};
pub use sections::shell_help_lines;

pub(in crate::ui) fn draw_shell_help_view(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });

    let lines = shell_help_lines(&app.theme);
    let total = lines.len().max(1);
    let view_h = inner.height.max(1) as usize;
    let max_scroll = total.saturating_sub(view_h);
    let top = if app.shell_help.scroll == usize::MAX {
        max_scroll
    } else {
        app.shell_help.scroll.min(max_scroll)
    };
    app.shell_help.scroll = top;
    let shown: Vec<_> = lines.into_iter().skip(top).take(view_h).collect();
    f.render_widget(
        Paragraph::new(shown).style(bg).wrap(Wrap { trim: false }),
        inner,
    );
}
