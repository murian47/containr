use anyhow::Context as _;
use crate::ui::App;
use ratatui::style::Style;
use std::fs;
use std::path::PathBuf;

pub(in crate::ui) fn write_text_file(
    path: &str,
    text: &str,
    force: bool,
) -> anyhow::Result<PathBuf> {
    let path = path.trim();
    anyhow::ensure!(!path.is_empty(), "missing file path");

    let path = expand_user_path(path);
    if path.exists() && !force {
        anyhow::bail!("file exists (use save! to overwrite)");
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).context("failed to create parent dir")?;
        }
    }
    fs::write(&path, text).context("failed to write file")?;
    Ok(path)
}

pub(in crate::ui) fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

pub(in crate::ui) fn shell_row_highlight(app: &App) -> Style {
    app.theme.list_selected.to_style()
}

pub(in crate::ui) fn truncate_end(s: &str, max: usize) -> String {
    let max = max.max(1);
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s.chars().take(max).collect();
    }
    let mut out: String = s.chars().take(max - 3).collect();
    out.push_str("...");
    out
}

pub(in crate::ui) fn short_commit(s: &str) -> String {
    s.chars().take(7).collect()
}

pub(in crate::ui) fn window_hscroll(s: &str, start: usize, max: usize) -> String {
    let max = max.max(1);
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max <= 3 {
        let start = start.min(chars.len().saturating_sub(1));
        return chars.into_iter().skip(start).take(max).collect();
    }

    let mut start = start.min(chars.len().saturating_sub(1));
    let show_prefix = start > 0;
    let mut avail = max;
    if show_prefix {
        avail = avail.saturating_sub(3);
    }

    let remaining = chars.len().saturating_sub(start);
    let show_suffix = remaining > avail;
    if show_suffix {
        avail = avail.saturating_sub(3);
    }
    if avail == 0 {
        avail = 1;
    }

    if chars.len() > avail {
        start = start.min(chars.len().saturating_sub(avail));
    } else {
        start = 0;
    }

    let mid: String = chars.iter().copied().skip(start).take(avail).collect();
    let mut out = String::new();
    if show_prefix {
        out.push_str("...");
    }
    out.push_str(&mid);
    if show_suffix {
        out.push_str("...");
    }
    truncate_end(&out, max)
}

pub(in crate::ui) fn slice_window(s: &str, start: usize, width: usize) -> String {
    let width = width.max(1);
    let chars: Vec<char> = s.chars().collect();
    if width >= chars.len() {
        return s.to_string();
    }
    let start = start.min(chars.len().saturating_sub(width));
    chars.iter().skip(start).take(width).collect()
}

pub(in crate::ui) fn draw_shell_scrollbar_v(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_top: usize,
    max_scroll: usize,
    total_lines: usize,
    view_height: usize,
    ascii_only: bool,
    theme: &crate::ui::theme::ThemeSpec,
) {
    use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

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
    theme: &crate::ui::theme::ThemeSpec,
) {
    use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

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
