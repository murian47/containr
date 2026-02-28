use super::panel_bg;
use crate::ui::App;
use crate::ui::render::inspect::current_match_pos;
use crate::ui::render::text::truncate_end;
use ratatui::layout::{Alignment, Margin};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

pub(super) fn draw_shell_logs_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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
    let re = if app.logs.use_regex {
        "regex:on"
    } else {
        "regex:off"
    };
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

pub(super) fn draw_shell_inspect_meta(
    f: &mut ratatui::Frame,
    app: &App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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

pub(super) fn draw_shell_help_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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

pub(super) fn draw_shell_messages_meta(
    f: &mut ratatui::Frame,
    app: &App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
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
