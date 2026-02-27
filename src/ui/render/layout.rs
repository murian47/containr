use crate::ui::render::details::draw_shell_main_details;
use crate::ui::render::messages::draw_shell_messages_dock;
use crate::ui::render::shell::draw_shell_main_list;
use crate::ui::render::sidebar::draw_shell_sidebar;
use crate::ui::render::theme_selector::draw_theme_selector;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellSplitMode, ShellView};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

pub(in crate::ui) fn draw_shell_body(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    if app.shell_view == ShellView::ThemeSelector {
        draw_theme_selector(f, app, area);
        return;
    }
    f.render_widget(Clear, area);
    let bg = app.theme.background.to_style();
    f.render_widget(ratatui::widgets::Block::default().style(bg), area);
    let dock_allowed = app.log_dock_enabled
        && !matches!(
            app.shell_view,
            ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages
        );
    if app.shell_sidebar_hidden {
        draw_shell_main_with_optional_dock(f, app, area, dock_allowed);
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if app.shell_sidebar_collapsed { 18 } else { 28 }),
            Constraint::Min(1),
        ])
        .split(area);
    draw_shell_sidebar(f, app, cols[0]);
    draw_shell_main_with_optional_dock(f, app, cols[1], dock_allowed);
}

fn draw_shell_main_with_optional_dock(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: Rect,
    dock_allowed: bool,
) {
    if !dock_allowed {
        draw_shell_main(f, app, area);
        return;
    }
    let dock_h = app
        .log_dock_height
        .min(area.height.saturating_sub(2).max(1));
    if area.height < dock_h + 2 {
        draw_shell_main(f, app, area);
        return;
    }
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(dock_h),
        ])
        .split(area);
    draw_shell_main(f, app, parts[0]);
    draw_shell_hr(f, app, parts[1]);
    draw_shell_messages_dock(f, app, parts[2]);
}

pub(in crate::ui) fn draw_shell_main(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    // Dashboard is a single-pane view (no details section).
    if app.shell_view == ShellView::Dashboard {
        draw_shell_main_list(f, app, area);
        return;
    }
    let is_full = matches!(app.shell_view, ShellView::Logs | ShellView::Inspect);
    let is_split_view = matches!(
        app.shell_view,
        ShellView::Stacks
            | ShellView::Containers
            | ShellView::Images
            | ShellView::Volumes
            | ShellView::Networks
            | ShellView::Templates
            | ShellView::Registries
    );

    if is_split_view && app.shell_split_mode == ShellSplitMode::Vertical {
        let parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Length(1),
                Constraint::Percentage(50),
            ])
            .split(area);
        draw_shell_main_list(f, app, parts[0]);
        draw_shell_vr(f, app, parts[1]);
        draw_shell_main_details(f, app, parts[2]);
        return;
    }

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            if matches!(
                app.shell_view,
                ShellView::Logs | ShellView::Inspect | ShellView::Messages | ShellView::Help
            ) {
                // Keep the meta area compact (3 lines) and centered.
                [
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                ]
            } else if is_full {
                [
                    Constraint::Percentage(85),
                    Constraint::Length(1),
                    Constraint::Percentage(15),
                ]
            } else {
                [
                    Constraint::Percentage(62),
                    Constraint::Length(1),
                    Constraint::Percentage(38),
                ]
            },
        )
        .split(area);

    draw_shell_main_list(f, app, parts[0]);
    draw_shell_hr(f, app, parts[1]);
    draw_shell_main_details(f, app, parts[2]);
}

pub(in crate::ui) fn draw_shell_hr(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let st = app.theme.divider.to_style();
    let line = "─".repeat(area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(line).style(st).wrap(Wrap { trim: false }),
        area,
    );
}

pub(in crate::ui) fn draw_shell_vr(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let st = app.theme.divider.to_style();
    let line = "│".repeat(area.height.max(1) as usize);
    f.render_widget(
        Paragraph::new(line).style(st).wrap(Wrap { trim: false }),
        area,
    );
}
