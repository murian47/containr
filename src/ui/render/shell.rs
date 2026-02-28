//! Shell chrome renderers.
//!
//! Header, title, main list dispatch, and command line rendering live here. These functions define
//! the visual shell around the domain-specific list/detail views.

use crate::ui::commands;
use crate::ui::core::runtime::current_server_label;
use crate::ui::core::types::{InspectMode, LogsMode};
use crate::ui::render::badges::header_logo_spans;
use crate::ui::render::breadcrumbs::shell_breadcrumbs;
use crate::ui::render::format::{dot_spinner, spinner_char, split_at_chars, truncate_start};
use crate::ui::render::header::draw_rate_limit_banner;
use crate::ui::render::utils::truncate_end;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{
    ShellFocus, ShellView, TemplatesKind, input_window_with_cursor,
};
use crate::ui::theme;
use crate::ui::views;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use std::time::Duration;

pub(in crate::ui) fn draw_shell_header(
    f: &mut ratatui::Frame,
    app: &App,
    _refresh: Duration,
    area: Rect,
) {
    let bg = app.theme.header.to_style();
    f.render_widget(Block::default().style(bg), area);

    let server = current_server_label(app);
    let crumb = shell_breadcrumbs(app);
    let conn = if app.conn_error.is_some() {
        "○"
    } else {
        "●"
    };
    let conn_style = if app.conn_error.is_some() {
        app.theme
            .text_error
            .to_style()
            .bg(theme::parse_color(&app.theme.header.bg))
    } else {
        app.theme
            .text_ok
            .to_style()
            .bg(theme::parse_color(&app.theme.header.bg))
    };

    let left = " CONTAINR  ";
    let unseen_errors = app.unseen_error_count();
    let err_badge = if unseen_errors > 0 {
        format!("  !{unseen_errors}")
    } else {
        String::new()
    };
    let deploy =
        if let Some((name, marker)) = app.templates_state.template_deploy_inflight.iter().next() {
            let secs = marker.started.elapsed().as_secs();
            let spin = spinner_char(marker.started, app.ascii_only);
            format!("  Deploy: {name} {spin} {secs}s")
        } else {
            String::new()
        };
    let mut global_loading = app.logs.loading
        || app.inspect.loading
        || !app.action_inflight.is_empty()
        || !app.image_action_inflight.is_empty()
        || !app.volume_action_inflight.is_empty()
        || !app.network_action_inflight.is_empty()
        || !app.templates_state.template_deploy_inflight.is_empty()
        || !app.templates_state.net_template_deploy_inflight.is_empty()
        || !app.stack_update_inflight.is_empty()
        || !app.image_updates_inflight.is_empty();
    if app.server_all_selected {
        global_loading = global_loading || app.dashboard_all.hosts.iter().any(|h| h.loading);
    } else {
        global_loading = global_loading || app.loading || app.dashboard.loading;
    }
    let refresh_icon = if app.ascii_only { "r" } else { "⏱" };
    let refresh_label = format!("{refresh_icon} {}s", app.refresh_secs.max(1));
    let commit_label = if commands::git_cmd::git_available() && app.git_autocommit {
        "  Commit: auto"
    } else {
        ""
    };
    let mid = format!(
        "Server: {server}  {conn} connected{err_badge}  {refresh_label}{commit_label}  View: {}{crumb}{deploy}",
        app.shell_view.title(),
    );
    let right = if global_loading {
        dot_spinner(app.ascii_only).to_string()
    } else {
        String::new()
    };

    let w = area.width.max(1) as usize;
    let mut line = String::new();
    line.push_str(left);
    line.push_str(&mid);
    let min_right = right.chars().count();
    let shown = truncate_end(&line, w.saturating_sub(min_right));
    let rem = w.saturating_sub(shown.chars().count());
    let right_shown = truncate_start(&right, rem);
    let right_len = right_shown.chars().count();
    let gap = rem.saturating_sub(right_len);

    let mut spans: Vec<Span> = Vec::new();
    let (logo, rest) = split_at_chars(&shown, left.chars().count());
    spans.extend(header_logo_spans(app, bg, logo));
    // Bolden breadcrumb for better scanability.
    if !crumb.is_empty() && rest.contains(&crumb) {
        let mut parts = rest.splitn(2, &crumb);
        let before = parts.next().unwrap_or_default();
        let after = parts.next().unwrap_or_default();
        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), bg));
        }
        spans.push(Span::styled(crumb.clone(), bg.add_modifier(Modifier::BOLD)));
        if !after.is_empty() {
            spans.push(Span::styled(after.to_string(), bg));
        }
    } else {
        spans.push(Span::styled(rest.to_string(), bg));
    }
    // Color the connection dot to reflect current status.
    if spans
        .iter()
        .map(|s| s.content.clone())
        .collect::<String>()
        .contains(conn)
    {
        // If the conn symbol is inside existing spans, split the last span that contains it.
        let mut updated: Vec<Span> = Vec::new();
        for s in spans.into_iter() {
            if s.content.contains(conn) {
                let parts: Vec<&str> = s.content.split(conn).collect();
                if parts.len() == 2 {
                    updated.push(Span::styled(parts[0].to_string(), s.style));
                    updated.push(Span::styled(conn.to_string(), conn_style));
                    updated.push(Span::styled(parts[1].to_string(), s.style));
                } else {
                    updated.push(s);
                }
            } else {
                updated.push(s);
            }
        }
        spans = updated;
    }
    // Color the error badge.
    if unseen_errors > 0 {
        let badge = format!("!{unseen_errors}");
        let mut updated: Vec<Span> = Vec::new();
        for s in spans.into_iter() {
            if s.content.contains(&badge) {
                let parts: Vec<&str> = s.content.split(&badge).collect();
                if parts.len() == 2 {
                    updated.push(Span::styled(parts[0].to_string(), s.style));
                    let badge_style = app
                        .theme
                        .text_error
                        .to_style()
                        .bg(theme::parse_color(&app.theme.header.bg))
                        .add_modifier(Modifier::BOLD);
                    updated.push(Span::styled(badge.clone(), badge_style));
                    updated.push(Span::styled(parts[1].to_string(), s.style));
                } else {
                    updated.push(s);
                }
            } else {
                updated.push(s);
            }
        }
        spans = updated;
    }
    if !right_shown.is_empty() {
        if gap > 0 {
            spans.push(Span::styled(" ".repeat(gap), bg));
        }
        spans.push(Span::styled(right_shown, bg.fg(Color::Gray)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(in crate::ui) fn draw_shell_title(
    f: &mut ratatui::Frame,
    app: &App,
    title: &str,
    count: usize,
    area: Rect,
) {
    // Subtle focus indication: highlight the list title when list has focus.
    let bg = if app.shell_focus == ShellFocus::List {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let left = if count == usize::MAX {
        format!(" {title}")
    } else {
        format!(" {title} ({count})")
    };
    let shown = truncate_end(&left, area.width.max(1) as usize);
    let fg = if app.shell_focus == ShellFocus::List {
        theme::parse_color(&app.theme.panel_focused.fg)
    } else {
        theme::parse_color(&app.theme.syntax_text.fg)
    };
    f.render_widget(
        Paragraph::new(shown)
            .style(bg.fg(fg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(in crate::ui) fn draw_shell_main_list(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let banner = if matches!(
        app.shell_view,
        ShellView::Logs | ShellView::Inspect | ShellView::Messages | ShellView::Help
    ) {
        None
    } else {
        app.status_banner()
    };
    let (title_area, banner_area, content_area) = if banner.is_some() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(area);
        (chunks[0], Some(chunks[1]), chunks[2])
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        (chunks[0], None, chunks[1])
    };

    match app.shell_view {
        ShellView::Dashboard => {
            draw_shell_title(f, app, "Dashboard", usize::MAX, title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::dashboard::render_dashboard(f, app, content_area);
        }
        ShellView::Stacks => {
            draw_shell_title(f, app, "Stacks", app.stacks.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::stacks::render_stacks(f, app, content_area);
        }
        ShellView::Containers => {
            draw_shell_title(f, app, "Containers", app.containers.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::containers::render_containers(f, app, content_area);
        }
        ShellView::Images => {
            draw_shell_title(f, app, "Images", app.images_visible_len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::images::render_images(f, app, content_area);
        }
        ShellView::Volumes => {
            draw_shell_title(f, app, "Volumes", app.volumes_visible_len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::volumes::render_volumes(f, app, content_area);
        }
        ShellView::Networks => {
            draw_shell_title(f, app, "Networks", app.networks.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::networks::render_networks(f, app, content_area);
        }
        ShellView::Templates => match app.templates_state.kind {
            TemplatesKind::Stacks => {
                draw_shell_title(
                    f,
                    app,
                    "Templates: Stacks",
                    app.templates_state.templates.len(),
                    title_area,
                );
                if let Some(area) = banner_area {
                    draw_rate_limit_banner(f, app, banner, area);
                }
                views::templates::render_templates(f, app, content_area);
            }
            TemplatesKind::Networks => {
                draw_shell_title(
                    f,
                    app,
                    "Templates: Networks",
                    app.templates_state.net_templates.len(),
                    title_area,
                );
                if let Some(area) = banner_area {
                    draw_rate_limit_banner(f, app, banner, area);
                }
                views::templates::render_templates(f, app, content_area);
            }
        },
        ShellView::Registries => {
            draw_shell_title(
                f,
                app,
                "Registries",
                app.registries_cfg.registries.len(),
                title_area,
            );
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            views::registries::render_registries(f, app, content_area);
        }
        ShellView::Logs => {
            draw_shell_title(f, app, "Logs", app.logs_total_lines(), title_area);
            views::logs::render_logs(f, app, content_area);
        }
        ShellView::Inspect => {
            draw_shell_title(f, app, "Inspect", app.inspect.lines.len(), title_area);
            views::inspect::render_inspect(f, app, content_area);
        }
        ShellView::Help => {
            draw_shell_title(f, app, "Help", 0, title_area);
            views::help::render_help(f, app, content_area);
        }
        ShellView::Messages => {
            draw_shell_title(f, app, "Messages", app.session_msgs.len(), title_area);
            views::messages::render_messages(f, app, content_area);
        }
        ShellView::ThemeSelector => {
            draw_shell_title(f, app, "Themes", 0, title_area);
        }
    }
}

pub(in crate::ui) fn draw_shell_cmdline(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let bg = app.theme.cmdline.to_style();
    f.render_widget(Block::default().style(bg), area);

    let (mode, prefix, input, cursor, show_cursor): (&str, &str, String, usize, bool) =
        if app.shell_cmdline.mode {
            if let Some(confirm) = &app.shell_cmdline.confirm {
                ("CONFIRM", ":", format!("{} (y/n)", confirm.label), 0, false)
            } else {
                (
                    "COMMAND",
                    ":",
                    app.shell_cmdline.input.clone(),
                    app.shell_cmdline.cursor,
                    true,
                )
            }
        } else {
            match app.shell_view {
                ShellView::Logs => match app.logs.mode {
                    LogsMode::Normal => ("CONTAINR", "", String::new(), 0, false),
                    LogsMode::Search => (
                        "SEARCH",
                        "/",
                        app.logs.input.clone(),
                        app.logs.input_cursor,
                        true,
                    ),
                    LogsMode::Command => (
                        "COMMAND",
                        ":",
                        app.logs.command.clone(),
                        app.logs.command_cursor,
                        true,
                    ),
                },
                ShellView::Inspect => match app.inspect.mode {
                    InspectMode::Normal => ("CONTAINR", "", String::new(), 0, false),
                    InspectMode::Search => (
                        "SEARCH",
                        "/",
                        app.inspect.input.clone(),
                        app.inspect.input_cursor,
                        true,
                    ),
                    InspectMode::Command => (
                        "COMMAND",
                        ":",
                        app.inspect.input.clone(),
                        app.inspect.input_cursor,
                        true,
                    ),
                },
                ShellView::ThemeSelector => {
                    if app.theme_selector.search_mode {
                        (
                            "SEARCH",
                            "/",
                            app.theme_selector.search_input.clone(),
                            app.theme_selector.search_cursor,
                            true,
                        )
                    } else {
                        ("CONTAINR", "", String::new(), 0, false)
                    }
                }
                ShellView::Messages | ShellView::Help => ("CONTAINR", "", String::new(), 0, false),
                _ => ("CONTAINR", "", String::new(), 0, false),
            }
        };

    let w = area.width.max(1) as usize;
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(
        format!(" {mode} "),
        app.theme.cmdline_label.to_style(),
    ));

    if !prefix.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

        let fixed_len = format!(" {mode} ").chars().count() + 1 + prefix.chars().count();
        let avail = w.saturating_sub(fixed_len).max(1);
        if show_cursor {
            let input_w = avail.saturating_sub(1).max(1);
            let (before, at, after) = input_window_with_cursor(&input, cursor, input_w);
            spans.push(Span::styled(before, bg));
            spans.push(Span::styled(at, app.theme.cmdline_cursor.to_style()));
            spans.push(Span::styled(after, bg));
        } else {
            spans.push(Span::styled(
                truncate_end(&input, avail),
                app.theme.cmdline_inactive.to_style(),
            ));
        }
    } else {
        spans.push(Span::styled(
            "  (press : for commands)",
            app.theme.text_faint.to_style(),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}
