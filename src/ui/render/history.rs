use crate::ui::render::format::format_action_ts;
use crate::ui::render::scroll::draw_shell_scrollbar_v;
use crate::ui::render::tables::shell_header_style;
use crate::ui::render::text::short_commit;
use crate::ui::render::utils::shell_row_highlight;
use crate::ui::state::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap};
use time::OffsetDateTime;

pub(in crate::ui) fn draw_shell_history_view(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    if app.deploy_history.entries.is_empty() {
        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("No deploy history entries.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        app.deploy_history.selected = 0;
        app.deploy_history.scroll_top = 0;
        return;
    }

    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });
    let layout = if inner.width > 8 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(inner)
    };
    let table_area = layout[0];
    let scrollbar_area = layout[1];

    let total = app.deploy_history.entries.len();
    app.deploy_history.selected = app.deploy_history.selected.min(total.saturating_sub(1));
    let header_rows = 1usize;
    let view_height = table_area.height.max(1) as usize;
    let body_height = view_height.saturating_sub(header_rows).max(1);

    let mut top = app
        .deploy_history
        .scroll_top
        .min(total.saturating_sub(body_height));
    let cursor = app.deploy_history.selected;
    if cursor < top {
        top = cursor;
    } else if cursor >= top.saturating_add(body_height) {
        top = cursor
            .saturating_sub(body_height.saturating_sub(1))
            .min(total.saturating_sub(body_height));
    }
    app.deploy_history.scroll_top = top;

    let rows: Vec<Row> = app
        .deploy_history
        .entries
        .iter()
        .skip(top)
        .take(body_height)
        .map(|entry| {
            let ts = OffsetDateTime::from_unix_timestamp(entry.timestamp)
                .map(format_action_ts)
                .unwrap_or_else(|_| entry.timestamp.to_string());
            let commit = entry
                .commit
                .as_deref()
                .map(short_commit)
                .unwrap_or_else(|| "-".to_string());
            Row::new(vec![
                Cell::from(ts),
                Cell::from(entry.server_name.clone()),
                Cell::from(commit),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(24),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("TIMESTAMP"),
            Cell::from("SERVER"),
            Cell::from("COMMIT"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    let mut state = TableState::default().with_selected(Some(cursor - top));
    f.render_stateful_widget(table, table_area, &mut state);

    if scrollbar_area.width > 0 {
        let max_scroll = total.saturating_sub(body_height);
        draw_shell_scrollbar_v(
            f,
            scrollbar_area,
            top,
            max_scroll,
            total,
            body_height,
            app.ascii_only,
            &app.theme,
        );
    }
}
