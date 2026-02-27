#![allow(dead_code)]

use ratatui::layout::Constraint;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::ui::render::status::image_update_indicator;
use crate::ui::render::tables::shell_header_style;
use crate::ui::render::status::{action_error_label, action_status_prefix};
use crate::ui::render::utils::shell_row_highlight;
use crate::ui::state::image_updates::resolve_stack_update_state;
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::core::types::ActionErrorKind;
use crate::ui::state::app::App;

/// Render Stacks table (moved from render.inc.rs)
pub fn render_stacks_impl(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if app.stacks.is_empty() {
        f.render_widget(
            Paragraph::new("No stacks found (no compose/stack labels).")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows: Vec<Row> = app
        .stacks
        .iter()
        .map(|s| {
            let row_style = if s.running == 0 {
                bg.patch(app.theme.text_dim.to_style())
            } else {
                Style::default()
            };
            let (upd_text, upd_style) = image_update_indicator(
                app,
                resolve_stack_update_state(app, &s.name),
                bg,
            );
            let mut state = String::new();
            let mut state_style = row_style;
            for c in app
                .containers
                .iter()
                .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(s.name.as_str()))
            {
                if let Some(marker) = app.action_inflight.get(&c.id) {
                    state = action_status_prefix(marker.action).to_string();
                    state_style = bg.patch(app.theme.text_warn.to_style());
                    break;
                }
                if let Some(err) = app.container_action_error.get(&c.id) {
                    state = action_error_label(err).to_string();
                    state_style = match err.kind {
                        ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                        ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                    };
                    break;
                }
            }

            let name_cell = if let Some(marker) = app.stack_update_inflight.get(&s.name) {
                let secs = marker.started.elapsed().as_secs();
                Cell::from(Line::from(vec![
                    Span::raw(s.name.clone()),
                    Span::styled(
                        format!(" (Updating {secs}s)"),
                        bg.patch(app.theme.text_warn.to_style()),
                    ),
                ]))
            } else if state.is_empty() {
                Cell::from(s.name.clone())
            } else {
                Cell::from(Line::from(vec![
                    Span::raw(s.name.clone()),
                    Span::styled(format!(" ({state})"), state_style),
                ]))
            };
            let row = Row::new(vec![
                name_cell,
                Cell::from(upd_text).style(upd_style),
                Cell::from(s.total.to_string()),
                Cell::from(s.running.to_string()),
            ]);
            row.style(row_style)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.stacks_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Min(26),
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("UPD"),
            Cell::from("TOTAL"),
            Cell::from("RUN"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

/// Public wrapper for callers (keeps existing call sites stable).
pub fn render_stacks(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    render_stacks_impl(f, app, area);
}
