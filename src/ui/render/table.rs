use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Wrap};

use crate::ui::render::format::{pad_right, wrap_text};
use crate::ui::render::text::truncate_end;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellFocus;

#[derive(Debug, Clone)]
pub(in crate::ui) struct DetailRow {
    pub key: &'static str,
    pub value: String,
    pub style: ratatui::style::Style,
}

pub(in crate::ui) fn render_detail_table(
    f: &mut ratatui::Frame,
    app: &App,
    area: ratatui::layout::Rect,
    mut rows: Vec<DetailRow>,
    scroll: usize,
) -> usize {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let table_w = inner.width.max(1) as usize;
    let key_w = 12usize.min(table_w.saturating_sub(1).max(1));
    let val_w = table_w.saturating_sub(key_w + 1).max(1);
    let key_style = bg.patch(app.theme.text_dim.to_style());

    let mut out_lines: Vec<Line<'static>> = Vec::new();
    for row in rows.drain(..) {
        let wrap = matches!(row.key, "Last error" | "Used by");
        let wrapped = if wrap {
            wrap_text(&row.value, val_w)
        } else {
            vec![truncate_end(&row.value, val_w)]
        };
        for (idx, line) in wrapped.into_iter().enumerate() {
            let key = if idx == 0 { row.key } else { "" };
            let key = pad_right(key, key_w);
            out_lines.push(Line::from(vec![
                Span::styled(key, key_style),
                Span::styled(line, row.style),
            ]));
        }
    }

    let max_scroll = out_lines.len().saturating_sub(inner.height.max(1) as usize);
    let scroll = scroll.min(max_scroll);
    let scroll_u16 = scroll.min(u16::MAX as usize) as u16;
    let para = Paragraph::new(out_lines)
        .style(bg)
        .wrap(Wrap { trim: false })
        .scroll((scroll_u16, 0));
    f.render_widget(para, inner);
    scroll
}
