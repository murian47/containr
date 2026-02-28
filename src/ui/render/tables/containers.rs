use crate::docker::ContainerRow;
use crate::ui::core::types::{ActionErrorKind, ViewEntry};
use crate::ui::render::format::loading_spinner;
use crate::ui::render::status::image_update_indicator;
use crate::ui::render::status::{action_error_label, action_status_prefix};
use crate::ui::render::tables::common::shell_header_style;
use crate::ui::render::utils::{is_container_stopped, shell_row_highlight, truncate_end};
use crate::ui::state::app::App;
use crate::ui::state::image_updates::{resolve_image_update_state, resolve_stack_update_state};
use crate::ui::state::shell_types::ListMode;
use ratatui::layout::{Constraint, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap};

pub(in crate::ui) fn draw_shell_containers_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    app.ensure_view();
    if app.containers.is_empty() {
        let msg = if app.loading {
            let spinner = loading_spinner(app.loading_since);
            format!("Loading... {spinner}")
        } else if app.last_error.is_some() {
            "Failed to load (see status)".to_string()
        } else {
            "No containers".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            area.inner(Margin {
                vertical: 0,
                horizontal: 1,
            }),
        );
        return;
    }

    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });

    let header = Row::new(vec![
        Cell::from("NAME"),
        Cell::from("IMAGE"),
        Cell::from("UPD"),
        Cell::from("CPU"),
        Cell::from("MEM"),
        Cell::from("STATUS"),
        Cell::from("IP"),
    ])
    .style(shell_header_style(app));

    let mut rows: Vec<Row> = Vec::new();

    let make_container_row = |c: &ContainerRow, name_prefix: &str| -> Row {
        let stopped = is_container_stopped(&c.status);
        let marked = app.is_marked(&c.id);
        let row_style = if marked {
            app.theme.marked.to_style()
        } else if stopped {
            app.theme.text_faint.to_style().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };

        let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
        let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
        let ip = app
            .ip_cache
            .get(&c.id)
            .map(|(ip, _)| ip.as_str())
            .unwrap_or("-");
        let status = if app.is_stack_update_container(&c.id) {
            "Updating...".to_string()
        } else if let Some(marker) = app.action_inflight.get(&c.id) {
            action_status_prefix(marker.action).to_string()
        } else if let Some(err) = app.container_action_error.get(&c.id) {
            action_error_label(err).to_string()
        } else {
            c.status.clone()
        };
        let status_style =
            if app.is_stack_update_container(&c.id) || app.action_inflight.contains_key(&c.id) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.container_action_error.get(&c.id) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                row_style
            };

        let name = format!("{name_prefix}{}", c.name);
        let (upd_text, upd_style) =
            image_update_indicator(app, resolve_image_update_state(app, &c.image).1, bg);
        Row::new(vec![
            Cell::from(truncate_end(&name, 22)).style(row_style),
            Cell::from(truncate_end(&c.image, 40)).style(row_style),
            Cell::from(upd_text).style(upd_style),
            Cell::from(cpu).style(row_style),
            Cell::from(mem).style(row_style),
            Cell::from(status).style(status_style),
            Cell::from(truncate_end(ip, 15)).style(row_style),
        ])
        .style(row_style)
    };

    if app.list_mode == ListMode::Tree {
        for e in &app.view {
            match e {
                ViewEntry::StackHeader {
                    name,
                    total,
                    running,
                    expanded,
                } => {
                    let st = if *running == 0 {
                        app.theme.text_faint.to_style().add_modifier(Modifier::BOLD)
                    } else if *running == *total {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD)
                    };
                    let glyph = if *expanded { "▾" } else { "▸" };
                    let mut name_text = format!("{glyph} {name}");
                    if let Some(marker) = app.stack_update_inflight.get(name) {
                        let secs = marker.started.elapsed().as_secs();
                        name_text.push_str(&format!(" (Updating {secs}s)"));
                    }
                    let (upd_text, upd_style) = if app.stack_update_error.contains_key(name) {
                        ("!".to_string(), bg.patch(app.theme.text_error.to_style()))
                    } else {
                        image_update_indicator(app, resolve_stack_update_state(app, name), bg)
                    };
                    rows.push(
                        Row::new(vec![
                            Cell::from(name_text).style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
                            Cell::from(upd_text).style(upd_style),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                        ])
                        .style(st),
                    );
                }
                ViewEntry::UngroupedHeader { total, running } => {
                    let st = app.theme.text.to_style().add_modifier(Modifier::BOLD);
                    rows.push(
                        Row::new(vec![
                            Cell::from("Ungrouped").style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                        ])
                        .style(st),
                    );
                }
                ViewEntry::Container { id, indent, .. } => {
                    if let Some(idx) = app.container_idx_by_id.get(id).copied()
                        && let Some(c) = app.containers.get(idx)
                    {
                        let prefix = "  ".repeat(*indent);
                        rows.push(make_container_row(c, &prefix));
                    }
                }
            }
        }
    } else {
        for c in &app.containers {
            rows.push(make_container_row(c, ""));
        }
    }

    let widths = [
        Constraint::Length(22),
        Constraint::Min(20),
        Constraint::Length(3),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(22),
        Constraint::Length(15),
    ];

    let mut state = TableState::default();
    state.select(Some(app.selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(rows, widths)
        .header(header)
        .style(bg)
        .column_spacing(1)
        .row_highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}
