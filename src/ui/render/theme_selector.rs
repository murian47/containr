use crate::ui::{App, ShellFocus, ShellView, shell_module_shortcut};
use crate::ui::render::scroll::draw_shell_scrollbar_v;
use crate::ui::render::text::truncate_end;
use crate::ui::render::utils::shell_row_highlight;
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Row, Table, Wrap};

pub(in crate::ui) fn draw_theme_selector(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if app.shell_sidebar_collapsed { 18 } else { 28 }),
            Constraint::Min(1),
        ])
        .split(area);

    draw_theme_selector_sidebar(f, app, cols[0]);
    draw_theme_selector_preview(f, app, cols[1]);
}

fn draw_theme_selector_sidebar(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = if app.shell_focus == ShellFocus::Sidebar {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    if inner.height == 0 {
        return;
    }
    let list_area = inner;
    let visible = list_area.height.max(1) as usize;

    if app.theme_selector.page_size != visible {
        app.theme_selector.page_size = visible;
        if app.theme_selector.center_on_open {
            app.theme_selector_adjust_scroll(true);
            app.theme_selector.center_on_open = false;
        } else {
            app.theme_selector_adjust_scroll(false);
        }
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(list_area);
    let list_area = cols[0];
    let vbar_area = cols[1];

    let list_w = list_area.width.max(1) as usize;
    let mut items: Vec<ListItem> = Vec::new();
    for (idx, name) in app.theme_selector.names.iter().enumerate() {
        let selected = idx == app.theme_selector.selected;
        let st = if selected {
            shell_row_highlight(app)
        } else {
            bg
        };
        let label = format!(" {name}");
        let label = truncate_end(&label, list_w);
        items.push(ListItem::new(Line::from(Span::styled(label, st))));
    }

    let mut state = ListState::default();
    if !app.theme_selector.names.is_empty() {
        let selected = app.theme_selector.selected;
        let max_scroll = app
            .theme_selector
            .names
            .len()
            .saturating_sub(visible);
        let scroll = app.theme_selector.scroll.min(max_scroll);
        *state.offset_mut() = scroll;
        state.select(Some(selected));
        app.theme_selector.scroll = scroll;
    }
    f.render_stateful_widget(List::new(items), list_area, &mut state);

    let total = app.theme_selector.names.len();
    let max_scroll = total.saturating_sub(visible);
    draw_shell_scrollbar_v(
        f,
        vbar_area,
        app.theme_selector.scroll,
        max_scroll,
        total,
        visible,
        app.ascii_only,
        &app.theme,
    );
}

fn draw_theme_selector_preview(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let preview = &app.theme_selector.preview_theme;
    let outer = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 3,
    });
    let frame_style = app.theme.divider.to_style();
    let frame = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .style(frame_style);
    f.render_widget(frame, outer);

    let inner = outer.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let bg = preview.panel.to_style();
    f.render_widget(Block::default().style(bg), inner);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
            Constraint::Length(1), // cmdline
        ])
        .split(inner);

    draw_preview_header(f, app, preview, rows[0]);
    draw_preview_body(f, preview, &app.theme_selector.error, rows[1]);
    draw_preview_footer(f, preview, rows[2]);
    draw_preview_cmdline(f, app, preview, rows[3]);
}

fn draw_preview_header(
    f: &mut ratatui::Frame,
    app: &App,
    theme: &theme::ThemeSpec,
    area: Rect,
) {
    let st = theme.header.to_style();
    let mut spans = preview_logo_spans(app, theme, "CONTAINR");
    let server_name = "demo2";
    spans.push(Span::styled("  Server: ".to_string(), st));
    spans.push(Span::styled(server_name.to_string(), st));
    spans.push(Span::styled("  ".to_string(), st));
    spans.push(Span::styled("●".to_string(), theme.text_ok.to_style()));
    spans.push(Span::styled(" connected  View: Containers".to_string(), st));
    f.render_widget(Paragraph::new(Line::from(spans)).style(st), area);
}

fn draw_preview_footer(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.footer.to_style();
    let text = Span::styled("F1 help   b sidebar   ^p layout   :q quit", st);
    f.render_widget(Paragraph::new(Line::from(text)).style(st), area);
}

fn draw_preview_cmdline(f: &mut ratatui::Frame, app: &App, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.cmdline.to_style();
    let label = Span::styled("CONTAINR", theme.cmdline_label.to_style());
    let theme_name = app
        .theme_selector
        .names
        .get(app.theme_selector.selected)
        .map(|s| s.as_str())
        .unwrap_or("default");
    let shown = if theme_name.contains(' ') {
        format!("\"{theme_name}\"")
    } else {
        theme_name.to_string()
    };
    let prompt = Span::styled(format!(" :theme use {shown}"), theme.cmdline_inactive.to_style());
    f.render_widget(Paragraph::new(Line::from(vec![label, prompt])).style(st), area);
}

fn draw_preview_body(
    f: &mut ratatui::Frame,
    theme: &theme::ThemeSpec,
    error: &Option<String>,
    area: Rect,
) {
    let mut body = area;
    if let Some(msg) = error {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        let st = theme.text_error.to_style();
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg.clone(), st))).style(st),
            rows[0],
        );
        body = rows[1];
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Length(2), Constraint::Min(1)])
        .split(body);

    draw_preview_sidebar(f, theme, cols[0]);
    f.render_widget(Block::default().style(theme.panel.to_style()), cols[1]);
    draw_preview_main(f, theme, cols[2]);
}

fn draw_preview_sidebar(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.panel.to_style();
    f.render_widget(Block::default().style(st), area);
    let w = area.width.max(1) as usize;
    let active = theme.active.to_style();
    let dim = theme.text_dim.to_style();
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(" ".repeat(w), st)));
    lines.push(preview_sidebar_hint_line("demo1", '1', st, w));
    lines.push(preview_sidebar_hint_line("demo2", '2', active, w));
    lines.push(Line::from(Span::styled("─".repeat(w), theme.divider.to_style())));
    lines.push(preview_sidebar_module_line("Dashboard", shell_module_shortcut(ShellView::Dashboard), st, w));
    lines.push(preview_sidebar_module_line("Stacks", shell_module_shortcut(ShellView::Stacks), st, w));
    lines.push(preview_sidebar_module_line("Containers", shell_module_shortcut(ShellView::Containers), active, w));
    lines.push(preview_sidebar_module_line("Images", shell_module_shortcut(ShellView::Images), st, w));
    lines.push(preview_sidebar_module_line("Volumes", shell_module_shortcut(ShellView::Volumes), st, w));
    lines.push(preview_sidebar_module_line("Networks", shell_module_shortcut(ShellView::Networks), st, w));
    lines.push(Line::from(Span::styled(" ".repeat(w), st)));
    lines.push(preview_sidebar_module_line("Templates", shell_module_shortcut(ShellView::Templates), dim, w));
    lines.push(preview_sidebar_module_line("Registries", shell_module_shortcut(ShellView::Registries), dim, w));
    f.render_widget(Paragraph::new(lines).style(st), area);
}

fn preview_sidebar_hint_line(text: &str, hint: char, st: Style, width: usize) -> Line<'static> {
    let base = format!(" {text}");
    let hint = format!("[{hint}]");
    let hint_len = hint.chars().count();
    let left_max = width.saturating_sub(hint_len.saturating_add(1)).max(1);
    let base_shown = truncate_end(&base, left_max);
    let base_len = base_shown.chars().count();
    let gap = width.saturating_sub(base_len.saturating_add(hint_len));
    Line::from(vec![
        Span::styled(base_shown, st),
        Span::styled(" ".repeat(gap), st),
        Span::styled(hint, st),
    ])
}

fn preview_sidebar_module_line(text: &str, hint: char, st: Style, width: usize) -> Line<'static> {
    preview_sidebar_hint_line(text, hint, st, width)
}

fn draw_preview_main(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.panel.to_style();
    f.render_widget(Block::default().style(st), area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Percentage(60),
            Constraint::Length(1),
            Constraint::Percentage(40),
        ])
        .split(area);

    draw_preview_title(f, theme, rows[0]);
    draw_preview_table(f, theme, rows[1]);
    draw_preview_divider(f, theme, rows[2]);
    draw_preview_details(f, theme, rows[3]);
}

fn draw_preview_table(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let header_style = theme.table_header.to_style();
    let row_style = theme.panel.to_style();
    let selected_style = theme.list_selected.to_style();

    let header = Row::new(vec!["NAME", "STATUS", "IMAGE"]).style(header_style);
    let rows = vec![
        Row::new(vec!["api", "running", "ghcr.io/demo/api:latest"]).style(selected_style),
        Row::new(vec!["worker", "exited", "ghcr.io/demo/worker:latest"]).style(row_style),
        Row::new(vec!["db", "running", "postgres:16"]).style(row_style),
    ];

    let table = Table::new(rows, [Constraint::Percentage(34), Constraint::Percentage(18), Constraint::Percentage(48)])
        .header(header)
        .style(row_style);
    f.render_widget(table, area);
}

fn draw_preview_details(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.text.to_style();
    let dim = theme.text_dim.to_style();
    let lines = vec![
        Line::from(vec![Span::styled("Name:", dim), Span::styled(" api", st)]),
        Line::from(vec![Span::styled("Image:", dim), Span::styled(" ghcr.io/demo/api:latest", st)]),
        Line::from(vec![Span::styled("Ports:", dim), Span::styled(" 8080->80/tcp", st)]),
        Line::from(vec![Span::styled("Updated:", dim), Span::styled(" 12s ago", st)]),
    ];
    f.render_widget(Paragraph::new(lines).style(st), area);
}

fn draw_preview_divider(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.divider.to_style();
    let line = "─".repeat(area.width.max(1) as usize);
    f.render_widget(Paragraph::new(line).style(st).wrap(Wrap { trim: false }), area);
}

fn draw_preview_title(f: &mut ratatui::Frame, theme: &theme::ThemeSpec, area: Rect) {
    let st = theme.panel_focused.to_style();
    let text = " Containers (3)";
    f.render_widget(Paragraph::new(text).style(st).wrap(Wrap { trim: false }), area);
}

fn preview_logo_spans(app: &App, theme: &theme::ThemeSpec, shown: &str) -> Vec<Span<'static>> {
    let base = theme.header.to_style();
    let bg = theme::parse_color(&theme.header.bg);
    let bg_rgb = match bg {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    };
    let is_dark = bg_rgb.map(|(r, g, b)| rel_luma(r, g, b) < 0.55).unwrap_or(true);

    let bright_palette: [Color; 8] = [
        Color::Rgb(255, 95, 86),
        Color::Rgb(255, 189, 46),
        Color::Rgb(39, 201, 63),
        Color::Rgb(64, 156, 255),
        Color::Rgb(175, 82, 222),
        Color::Rgb(255, 105, 180),
        Color::Rgb(0, 212, 212),
        Color::Rgb(255, 255, 255),
    ];
    let dark_palette: [Color; 8] = [
        Color::Rgb(120, 20, 20),
        Color::Rgb(120, 80, 0),
        Color::Rgb(0, 90, 40),
        Color::Rgb(0, 60, 120),
        Color::Rgb(70, 30, 110),
        Color::Rgb(120, 30, 70),
        Color::Rgb(0, 90, 90),
        Color::Rgb(0, 0, 0),
    ];
    let palette: &[Color] = if is_dark { &bright_palette } else { &dark_palette };

    let seed = app.header_logo_seed as usize;
    let offset = seed % palette.len();
    let mut step = (((seed >> 8) as usize) % (palette.len().saturating_sub(1)).max(1)).max(1);
    if gcd(step, palette.len()) != 1 {
        step = 1;
    }

    let mut out: Vec<Span<'static>> = Vec::new();
    let mut letter_i = 0usize;
    for ch in shown.chars() {
        if ch.is_ascii_alphabetic() {
            let mut c = palette[(offset + letter_i.saturating_mul(step)) % palette.len()];
            if let Some((br, bg, bb)) = bg_rgb {
                let ratio = contrast_ratio((br, bg, bb), c);
                if ratio < 3.0 {
                    c = if is_dark { Color::White } else { Color::Black };
                }
            }
            out.push(Span::styled(ch.to_string(), base.fg(c).add_modifier(Modifier::BOLD)));
            letter_i = letter_i.saturating_add(1);
        } else {
            out.push(Span::styled(ch.to_string(), base));
        }
    }
    out
}

fn rel_luma(r: u8, g: u8, b: u8) -> f32 {
    let r = (r as f32) / 255.0;
    let g = (g as f32) / 255.0;
    let b = (b as f32) / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast_ratio(bg: (u8, u8, u8), fg: Color) -> f32 {
    let (fr, fg, fb) = match fg {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::White => (255, 255, 255),
        Color::Black => (0, 0, 0),
        _ => (255, 255, 255),
    };
    let l1 = rel_luma(bg.0, bg.1, bg.2) + 0.05;
    let l2 = rel_luma(fr, fg, fb) + 0.05;
    if l1 > l2 { l1 / l2 } else { l2 / l1 }
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}
