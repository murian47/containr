//! Templates view rendering.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::layout::Constraint;
use ratatui::style::Modifier;
use std::time::Instant;

use crate::ui::render::status::action_error_label;
use crate::ui::{
    shell_header_style, shell_row_highlight, ActionErrorKind, App, GitRemoteStatus, TemplatesKind,
};

pub fn render_templates(f: &mut Frame, app: &mut App, area: Rect) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => draw_shell_stack_templates_table(f, app, area),
        TemplatesKind::Networks => draw_shell_net_templates_table(f, app, area),
    }
}

fn draw_shell_stack_templates_table(f: &mut Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if let Some(err) = &app.templates_state.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if app.templates_state.templates.is_empty() {
        let msg = format!("No templates in {}", app.stack_templates_dir().display());
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let now = Instant::now();
    let mut max_state = "STATE".chars().count();
    let active_server = app.active_server.as_deref();
    let git_status_cell =
        |dirty: bool, status: GitRemoteStatus, untracked: bool| -> Cell<'static> {
            let left = if dirty { "!" } else { "✓" };
            let left_style = if dirty {
                bg.patch(app.theme.text_warn.to_style())
            } else {
                bg.patch(app.theme.text_ok.to_style())
            };
            let (right, right_style) = if untracked {
                (" ", bg)
            } else {
                match status {
                    GitRemoteStatus::UpToDate => ("✓", bg.patch(app.theme.text_ok.to_style())),
                    GitRemoteStatus::Ahead => ("↑", bg.patch(app.theme.text_info.to_style())),
                    GitRemoteStatus::Behind => ("↓", bg.patch(app.theme.text_warn.to_style())),
                    GitRemoteStatus::Diverged => ("!", bg.patch(app.theme.text_error.to_style())),
                    GitRemoteStatus::Unknown => ("·", bg.patch(app.theme.text_dim.to_style())),
                }
            };
            Cell::from(Line::from(vec![
                Span::styled(left, left_style),
                Span::styled(right, right_style),
            ]))
        };
    let rows: Vec<Row> = app
        .templates_state
        .templates
        .iter()
        .map(|t| {
            let dirty = app.templates_state.dirty_templates.contains(&t.name);
            let untracked = app.templates_state.untracked_templates.contains(&t.name);
            let git_status = app
                .templates_state
                .git_remote_templates
                .get(&t.name)
                .copied()
                .unwrap_or(GitRemoteStatus::Unknown);
            let (deployed_any, deployed_on_active) = if let Some(id) = t.template_id.as_ref() {
                if let Some(list) = app.template_deploys.get(id) {
                    let any = !list.is_empty();
                    let on_active = active_server
                        .map(|srv| list.iter().any(|e| e.server_name == srv))
                        .unwrap_or(any);
                    (any, on_active)
                } else {
                    (false, false)
                }
            } else {
                (false, false)
            };
            let (state, state_style) = if let Some(m) =
                app.templates_state.template_deploy_inflight.get(&t.name)
            {
                let secs = now.duration_since(m.started).as_secs();
                (
                    format!("deploy {secs}s"),
                    Style::default().patch(app.theme.text_warn.to_style()),
                )
            } else if let Some(err) = app.template_action_error.get(&t.name) {
                let st = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                (action_error_label(err).to_string(), st)
            } else if deployed_any {
                ("deployed".to_string(), Style::default())
            } else {
                (String::new(), Style::default())
            };
            let row_style = if deployed_on_active
                || app
                    .templates_state
                    .template_deploy_inflight
                    .contains_key(&t.name)
            {
                Style::default()
            } else {
                bg.patch(app.theme.text_dim.to_style()).add_modifier(Modifier::DIM)
            };
            max_state = max_state.max(state.chars().count());
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_compose { "yes" } else { "no" }),
                Cell::from(state).style(state_style),
                git_status_cell(dirty, git_status, untracked),
                Cell::from(t.desc.clone()),
            ])
            .style(row_style)
        })
        .collect();
    let state_w = max_state.clamp(10, 22) as u16;

    let mut state = TableState::default();
    state.select(Some(
        app.templates_state
            .templates_selected
            .min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(state_w),
            Constraint::Length(3),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("COMPOSE"),
            Cell::from("STATE"),
            Cell::from("GIT"),
            Cell::from("DESC"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_net_templates_table(f: &mut Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if let Some(err) = &app.templates_state.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if app.templates_state.net_templates.is_empty() {
        let msg = format!("No network templates in {}", app.net_templates_dir().display());
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let now = Instant::now();
    let mut max_state = "STATE".chars().count();
    let active_server = app.active_server.as_deref();
    let git_status_cell =
        |dirty: bool, status: GitRemoteStatus, untracked: bool| -> Cell<'static> {
            let left = if dirty { "!" } else { "✓" };
            let left_style = if dirty {
                bg.patch(app.theme.text_warn.to_style())
            } else {
                bg.patch(app.theme.text_ok.to_style())
            };
            let (right, right_style) = if untracked {
                (" ", bg)
            } else {
                match status {
                    GitRemoteStatus::UpToDate => ("✓", bg.patch(app.theme.text_ok.to_style())),
                    GitRemoteStatus::Ahead => ("↑", bg.patch(app.theme.text_info.to_style())),
                    GitRemoteStatus::Behind => ("↓", bg.patch(app.theme.text_warn.to_style())),
                    GitRemoteStatus::Diverged => ("!", bg.patch(app.theme.text_error.to_style())),
                    GitRemoteStatus::Unknown => ("·", bg.patch(app.theme.text_dim.to_style())),
                }
            };
            Cell::from(Line::from(vec![
                Span::styled(left, left_style),
                Span::styled(right, right_style),
            ]))
        };
    let rows: Vec<Row> = app
        .templates_state
        .net_templates
        .iter()
        .map(|t| {
            let dirty = app.templates_state.dirty_net_templates.contains(&t.name);
            let untracked = app
                .templates_state
                .untracked_net_templates
                .contains(&t.name);
            let git_status = app
                .templates_state
                .git_remote_net_templates
                .get(&t.name)
                .copied()
                .unwrap_or(GitRemoteStatus::Unknown);
            let (deployed_any, deployed_on_active) =
                if let Some(list) = app.net_template_deploys.get(&t.name) {
                    let any = !list.is_empty();
                    let on_active = active_server
                        .map(|srv| list.iter().any(|e| e.server_name == srv))
                        .unwrap_or(any);
                    (any, on_active)
                } else {
                    (false, false)
                };
            let (state, state_style) = if let Some(m) =
                app.templates_state.net_template_deploy_inflight.get(&t.name)
            {
                let secs = now.duration_since(m.started).as_secs();
                (
                    format!("deploy {secs}s"),
                    Style::default().patch(app.theme.text_warn.to_style()),
                )
            } else if let Some(err) = app.net_template_action_error.get(&t.name) {
                let st = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                (action_error_label(err).to_string(), st)
            } else if deployed_any {
                ("deployed".to_string(), Style::default())
            } else {
                (String::new(), Style::default())
            };
            let row_style = if deployed_on_active
                || app
                    .templates_state
                    .net_template_deploy_inflight
                    .contains_key(&t.name)
            {
                Style::default()
            } else {
                bg.patch(app.theme.text_dim.to_style()).add_modifier(Modifier::DIM)
            };
            max_state = max_state.max(state.chars().count());
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_cfg { "yes" } else { "no" }),
                Cell::from(state).style(state_style),
                git_status_cell(dirty, git_status, untracked),
                Cell::from(t.desc.clone()),
            ])
            .style(row_style)
        })
        .collect();
    let state_w = max_state.clamp(10, 22) as u16;

    let mut state = TableState::default();
    state.select(Some(
        app.templates_state
            .net_templates_selected
            .min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(state_w),
            Constraint::Length(3),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("CFG"),
            Cell::from("STATE"),
            Cell::from("GIT"),
            Cell::from("DESC"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}
