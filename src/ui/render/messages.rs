use crate::ui::render::scroll::draw_shell_scrollbar_v;
use crate::ui::render::text::window_hscroll;
use crate::ui::render::utils::{draw_focus_accent, shell_row_highlight};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::MsgLevel;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};
use time::OffsetDateTime;

fn patch_line_style(mut line: Line<'static>, style: Style) -> Line<'static> {
    for span in &mut line.spans {
        span.style = span.style.patch(style);
    }
    line
}

pub(in crate::ui) fn format_session_ts(at: OffsetDateTime) -> String {
    use std::sync::OnceLock;
    static FMT: OnceLock<Vec<time::format_description::FormatItem<'static>>> = OnceLock::new();
    let fmt = FMT.get_or_init(|| {
        time::format_description::parse("[hour]:[minute]:[second]").unwrap_or_else(|_| Vec::new())
    });
    at.format(fmt)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

pub(in crate::ui) fn draw_shell_messages_view(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    draw_shell_messages_list(f, app, area, bg);
}

pub(in crate::ui) fn draw_shell_messages_dock(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    draw_shell_messages_list(f, app, area, bg);
    draw_focus_accent(
        f,
        app,
        area,
        app.shell_focus == crate::ui::state::shell_types::ShellFocus::Dock,
    );
}

fn draw_shell_messages_list(f: &mut ratatui::Frame, app: &mut App, area: Rect, bg: Style) {
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let list_area = cols[0];
    let vbar_area = cols[1];

    let total_msgs = app.session_msgs.len();
    let total = total_msgs.max(1);
    let view_h = list_area.height.max(1) as usize;
    let max_scroll = total.saturating_sub(view_h);
    let w = list_area.width.max(1) as usize;
    let cursor = app.messages_cursor();
    if total_msgs > 0 {
        app.shell_msgs.scroll = cursor;
    }
    let mut top = if app.shell_msgs.scroll_top == usize::MAX {
        max_scroll
    } else {
        app.shell_msgs.scroll_top.min(max_scroll)
    };
    if cursor < top {
        top = cursor;
    } else if cursor >= top.saturating_add(view_h) {
        top = cursor
            .saturating_add(1)
            .saturating_sub(view_h)
            .min(max_scroll);
    }
    app.shell_msgs.scroll_top = top;

    let lnw = if app.logs.show_line_numbers {
        total_msgs.max(1).to_string().len()
    } else {
        0
    };
    let sel = app.messages_selection_range();

    // Clamp horizontal scroll to the selected message width.
    if let Some(m) = app.session_msgs.get(cursor) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let num_w = if app.logs.show_line_numbers {
            lnw + 1
        } else {
            0
        };
        let fixed_len = num_w + format!("{ts} {lvl} ").chars().count();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let max_h = m.text.chars().count().saturating_sub(msg_w);
        app.shell_msgs.hscroll = app.shell_msgs.hscroll.min(max_h);
    } else {
        app.shell_msgs.hscroll = 0;
    }

    let mut items: Vec<ListItem> = Vec::new();
    for (idx, m) in app.session_msgs.iter().enumerate().skip(top).take(view_h) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let lvl_style = match m.level {
            MsgLevel::Info => bg.patch(app.theme.text_dim.to_style()),
            MsgLevel::Warn => bg.patch(app.theme.text_warn.to_style()),
            MsgLevel::Error => bg.patch(app.theme.text_error.to_style()),
        };
        let ts = format_session_ts(m.at);
        let ts_style = bg.patch(app.theme.text_faint.to_style());
        let mut spans: Vec<Span<'static>> = Vec::new();
        if app.logs.show_line_numbers {
            let ln = format!("{:>lnw$} ", idx + 1);
            spans.push(Span::styled(ln, ts_style));
        }
        spans.push(Span::styled(ts, ts_style));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(lvl.to_string(), lvl_style));
        spans.push(Span::raw(" "));
        let fixed_len = spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum::<usize>();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let msg = window_hscroll(&m.text, app.shell_msgs.hscroll, msg_w);
        spans.push(Span::styled(msg, bg));
        let mut line = Line::from(spans);
        let selected = sel.map(|(a, b)| idx >= a && idx <= b).unwrap_or(false);
        if selected {
            line = patch_line_style(line, app.theme.marked.to_style());
        }
        items.push(ListItem::new(line));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }
    let mut highlight_style = shell_row_highlight(app);
    if app.shell_msgs.select_anchor.is_some() {
        let marked = app.theme.marked.to_style();
        highlight_style = highlight_style
            .fg(marked
                .fg
                .unwrap_or_else(|| highlight_style.fg.unwrap_or_default()))
            .add_modifier(marked.add_modifier);
    }
    let list = List::new(items)
        .style(bg)
        .highlight_style(highlight_style)
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(top)));
    f.render_stateful_widget(list, list_area, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        top,
        max_scroll,
        total,
        view_h,
        app.ascii_only,
        &app.theme,
    );
}
