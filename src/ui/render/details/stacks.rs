use super::panel_bg;
use crate::docker::{ContainerRow, NetworkRow};
use crate::ui::core::types::TemplateDeployEntry;
use crate::ui::core::types::{ActionErrorKind, StackDetailsFocus};
use crate::ui::render::format::format_action_ts;
use crate::ui::render::layout::draw_shell_hr;
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::render::status::{action_error_label, action_status_prefix};
use crate::ui::render::tables::shell_header_style;
use crate::ui::render::text::short_commit;
use crate::ui::render::utils::shell_row_highlight;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellFocus;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};
use time::OffsetDateTime;

pub(super) fn draw_shell_stack_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);

    let Some(stack) = app.selected_stack_entry() else {
        let inner = area.inner(Margin {
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
    let template_id = app.stack_template_id(&stack.name);
    let active_server = app.active_server.clone();
    let deploy_history: Vec<TemplateDeployEntry> = template_id
        .as_deref()
        .and_then(|id| app.template_deploys.get(id))
        .cloned()
        .unwrap_or_default();
    let has_history = !deploy_history.is_empty();

    let inner = area.inner(Margin {
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

    if networks.is_empty() && !has_history {
        draw_stack_containers_table(f, app, inner, &containers, true);
        return;
    }

    let history_height = inner.height.saturating_div(4).clamp(4, 8);
    if networks.is_empty() {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(history_height),
            ])
            .split(inner);
        draw_stack_containers_table(f, app, parts[0], &containers, containers_focused);
        draw_shell_hr(f, app, parts[1]);
        draw_stack_deploy_history(
            f,
            app,
            parts[2],
            template_id.as_deref(),
            Some(deploy_history.as_slice()),
            active_server.as_deref(),
        );
        return;
    }

    if has_history {
        let networks_height = inner.height.saturating_div(3).clamp(4, 10);
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(networks_height),
                Constraint::Length(1),
                Constraint::Length(history_height),
            ])
            .split(inner);
        draw_stack_containers_table(f, app, parts[0], &containers, containers_focused);
        draw_shell_hr(f, app, parts[1]);
        draw_stack_networks_table(f, app, parts[2], &networks, networks_focused);
        draw_shell_hr(f, app, parts[3]);
        draw_stack_deploy_history(
            f,
            app,
            parts[4],
            template_id.as_deref(),
            Some(deploy_history.as_slice()),
            active_server.as_deref(),
        );
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
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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
            let status = if app.is_stack_update_container(&c.id) {
                "Updating...".to_string()
            } else if let Some(marker) = app.action_inflight.get(&c.id) {
                action_status_prefix(marker.action).to_string()
            } else if let Some(err) = app.container_action_error.get(&c.id) {
                action_error_label(err).to_string()
            } else {
                c.status.clone()
            };
            let status_style = if app.is_stack_update_container(&c.id)
                || app.action_inflight.contains_key(&c.id)
            {
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

fn draw_stack_deploy_history(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    template_id: Option<&str>,
    deploy_list: Option<&[TemplateDeployEntry]>,
    active_server: Option<&str>,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });
    let info_style = bg.patch(app.theme.text_dim.to_style());
    let header_style = info_style;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let template = template_id.unwrap_or("-");
    lines.push(Line::from(vec![
        Span::styled("DEPLOY HISTORY ", header_style),
        Span::styled(format!("(template {template})"), header_style),
    ]));
    lines.extend(stack_deploy_history_lines(
        deploy_list,
        active_server,
        info_style,
    ));
    let height = inner.height.max(1) as usize;
    let view = lines.into_iter().take(height).collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(Text::from(view))
            .style(bg)
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn stack_deploy_history_lines(
    deploy_list: Option<&[TemplateDeployEntry]>,
    active_server: Option<&str>,
    info_style: Style,
) -> Vec<Line<'static>> {
    let Some(list) = deploy_list else {
        return vec![Line::from(Span::styled(
            "No deploy history found.".to_string(),
            info_style,
        ))];
    };
    if list.is_empty() {
        return vec![Line::from(Span::styled(
            "No deploy history found.".to_string(),
            info_style,
        ))];
    }
    let mut entries = list.to_vec();
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let mut lines = vec![Line::from(Span::styled(
        format!("Deploys: {}", entries.len()),
        info_style,
    ))];
    let visible = 3usize;
    for entry in entries.iter().take(visible) {
        lines.push(Line::from(Span::styled(
            format_stack_deploy_entry(entry, active_server),
            info_style,
        )));
    }
    if entries.len() > visible {
        lines.push(Line::from(Span::styled(
            format!("... {} more", entries.len() - visible),
            info_style,
        )));
    }
    lines
}

fn format_stack_deploy_entry(entry: &TemplateDeployEntry, active_server: Option<&str>) -> String {
    let ts = OffsetDateTime::from_unix_timestamp(entry.timestamp)
        .map(format_action_ts)
        .unwrap_or_else(|_| entry.timestamp.to_string());
    let marker = if active_server
        .map(|server| server == entry.server_name)
        .unwrap_or(false)
    {
        "*"
    } else {
        "-"
    };
    let commit = entry
        .commit
        .as_deref()
        .map(short_commit)
        .unwrap_or_else(|| "-".to_string());
    format!("{marker} {ts} | {} | {commit}", entry.server_name)
}

fn draw_stack_networks_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    networks: &[NetworkRow],
    focused: bool,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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
        Row::new(vec![
            Cell::from("NETWORK"),
            Cell::from("DRIVER"),
            Cell::from("SCOPE"),
        ])
        .style(header_style),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_widget(table, inner);
}
