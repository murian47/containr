use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::ui::theme::ThemeSpec;

pub(in crate::ui) fn draw_shell_scrollbar_v(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_top: usize,
    max_scroll: usize,
    total_lines: usize,
    view_height: usize,
    ascii_only: bool,
    theme: &ThemeSpec,
) {
    if area.height == 0 || total_lines == 0 {
        return;
    }
    let mapped_pos = if max_scroll == 0 || total_lines <= 1 {
        0
    } else {
        (scroll_top.min(max_scroll) * (total_lines - 1)) / max_scroll
    };
    let track = if ascii_only { "|" } else { "│" };
    let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(track))
        .thumb_symbol(track)
        .track_style(theme.scroll_track.to_style())
        .thumb_style(theme.scroll_thumb.to_style());
    let mut sb_state = ScrollbarState::new(total_lines)
        .position(mapped_pos)
        .viewport_content_length(view_height.max(1));
    f.render_stateful_widget(sb, area, &mut sb_state);
}

pub(in crate::ui) fn draw_shell_scrollbar_h(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_left: usize,
    max_scroll: usize,
    content_width: usize,
    view_width: usize,
    ascii_only: bool,
    theme: &ThemeSpec,
) {
    if area.height == 0 || content_width == 0 {
        return;
    }
    let mapped_pos = if max_scroll == 0 || content_width <= 1 {
        0
    } else {
        (scroll_left.min(max_scroll) * (content_width - 1)) / max_scroll
    };
    let track = if ascii_only { "-" } else { "─" };
    let sb = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(track))
        .thumb_symbol(track)
        .track_style(theme.scroll_track.to_style())
        .thumb_style(theme.scroll_thumb.to_style());
    let mut sb_state = ScrollbarState::new(content_width)
        .position(mapped_pos)
        .viewport_content_length(view_width.max(1));
    f.render_stateful_widget(sb, area, &mut sb_state);
}
