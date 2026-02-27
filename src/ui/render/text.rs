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

// Horizontal window with ellipsis; used in messages/log views.
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

// Simple slice of a string by chars; used for logs/inspect horizontal cropping.
pub(in crate::ui) fn slice_window(s: &str, start: usize, width: usize) -> String {
    let width = width.max(1);
    let chars: Vec<char> = s.chars().collect();
    if width >= chars.len() {
        return s.to_string();
    }
    let start = start.min(chars.len().saturating_sub(width));
    chars.iter().skip(start).take(width).collect()
}
