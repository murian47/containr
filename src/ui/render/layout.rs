use crate::ui::{
    App, ShellSplitMode, ShellView, draw_shell_main_details, draw_shell_main_list,
    draw_shell_sidebar,
};
use crate::ui::render::theme_selector::draw_theme_selector;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Paragraph, Wrap};

pub(in crate::ui) fn draw_shell_body(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    if app.shell_view == ShellView::ThemeSelector {
        draw_theme_selector(f, app, area);
        return;
    }
    if app.shell_sidebar_hidden {
        draw_shell_main(f, app, area);
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
    draw_shell_main(f, app, cols[1]);
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
