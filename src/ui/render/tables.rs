use crate::docker::ContainerRow;
use crate::ui::core::types::{ActionErrorKind, ViewEntry};
use crate::ui::render::format::loading_spinner;
use crate::ui::render::status::image_update_indicator;
use crate::ui::render::status::{action_error_label, action_status_prefix};
use crate::ui::render::utils::{is_container_stopped, shell_row_highlight, truncate_end};
use crate::ui::state::app::App;
use crate::ui::state::image_updates::{resolve_image_update_state, resolve_stack_update_state};
use crate::ui::state::shell_types::ListMode;
use ratatui::layout::{Constraint, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap};

pub(in crate::ui) fn shell_header_style(app: &App) -> Style {
    app.theme.table_header.to_style()
}

pub(in crate::ui) fn draw_shell_containers_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: Rect,
) {
    // Reuse existing container row computation logic, but render without outer borders.
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

    // Keep the same column widths as before; only remove the visual separators.
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

pub(in crate::ui) fn draw_shell_images_table(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });

    const REF_TEXT_MAX: usize = 62;
    const ID_TEXT_MAX: usize = 50;
    const USED_W: usize = 3;
    const SIZE_W: usize = 10;
    const REF_MIN_W: usize = 24;
    const ID_MIN_W: usize = 10;

    let size_cell = |s: &str| -> String {
        // SIZE values are ASCII (e.g. "294MB", "2.06GB"), so fixed-width padding is fine.
        if s.chars().count() >= SIZE_W {
            truncate_end(s, SIZE_W)
        } else {
            format!("{:>width$}", s, width = SIZE_W)
        }
    };

    // Keep columns compact: size REF/ID to the actual visible content (capped),
    // but always reserve space for USED/SIZE.
    let mut max_ref = 0usize;
    let mut max_id = 0usize;
    let mut rows: Vec<Row> = Vec::new();
    for img in app
        .images
        .iter()
        .filter(|img| !app.images_unused_only || !app.image_referenced(img))
    {
        let reference_full = img.name();
        let reference = truncate_end(&reference_full, REF_TEXT_MAX);
        let id = truncate_end(&img.id, ID_TEXT_MAX);
        let key = App::image_row_key(img);
        let marked = app.is_image_marked(&key);
        let row_style = if marked {
            app.theme.marked.to_style()
        } else {
            Style::default()
        };
        let is_removing = app.image_action_inflight.contains_key(&key);
        let err = app.image_action_error.get(&key);
        let used = app
            .image_referenced_count_by_id
            .get(&img.id)
            .copied()
            .unwrap_or(0)
            > 0;
        let used_cell = if used {
            if app.ascii_only { "Y" } else { "✓" }
        } else {
            ""
        };
        let size = if is_removing {
            Cell::from(size_cell("removing")).style(bg.patch(app.theme.text_warn.to_style()))
        } else if let Some(err) = err {
            let style = match err.kind {
                ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
            };
            Cell::from(size_cell(action_error_label(err))).style(style)
        } else {
            Cell::from(size_cell(&img.size))
        };
        max_ref = max_ref.max(reference.chars().count());
        max_id = max_id.max(id.chars().count());
        rows.push(
            Row::new(vec![
                Cell::from(reference),
                Cell::from(id),
                Cell::from(used_cell).style(bg.patch(app.theme.text_ok.to_style())),
                size,
            ])
            .style(row_style),
        );
    }
    let inner_w = inner.width.max(1) as usize;
    let spacing = 3; // 4 columns => 3 spaces
    let fixed = USED_W + SIZE_W + spacing;
    let avail = inner_w.saturating_sub(fixed);

    let mut ref_w = max_ref.clamp(REF_MIN_W, REF_TEXT_MAX).min(avail);
    let mut id_w = max_id
        .clamp(ID_MIN_W, ID_TEXT_MAX)
        .min(avail.saturating_sub(ref_w));
    if ref_w + id_w < avail {
        let extra = avail - (ref_w + id_w);
        let add_ref = extra.min(REF_TEXT_MAX.saturating_sub(ref_w));
        ref_w += add_ref;
        let extra = extra - add_ref;
        id_w = (id_w + extra).min(ID_TEXT_MAX);
    }
    if avail > 0 {
        if ref_w == 0 {
            ref_w = 1.min(avail);
        }
        if id_w == 0 && avail > ref_w {
            id_w = 1;
        }
    }

    let mut state = TableState::default();
    state.select(Some(app.images_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Length(ref_w as u16),
            Constraint::Length(id_w as u16),
            Constraint::Length(USED_W as u16),
            Constraint::Length(SIZE_W as u16),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("REF"),
            Cell::from("ID"),
            Cell::from("USED"),
            Cell::from(size_cell("SIZE")),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

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

pub(in crate::ui) fn draw_shell_networks_table(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });

    let used_cell = |used: bool, bg: Style, app: &App| -> Cell<'static> {
        if !used {
            Cell::from("")
        } else if app.ascii_only {
            Cell::from("Y").style(bg.patch(app.theme.text_ok.to_style()))
        } else {
            Cell::from("✓").style(bg.patch(app.theme.text_ok.to_style()))
        }
    };

    let rows: Vec<Row> = app
        .networks
        .iter()
        .map(|n| {
            let marked = app.is_network_marked(&n.id);
            let st = if marked {
                app.theme.marked.to_style()
            } else if App::is_system_network(n) {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let used = app
                .network_referenced_count_by_id
                .get(&n.id)
                .copied()
                .unwrap_or(0)
                > 0;
            let is_removing = app.network_action_inflight.contains_key(&n.id);
            let err = app.network_action_error.get(&n.id);
            let scope_cell = if is_removing {
                Cell::from("removing").style(bg.patch(app.theme.text_warn.to_style()))
            } else if let Some(err) = err {
                let style = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                Cell::from(action_error_label(err)).style(style)
            } else {
                Cell::from(n.scope.clone())
            };
            Row::new(vec![
                Cell::from(n.name.clone()),
                Cell::from(n.id.clone()),
                Cell::from(n.driver.clone()),
                used_cell(used, bg, app),
                scope_cell,
            ])
            .style(st)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.networks_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            // Keep NAME compact so ID can expand.
            Constraint::Length(16),
            Constraint::Min(16),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("ID"),
            Cell::from("DRIVER"),
            Cell::from("USED"),
            Cell::from("SCOPE"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}
