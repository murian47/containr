use std::time::Instant;

use ratatui::style::Style;
use ratatui::text::Span;

use crate::ui::render::utils::truncate_end;

pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let next = if line.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", line, word)
        };
        if next.chars().count() > width {
            if !line.is_empty() {
                out.push(truncate_end(&line, width));
            }
            line = word.to_string();
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        out.push(truncate_end(&line, width));
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

pub(crate) fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return truncate_end(text, width);
    }
    let mut out = text.to_string();
    out.push_str(&" ".repeat(width - len + 1));
    out
}

pub(crate) fn truncate_start(s: &str, max: usize) -> String {
    let max = max.max(1);
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s
            .chars()
            .rev()
            .take(max)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    let tail: String = s
        .chars()
        .rev()
        .take(max - 3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

pub(crate) fn format_bytes_short(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0usize;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes}B")
    } else if v >= 10.0 {
        format!("{:.0}{}", v, UNITS[u])
    } else {
        format!("{:.1}{}", v, UNITS[u])
    }
}

pub(crate) fn bar_spans_threshold(
    width: usize,
    ratio: f32,
    ascii_only: bool,
    filled_style: Style,
    empty_style: Style,
) -> Vec<Span<'static>> {
    let width = width.max(1);
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = ((width as f32) * ratio).round() as usize;
    let filled = filled.min(width);
    let (on, off) = if ascii_only { ('#', '.') } else { ('█', '░') };
    let mut out: Vec<Span<'static>> = Vec::new();
    if filled > 0 {
        let mut s = String::with_capacity(filled);
        s.extend(std::iter::repeat(on).take(filled));
        out.push(Span::styled(s, filled_style));
    }
    if width > filled {
        let mut s = String::with_capacity(width - filled);
        s.extend(std::iter::repeat(off).take(width - filled));
        out.push(Span::styled(s, empty_style));
    }
    out
}

pub(crate) fn bar_spans_gradient(
    width: usize,
    ratio: f32,
    ascii_only: bool,
    ok: Style,
    warn: Style,
    err: Style,
    empty_style: Style,
) -> Vec<Span<'static>> {
    let width = width.max(1);
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = ((width as f32) * ratio).round() as usize;
    let filled = filled.min(width);
    let (on, off) = if ascii_only { ('#', '.') } else { ('█', '░') };
    let mut out: Vec<Span<'static>> = Vec::new();

    let mut cur_style: Option<Style> = None;
    let mut cur_buf = String::new();
    for i in 0..filled {
        let pos_ratio = (i + 1) as f32 / (width as f32);
        let st = if pos_ratio >= 0.85 {
            err
        } else if pos_ratio >= 0.70 {
            warn
        } else {
            ok
        };
        if cur_style.map(|c| c == st).unwrap_or(false) {
            cur_buf.push(on);
        } else {
            if !cur_buf.is_empty() {
                out.push(Span::styled(cur_buf, cur_style.unwrap_or(ok)));
                cur_buf = String::new();
            }
            cur_style = Some(st);
            cur_buf.push(on);
        }
    }
    if !cur_buf.is_empty() {
        out.push(Span::styled(cur_buf, cur_style.unwrap_or(ok)));
    }
    if width > filled {
        let mut s = String::with_capacity(width - filled);
        s.extend(std::iter::repeat(off).take(width - filled));
        out.push(Span::styled(s, empty_style));
    }
    out
}

pub(crate) fn loading_spinner(since: Option<Instant>) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let Some(since) = since else {
        return FRAMES[0];
    };
    let idx = (since.elapsed().as_millis() / 120) as usize % FRAMES.len();
    FRAMES[idx]
}

pub(crate) fn spinner_char(started: Instant, ascii_only: bool) -> char {
    let ms = started.elapsed().as_millis() as u64;
    if ascii_only {
        let frames = ['|', '/', '-', '\\'];
        frames[((ms / 150) % frames.len() as u64) as usize]
    } else {
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        frames[((ms / 120) % frames.len() as u64) as usize]
    }
}
