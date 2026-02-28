use super::panel_bg;
use crate::ui::commands::git_cmd;
use crate::ui::core::types::ActionErrorKind;
use crate::ui::render::highlight::{json_highlight_line, yaml_highlight_line};
use crate::ui::render::status::action_error_label;
use crate::ui::render::text::short_commit;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::TemplatesKind;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};
use std::fs;

pub(super) fn draw_shell_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => draw_shell_stack_template_details(f, app, area),
        TemplatesKind::Networks => draw_shell_net_template_details(f, app, area),
    }
}

fn draw_shell_stack_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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
    app.templates_state.templates_details_scroll =
        app.templates_state.templates_details_scroll.min(max_scroll);

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

    let dirty = app.templates_state.dirty_templates.contains(&t.name);
    let (mut status_text, status_style) =
        if let Some(m) = app.templates_state.template_deploy_inflight.get(&t.name) {
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
        } else if dirty {
            (
                "Status: modified".to_string(),
                bg.patch(app.theme.text_warn.to_style()),
            )
        } else {
            (
                "Status: -".to_string(),
                bg.patch(app.theme.text_dim.to_style()),
            )
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
    let repo_commit = app.templates_state.git_head.clone();
    let template_commit = git_cmd::git_head_short(&t.dir);
    let local_commit = template_commit.clone().or(repo_commit.clone());
    let deployed_commit = deploy_list.and_then(|list| {
        active_server
            .and_then(|srv| list.iter().find(|e| e.server_name == srv))
            .or_else(|| list.first())
            .and_then(|e| e.commit.as_deref())
            .map(short_commit)
    });
    let info_style = bg.patch(app.theme.text_dim.to_style());
    let warn_style = bg.patch(app.theme.text_warn.to_style());
    if dirty && servers_text != "-" && !status_text.starts_with("Status: deploying") {
        status_text = "Status: deployed (modified)".to_string();
    }

    let commit_line = match (local_commit, deployed_commit) {
        (Some(local), Some(deployed)) if local != deployed => Line::from(Span::styled(
            format!("Commit: local {local} | deployed {deployed}"),
            info_style,
        )),
        (Some(_local), Some(deployed)) => Line::from(Span::styled(
            format!("Commit: {deployed} (local)"),
            info_style,
        )),
        (None, Some(deployed)) => {
            Line::from(Span::styled(format!("Commit: {deployed}"), info_style))
        }
        (Some(local), None) => {
            if let (Some(repo), Some(template)) =
                (repo_commit.as_deref(), template_commit.as_deref())
            {
                if repo != template {
                    return_line_with_git_mismatch(local, repo, info_style, warn_style)
                } else {
                    Line::from(Span::styled(format!("Commit: local {local}"), info_style))
                }
            } else if let Some(repo) = repo_commit.as_deref() {
                if local != repo {
                    return_line_with_git_mismatch(local, repo, info_style, warn_style)
                } else {
                    Line::from(Span::styled(format!("Commit: local {local}"), info_style))
                }
            } else {
                Line::from(Span::styled(format!("Commit: local {local}"), info_style))
            }
        }
        (None, None) => Line::from(Span::styled("Commit: -".to_string(), info_style)),
    };
    let status_lines = Text::from(vec![
        Line::from(Span::styled(status_text, status_style)),
        Line::from(Span::styled(format!("Servers: {servers_text}"), info_style)),
        commit_line,
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

fn return_line_with_git_mismatch(
    local: String,
    repo: &str,
    info: Style,
    warn: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled("Commit: local ", info),
        Span::styled(local, info),
        Span::styled(" / git ", info),
        Span::styled(repo.to_string(), warn),
    ])
}

fn draw_shell_net_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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

    let dirty = app.templates_state.dirty_net_templates.contains(&t.name);
    let (status_text, status_style) = if let Some(m) = app
        .templates_state
        .net_template_deploy_inflight
        .get(&t.name)
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
    } else if dirty {
        (
            "Status: modified".to_string(),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else {
        (
            "Status: -".to_string(),
            bg.patch(app.theme.text_dim.to_style()),
        )
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
