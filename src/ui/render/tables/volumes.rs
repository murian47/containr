use crate::ui::core::types::ActionErrorKind;
use crate::ui::render::status::action_error_label;
use crate::ui::render::tables::common::shell_header_style;
use crate::ui::render::utils::shell_row_highlight;
use crate::ui::state::app::App;
use ratatui::layout::{Constraint, Margin, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Cell, Row, Table, TableState};

pub(in crate::ui) fn draw_shell_volumes_table(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });

    let used_cell = |used: usize, bg: Style, app: &App| -> Cell<'static> {
        if used == 0 {
            Cell::from("")
        } else if app.ascii_only {
            Cell::from("Y").style(bg.patch(app.theme.text_ok.to_style()))
        } else {
            Cell::from("✓").style(bg.patch(app.theme.text_ok.to_style()))
        }
    };

    let rows: Vec<Row> = app
        .volumes
        .iter()
        .filter(|v| !app.volumes_unused_only || !app.volume_referenced(v))
        .map(|v| {
            let used = app
                .volume_referenced_count_by_name
                .get(&v.name)
                .copied()
                .unwrap_or(0);
            let marked = app.is_volume_marked(&v.name);
            let st = if marked {
                app.theme.marked.to_style()
            } else {
                Style::default()
            };
            let is_removing = app.volume_action_inflight.contains_key(&v.name);
            let err = app.volume_action_error.get(&v.name);
            let used_cell = if is_removing {
                Cell::from("removing").style(bg.patch(app.theme.text_warn.to_style()))
            } else if let Some(err) = err {
                let style = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                Cell::from(action_error_label(err)).style(style)
            } else {
                used_cell(used, bg, app)
            };
            Row::new(vec![
                Cell::from(v.name.clone()),
                Cell::from(v.driver.clone()),
                used_cell,
            ])
            .style(st)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(app.volumes_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(10),
            Constraint::Length(3),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("DRIVER"),
            Cell::from("USED"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}
