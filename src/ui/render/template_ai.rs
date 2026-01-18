use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::ui::{App, TemplateAiFocus};
use crate::ui::render::layout::{draw_shell_hr, draw_shell_vr};
use crate::ui::render::text::truncate_end;
use crate::ui::theme;

pub(crate) fn draw_template_ai_view(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    draw_template_ai_title(f, app, parts[0]);

    let body = parts[1];
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Percentage(55),
        ])
        .split(body);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Length(1), Constraint::Percentage(50)])
        .split(rows[0]);

    draw_template_ai_prompt(f, app, cols[0]);
    draw_shell_vr(f, app, cols[1]);
    draw_template_ai_template(f, app, cols[2]);
    draw_shell_hr(f, app, rows[1]);
    draw_template_ai_result(f, app, rows[2]);
}

fn draw_template_ai_title(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let bg = if app.shell_focus == crate::ui::ShellFocus::List {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    let title = if app.template_ai.target_name.trim().is_empty() {
        " Template AI".to_string()
    } else {
        format!(" Template AI: {}", app.template_ai.target_name.trim())
    };
    let shown = truncate_end(&title, area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(shown)
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_template_ai_prompt(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let focused = app.template_ai.focus == TemplateAiFocus::Prompt;
    let (header, content) = split_header(area);
    let st = panel_style(app, focused);
    let title_style = pane_title_style(app, focused);

    f.render_widget(Block::default().style(st), area);
    f.render_widget(
        Paragraph::new(" Prompt")
            .style(title_style)
            .wrap(Wrap { trim: false }),
        header,
    );

    let cursor_style = app.theme.cmdline_cursor.to_style();
    let lines = prompt_lines(&app.template_ai.prompt, app.template_ai.prompt_cursor, st, cursor_style);
    let text = Text::from(lines);
    f.render_widget(
        Paragraph::new(text)
            .style(st)
            .scroll((app.template_ai.prompt_scroll.min(u16::MAX as usize) as u16, 0)),
        content,
    );
}

fn draw_template_ai_template(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let focused = app.template_ai.focus == TemplateAiFocus::Template;
    let (header, content) = split_header(area);
    let st = panel_style(app, focused);
    let title_style = pane_title_style(app, focused);
    let label = match app.templates_state.kind {
        crate::ui::TemplatesKind::Stacks => " Template (compose.yaml)",
        crate::ui::TemplatesKind::Networks => " Template (network.json)",
    };

    f.render_widget(Block::default().style(st), area);
    f.render_widget(
        Paragraph::new(label)
            .style(title_style)
            .wrap(Wrap { trim: false }),
        header,
    );
    let text = if app.template_ai.template_text.trim().is_empty() {
        Text::from(Line::from(Span::styled(
            "No template content loaded.",
            st.patch(app.theme.text_dim.to_style()),
        )))
    } else {
        Text::from(app.template_ai.template_text.clone())
    };
    f.render_widget(
        Paragraph::new(text)
            .style(st)
            .wrap(Wrap { trim: false })
            .scroll((app.template_ai.template_scroll.min(u16::MAX as usize) as u16, 0)),
        content,
    );
}

fn draw_template_ai_result(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let focused = app.template_ai.focus == TemplateAiFocus::Result;
    let (header, content) = split_header(area);
    let st = panel_style(app, focused);
    let title_style = pane_title_style(app, focused);

    f.render_widget(Block::default().style(st), area);
    f.render_widget(
        Paragraph::new(" Result")
            .style(title_style)
            .wrap(Wrap { trim: false }),
        header,
    );
    let text = if app.template_ai.result_text.trim().is_empty() {
        Text::from(Line::from(Span::styled(
            "Run the prompt to see results.",
            st.patch(app.theme.text_dim.to_style()),
        )))
    } else {
        Text::from(app.template_ai.result_text.clone())
    };
    f.render_widget(
        Paragraph::new(text)
            .style(st)
            .wrap(Wrap { trim: false })
            .scroll((app.template_ai.result_scroll.min(u16::MAX as usize) as u16, 0)),
        content,
    );
}

fn split_header(area: Rect) -> (Rect, Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    (parts[0], parts[1])
}

fn panel_style(app: &App, focused: bool) -> Style {
    if focused {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    }
}

fn pane_title_style(app: &App, focused: bool) -> Style {
    let base_bg = if focused {
        theme::parse_color(&app.theme.panel_focused.bg)
    } else {
        theme::parse_color(&app.theme.panel.bg)
    };
    app.theme.table_header.to_style().bg(base_bg)
}

fn prompt_lines(input: &str, cursor: usize, st: Style, cursor_style: Style) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    let input_lines: Vec<&str> = input.split('\n').collect();
    let (line_idx, col) = cursor_line_col(input, cursor);
    let cursor_line = line_idx.min(input_lines.len().saturating_sub(1));

    for (idx, line) in input_lines.iter().enumerate() {
        if idx == cursor_line {
            let (before, ch, after) = split_line_at_cursor(line, col);
            lines.push(Line::from(vec![
                Span::styled(before, st),
                Span::styled(ch, cursor_style),
                Span::styled(after, st),
            ]));
        } else {
            lines.push(Line::from(Span::styled((*line).to_string(), st)));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" ".to_string(), cursor_style)));
    }
    lines
}

fn cursor_line_col(input: &str, cursor: usize) -> (usize, usize) {
    let mut line = 0usize;
    let mut col = 0usize;
    let mut idx = 0usize;
    for ch in input.chars() {
        if idx == cursor {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
        idx += 1;
    }
    (line, col)
}

fn split_line_at_cursor(line: &str, col: usize) -> (String, String, String) {
    let mut chars = line.chars();
    let before: String = chars.by_ref().take(col).collect();
    let current = chars.next().unwrap_or(' ');
    let after: String = chars.collect();
    (before, current.to_string(), after)
}
