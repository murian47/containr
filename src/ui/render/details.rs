use std::fs;

use crate::docker::{ContainerRow, NetworkRow};
use crate::ui::render::format::format_action_ts;
use crate::ui::render::status::{action_error_details, action_error_label};
use crate::ui::render::table::{render_detail_table, DetailRow};
use crate::ui::render::text::truncate_end;
use crate::ui::render::text::short_commit;
use crate::ui::{
    action_status_prefix, current_match_pos, draw_shell_hr, image_update_indicator, image_update_view_for_ref,
    json_highlight_line, registry_auth_label, shell_header_style, shell_row_highlight,
    stack_name_from_labels, yaml_highlight_line, ActionErrorKind, App, ShellFocus, ShellView,
    StackDetailsFocus, TemplatesKind,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};
use time::OffsetDateTime;

pub(in crate::ui) fn draw_shell_main_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    match app.shell_view {
        ShellView::Dashboard => {}
        ShellView::Stacks => draw_shell_stack_details(f, app, area),
        ShellView::Containers => draw_shell_container_details(f, app, area),
        ShellView::Images => draw_shell_image_details(f, app, area),
        ShellView::Volumes => draw_shell_volume_details(f, app, area),
        ShellView::Networks => draw_shell_network_details(f, app, area),
        ShellView::Templates => draw_shell_template_details(f, app, area),
        ShellView::Registries => draw_shell_registry_details(f, app, area),
        ShellView::Logs => draw_shell_logs_meta(f, app, area),
        ShellView::Inspect => draw_shell_inspect_meta(f, app, area),
        ShellView::Help => draw_shell_help_meta(f, app, area),
        ShellView::Messages => draw_shell_messages_meta(f, app, area),
    }
}

fn draw_shell_container_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(c) = app.selected_container().cloned() else {
        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("Select a container to see details.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let mut scroll = app.container_details_scroll;
    if app.container_details_id.as_deref() != Some(&c.id) {
        app.container_details_id = Some(c.id.clone());
        scroll = 0;
    }
    let val = bg;
    let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
    let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
    let ip = app
        .ip_cache
        .get(&c.id)
        .map(|(ip, _)| ip.clone())
        .unwrap_or_else(|| "-".to_string());
    let (status_value, status_style) = if let Some(marker) = app.action_inflight.get(&c.id) {
        (
            action_status_prefix(marker.action).to_string(),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.container_action_error.get(&c.id) {
        let style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (action_error_label(err).to_string(), style)
    } else {
        (c.status.clone(), val)
    };
    let (update_text, update_style) = image_update_indicator(
        app,
        image_update_view_for_ref(app, &c.image).1,
        bg,
    );
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: c.name.clone(),
            style: val,
        },
        DetailRow {
            key: "ID",
            value: c.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Image",
            value: c.image.clone(),
            style: val,
        },
        DetailRow {
            key: "Update",
            value: update_text,
            style: update_style,
        },
        DetailRow {
            key: "Status",
            value: status_value,
            style: status_style,
        },
        DetailRow {
            key: "CPU / MEM",
            value: format!("{cpu} / {mem}"),
            style: val,
        },
        DetailRow {
            key: "IP",
            value: ip,
            style: val,
        },
        DetailRow {
            key: "Ports",
            value: c.ports.clone(),
            style: val,
        },
    ];
    if let Some(err) = app.container_action_error.get(&c.id) {
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.container_details_scroll = scroll;
}

fn draw_shell_stack_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);

    let Some(stack) = app.selected_stack_entry() else {
        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("Select a stack to see details.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    let mut containers: Vec<ContainerRow> = app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack.name.as_str()))
        .cloned()
        .collect();
    containers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let mut networks: Vec<NetworkRow> = app
        .networks
        .iter()
        .filter(|n| stack_name_from_labels(&n.labels).as_deref() == Some(stack.name.as_str()))
        .cloned()
        .collect();
    networks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    if containers.is_empty() && networks.is_empty() {
        f.render_widget(
            Paragraph::new("No stack resources found.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let focus = if app.shell_focus == ShellFocus::Details {
        app.stack_details_focus
    } else {
        StackDetailsFocus::Containers
    };
    let containers_focused = focus == StackDetailsFocus::Containers;
    let networks_focused = focus == StackDetailsFocus::Networks;

    if networks.is_empty() {
        draw_stack_containers_table(f, app, inner, &containers, true);
        return;
    }

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(65),
            Constraint::Length(1),
            Constraint::Percentage(35),
        ])
        .split(inner);
    draw_stack_containers_table(f, app, parts[0], &containers, containers_focused);
    draw_shell_hr(f, app, parts[1]);
    draw_stack_networks_table(f, app, parts[2], &networks, networks_focused);
}

fn draw_stack_containers_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    containers: &[ContainerRow],
    focused: bool,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if containers.is_empty() {
        f.render_widget(
            Paragraph::new("No containers in this stack.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let inner_height = inner.height.max(1) as usize;
    let header_rows = 1usize;
    let view_height = inner_height.saturating_sub(header_rows).max(1);
    let scroll = app
        .stacks_details_scroll
        .min(containers.len().saturating_sub(1));
    let rows: Vec<Row> = containers
        .iter()
        .skip(scroll)
        .take(view_height)
        .map(|c| {
            let status = if let Some(marker) = app.action_inflight.get(&c.id) {
                action_status_prefix(marker.action).to_string()
            } else if let Some(err) = app.container_action_error.get(&c.id) {
                action_error_label(err).to_string()
            } else {
                c.status.clone()
            };
            let status_style = if app.action_inflight.contains_key(&c.id) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.container_action_error.get(&c.id) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                bg
            };
            Row::new(vec![
                Cell::from(c.name.clone()),
                Cell::from(c.image.clone()),
                Cell::from(status).style(status_style),
                Cell::from(c.ports.clone()),
            ])
        })
        .collect();
    let header_style = if focused {
        shell_header_style(app)
    } else {
        bg.patch(app.theme.text_dim.to_style())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(26),
            Constraint::Length(28),
            Constraint::Length(14),
            Constraint::Min(12),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("CONTAINER"),
            Cell::from("IMAGE"),
            Cell::from("STATUS"),
            Cell::from("PORTS"),
        ])
        .style(header_style),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_widget(table, inner);
}

fn draw_stack_networks_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    networks: &[NetworkRow],
    focused: bool,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if networks.is_empty() {
        f.render_widget(
            Paragraph::new("No networks in this stack.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let inner_height = inner.height.max(1) as usize;
    let header_rows = 1usize;
    let view_height = inner_height.saturating_sub(header_rows).max(1);
    let scroll = app
        .stacks_networks_scroll
        .min(networks.len().saturating_sub(1));
    let rows: Vec<Row> = networks
        .iter()
        .skip(scroll)
        .take(view_height)
        .map(|n| {
            Row::new(vec![
                Cell::from(n.name.clone()),
                Cell::from(n.driver.clone()),
                Cell::from(n.scope.clone()),
            ])
        })
        .collect();
    let header_style = if focused {
        shell_header_style(app)
    } else {
        bg.patch(app.theme.text_dim.to_style())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![Cell::from("NETWORK"), Cell::from("DRIVER"), Cell::from("SCOPE")])
            .style(header_style),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_widget(table, inner);
}

fn draw_shell_image_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(img) = app.selected_image().cloned() else {
        return;
    };
    let mut scroll = app.image_details_scroll;
    if app.image_details_id.as_deref() != Some(&img.id) {
        app.image_details_id = Some(img.id.clone());
        scroll = 0;
    }
    let used_by = app
        .image_containers_by_id
        .get(&img.id)
        .cloned()
        .unwrap_or_default();
    let used_by = if used_by.is_empty() {
        "-".to_string()
    } else {
        used_by.join(", ")
    };
    let val = bg;
    let key = App::image_row_key(&img);
    let mut rows = vec![
        DetailRow {
            key: "Ref",
            value: img.name(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.image_action_inflight.contains_key(&key) {
                "removing".to_string()
            } else if let Some(err) = app.image_action_error.get(&key) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.image_action_inflight.contains_key(&key) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.image_action_error.get(&key) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "ID",
            value: img.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Size",
            value: img.size.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.image_action_error.get(&key) {
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.image_details_scroll = scroll;
}

fn draw_shell_volume_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(v) = app.selected_volume().cloned() else {
        return;
    };
    let mut scroll = app.volume_details_scroll;
    if app.volume_details_id.as_deref() != Some(&v.name) {
        app.volume_details_id = Some(v.name.clone());
        scroll = 0;
    }
    let used_by = app
        .volume_containers_by_name
        .get(&v.name)
        .map(|xs| xs.join(", "))
        .unwrap_or_else(|| "-".to_string());
    let val = bg;
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: v.name.clone(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.volume_action_inflight.contains_key(&v.name) {
                "removing".to_string()
            } else if let Some(err) = app.volume_action_error.get(&v.name) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.volume_action_inflight.contains_key(&v.name) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.volume_action_error.get(&v.name) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "Driver",
            value: v.driver.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.volume_action_error.get(&v.name) {
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v_style,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.volume_details_scroll = scroll;
}

fn draw_shell_network_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(n) = app.selected_network().cloned() else {
        return;
    };
    let mut scroll = app.network_details_scroll;
    if app.network_details_id.as_deref() != Some(&n.id) {
        app.network_details_id = Some(n.id.clone());
        scroll = 0;
    }
    let is_system = App::is_system_network(&n);
    let used_by = app
        .network_containers_by_id
        .get(&n.id)
        .cloned()
        .unwrap_or_default();
    let used_by = if used_by.is_empty() {
        "-".to_string()
    } else {
        used_by.join(", ")
    };
    let val = bg;
    let type_style = if is_system {
        bg.patch(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        bg.patch(Style::default().fg(Color::White))
    };
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: n.name.clone(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.network_action_inflight.contains_key(&n.id) {
                "removing".to_string()
            } else if let Some(err) = app.network_action_error.get(&n.id) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.network_action_inflight.contains_key(&n.id) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.network_action_error.get(&n.id) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "Type",
            value: if is_system { "System" } else { "User" }.to_string(),
            style: type_style,
        },
        DetailRow {
            key: "ID",
            value: n.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Driver",
            value: n.driver.clone(),
            style: val,
        },
        DetailRow {
            key: "Scope",
            value: n.scope.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.network_action_error.get(&n.id) {
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v_style,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.network_details_scroll = scroll;
}

fn draw_shell_stack_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);
    let status_area = parts[0];
    let content_area = parts[1];

    if let Some(err) = &app.templates_state.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_template().cloned() else {
        f.render_widget(
            Paragraph::new("No template selected.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    if !t.has_compose {
        f.render_widget(
            Paragraph::new("compose.yaml not found in template directory.")
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let content =
        fs::read_to_string(&t.compose_path).unwrap_or_else(|e| format!("read failed: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let lnw = lines.len().max(1).to_string().len();
    let view_h = content_area.height.max(1) as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.templates_state.templates_details_scroll = app
        .templates_state
        .templates_details_scroll
        .min(max_scroll);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len().max(1));
    let ln_style = bg.patch(app.theme.text_faint.to_style());

    for (i, l) in lines.iter().enumerate() {
        let ln = format!("{:>lnw$} ", i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(ln, ln_style));
        spans.extend(yaml_highlight_line(l, bg, &app.theme));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(format!("{:>lnw$} ", 1), ln_style)));
    }

    let (mut status_text, status_style) = if let Some(m) =
        app.templates_state.template_deploy_inflight.get(&t.name)
    {
        let secs = m.started.elapsed().as_secs();
        (
            format!("Status: deploying ({secs}s)"),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.template_action_error.get(&t.name) {
        let st = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (format!("Status: {}", action_error_label(err)), st)
    } else {
        ("Status: -".to_string(), bg.patch(app.theme.text_dim.to_style()))
    };
    let deploy_list = t
        .template_id
        .as_ref()
        .and_then(|id| app.template_deploys.get(id));
    let active_server = app.active_server.as_deref();
    let mut servers: Vec<String> = deploy_list
        .map(|list| list.iter().map(|info| info.server_name.clone()).collect())
        .unwrap_or_default();
    servers.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    servers.dedup();
    let servers_text = if servers.is_empty() {
        "-".to_string()
    } else {
        servers.join(", ")
    };
    if status_text == "Status: -" && servers_text != "-" {
        status_text = "Status: deployed".to_string();
    }
    let local_commit = app.templates_state.git_head.as_deref().map(short_commit);
    let deployed_commit = deploy_list.and_then(|list| {
        active_server
            .and_then(|srv| list.iter().find(|e| e.server_name == srv))
            .or_else(|| list.first())
            .and_then(|e| e.commit.as_deref())
            .map(short_commit)
    });
    let commit_text = match (local_commit, deployed_commit) {
        (Some(local), Some(deployed)) if local != deployed => {
            format!("Commit: local {local} | deployed {deployed}")
        }
        (Some(_local), Some(deployed)) => format!("Commit: {deployed} (local)"),
        (None, Some(deployed)) => format!("Commit: {deployed}"),
        (Some(local), None) => format!("Commit: local {local}"),
        (None, None) => "Commit: -".to_string(),
    };
    let info_style = bg.patch(app.theme.text_dim.to_style());
    let status_lines = Text::from(vec![
        Line::from(Span::styled(status_text, status_style)),
        Line::from(Span::styled(format!("Servers: {servers_text}"), info_style)),
        Line::from(Span::styled(commit_text, info_style)),
    ]);
    f.render_widget(
        Paragraph::new(status_lines).wrap(Wrap { trim: true }),
        status_area,
    );
    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_state
                .templates_details_scroll
                .min(u16::MAX as usize) as u16,
            0,
        )),
        content_area,
    );
}

fn draw_shell_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => draw_shell_stack_template_details(f, app, area),
        TemplatesKind::Networks => draw_shell_net_template_details(f, app, area),
    }
}

fn draw_shell_registry_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(r) = app
        .registries_cfg
        .registries
        .get(app.registries_selected)
        .cloned()
    else {
        return;
    };
    let mut scroll = app.registries_details_scroll;
    let host = r.host.trim().to_ascii_lowercase();
    let resolved = app.registry_auths.get(&host);
    let secret_status = if r.secret.as_ref().map(|s| s.trim()).unwrap_or("").is_empty() {
        "missing"
    } else if resolved.and_then(|a| a.secret.as_ref()).is_some() {
        "loaded"
    } else {
        "unavailable"
    };
    let username = r.username.clone().unwrap_or_else(|| "-".to_string());
    let test_repo = r.test_repo.clone().unwrap_or_else(|| "-".to_string());
    let (test_time, test_result) = if let Some(entry) = app.registry_tests.get(&host) {
        let ts = OffsetDateTime::from_unix_timestamp(entry.checked_at)
            .map(format_action_ts)
            .unwrap_or_else(|_| entry.checked_at.to_string());
        let status = if entry.ok { "ok" } else { "error" };
        let result = if entry.message.trim().is_empty() {
            status.to_string()
        } else {
            format!("{status}: {}", entry.message)
        };
        (ts, truncate_end(&result, 120))
    } else {
        ("-".to_string(), "-".to_string())
    };
    let val = bg;
    let rows = vec![
        DetailRow {
            key: "Host",
            value: r.host,
            style: val,
        },
        DetailRow {
            key: "Auth",
            value: registry_auth_label(&r.auth).to_string(),
            style: val,
        },
        DetailRow {
            key: "Username",
            value: username,
            style: val,
        },
        DetailRow {
            key: "Secret",
            value: secret_status.to_string(),
            style: val,
        },
        DetailRow {
            key: "Test repo",
            value: test_repo,
            style: val,
        },
        DetailRow {
            key: "Last test",
            value: test_time,
            style: val,
        },
        DetailRow {
            key: "Test result",
            value: test_result,
            style: val,
        },
        DetailRow {
            key: "Identity",
            value: app.registries_cfg.age_identity.clone(),
            style: val,
        },
    ];
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.registries_details_scroll = scroll;
}

fn draw_shell_net_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    let status_area = parts[0];
    let content_area = parts[1];

    if let Some(err) = &app.templates_state.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_net_template().cloned() else {
        f.render_widget(
            Paragraph::new("No network template selected.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    if !t.has_cfg {
        f.render_widget(
            Paragraph::new("network.json not found in template directory.")
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let content = fs::read_to_string(&t.cfg_path).unwrap_or_else(|e| format!("read failed: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let lnw = lines.len().max(1).to_string().len();
    let view_h = content_area.height.max(1) as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.templates_state.net_templates_details_scroll = app
        .templates_state
        .net_templates_details_scroll
        .min(max_scroll);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len().max(1));
    let ln_style = bg.patch(app.theme.text_faint.to_style());

    for (i, l) in lines.iter().enumerate() {
        let ln = format!("{:>lnw$} ", i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(ln, ln_style));
        spans.extend(json_highlight_line(l, bg, &app.theme));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(format!("{:>lnw$} ", 1), ln_style)));
    }

    let (status_text, status_style) = if let Some(m) =
        app.templates_state.net_template_deploy_inflight.get(&t.name)
    {
        let secs = m.started.elapsed().as_secs();
        (
            format!("Status: deploying ({secs}s)"),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.net_template_action_error.get(&t.name) {
        let st = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (format!("Status: {}", action_error_label(err)), st)
    } else {
        ("Status: -".to_string(), bg.patch(app.theme.text_dim.to_style()))
    };
    f.render_widget(
        Paragraph::new(status_text)
            .style(status_style)
            .wrap(Wrap { trim: true }),
        status_area,
    );
    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_state
                .net_templates_details_scroll
                .min(u16::MAX as usize) as u16,
            0,
        )),
        content_area,
    );
}

fn draw_shell_logs_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let q = app.logs.query.trim();
    let matches = if q.is_empty() {
        "Matches: -".to_string()
    } else if app.logs.use_regex && app.logs.regex_error.is_some() {
        "Regex: invalid".to_string()
    } else {
        format!("Matches: {}", app.logs.match_lines.len())
    };
    let re = if app.logs.use_regex { "regex:on" } else { "regex:off" };
    let pos = format!(
        "Line: {}/{}",
        app.logs.cursor.saturating_add(1),
        app.logs_total_lines().max(1)
    );
    let line = Line::from(vec![
        Span::styled(matches, Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("Query: ", Style::default().fg(Color::Gray)),
        Span::styled(
            if q.is_empty() { "-" } else { q },
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled(re, Style::default().fg(Color::Gray)),
        Span::raw("   "),
        Span::styled(pos, Style::default().fg(Color::Gray)),
    ]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_inspect_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let (cur, total) = current_match_pos(app);
    let matches = if app.inspect.query.trim().is_empty() {
        "Matches: -".to_string()
    } else {
        format!("Matches: {cur}/{total}")
    };
    let q = app.inspect.query.trim();
    let path = app
        .inspect
        .lines
        .get(app.inspect.selected)
        .map(|l| l.path.clone())
        .unwrap_or_else(|| "-".to_string());
    let line = Line::from(vec![
        Span::styled(matches, Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("Query: ", Style::default().fg(Color::Gray)),
        Span::styled(
            if q.is_empty() { "-" } else { q },
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled("Path: ", Style::default().fg(Color::Gray)),
        Span::styled(
            truncate_end(&path, inner.width.max(1) as usize / 2),
            Style::default().fg(Color::White),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_help_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let hint = "Use Up/Down/PageUp/PageDown to scroll. Press q to return.";
    f.render_widget(
        Paragraph::new(hint)
            .alignment(Alignment::Center)
            .style(bg.patch(app.theme.text_dim.to_style()))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_messages_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let hint = "Up/Down select  Left/Right hscroll  PageUp/PageDown  Home/End  ^c copy  q back";
    f.render_widget(
        Paragraph::new(hint)
            .style(bg.patch(app.theme.text_dim.to_style()))
            .wrap(Wrap { trim: true }),
        inner,
    );
}
