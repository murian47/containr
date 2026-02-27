use std::time::Instant;
use std::sync::OnceLock;

use ratatui::style::Style;
use ratatui::text::Span;
use time::OffsetDateTime;

use crate::ui::render::utils::truncate_end;

pub(in crate::ui) fn wrap_text(text: &str, width: usize) -> Vec<String> {
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

pub(in crate::ui) fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return truncate_end(text, width);
    }
    let mut out = text.to_string();
    out.push_str(&" ".repeat(width - len + 1));
    out
}

pub(in crate::ui) fn truncate_start(s: &str, max: usize) -> String {
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

pub(in crate::ui) fn format_bytes_short(bytes: u64) -> String {
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

pub(in crate::ui) fn format_action_ts(at: OffsetDateTime) -> String {
    static FMT: OnceLock<Vec<time::format_description::FormatItem<'static>>> = OnceLock::new();
    let fmt = FMT.get_or_init(|| {
        time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
            .unwrap_or_else(|_| Vec::new())
    });
    at.format(fmt)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

pub(in crate::ui) fn split_at_chars(s: &str, n: usize) -> (&str, &str) {
    if n == 0 {
        return ("", s);
    }
    let mut idx = 0usize;
    let mut chars = 0usize;
    for (i, _) in s.char_indices() {
        if chars == n {
            idx = i;
            break;
        }
        chars += 1;
        idx = s.len();
    }
    if chars < n {
        (s, "")
    } else {
        s.split_at(idx)
    }
}

pub(in crate::ui) fn bar_spans_threshold(
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
    let (on, off) = if ascii_only { ('#', '.') } else { ('▇', '▇') };
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

pub(in crate::ui) fn bar_spans_gradient(
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
    let (on, off) = if ascii_only { ('#', '.') } else { ('▇', '▇') };
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

pub(in crate::ui) fn loading_spinner(since: Option<Instant>) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let Some(since) = since else {
        return FRAMES[0];
    };
    let idx = (since.elapsed().as_millis() / 120) as usize % FRAMES.len();
    FRAMES[idx]
}

pub(in crate::ui) fn spinner_char(started: Instant, ascii_only: bool) -> char {
    let ms = started.elapsed().as_millis() as u64;
    if ascii_only {
        let frames = ['|', '/', '-', '\\'];
        frames[((ms / 150) % frames.len() as u64) as usize]
    } else {
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        frames[((ms / 120) % frames.len() as u64) as usize]
    }
}

pub(in crate::ui) fn dot_spinner(ascii_only: bool) -> &'static str {
    const FRAMES_ASCII: [&str; 3] = ["·..", ".·.", "..·"];
    const FRAMES_UNI: [&str; 3] = ["●··", "·●·", "··●"];
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let idx = ((ms / 400) % FRAMES_ASCII.len() as u64) as usize;
    if ascii_only {
        FRAMES_ASCII[idx]
    } else {
        FRAMES_UNI[idx]
    }
}
