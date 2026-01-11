use ratatui::style::Style;
use ratatui::text::Span;

use crate::ui::theme::ThemeSpec;

pub(crate) fn yaml_highlight_line(line: &str, base: Style, theme: &ThemeSpec) -> Vec<Span<'static>> {
    // Very small YAML-ish highlighter:
    // - comments: dim
    // - mapping keys: light blue
    let normal = base.patch(theme.syntax_text.to_style());
    let comment = base.patch(theme.syntax_comment.to_style());
    let key_style = base.patch(theme.syntax_key.to_style());

    let (code, comment_part) = split_yaml_comment(line);
    let mut spans: Vec<Span<'static>> = Vec::new();

    if code.trim().is_empty() {
        if !code.is_empty() {
            spans.push(Span::styled(code.to_string(), normal));
        }
    } else if let Some((prefix, key, rest)) = split_yaml_key(code) {
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix.to_string(), normal));
        }
        spans.push(Span::styled(key.to_string(), key_style));
        if !rest.is_empty() {
            spans.push(Span::styled(rest.to_string(), normal));
        }
    } else {
        spans.push(Span::styled(code.to_string(), normal));
    }

    if let Some(c) = comment_part {
        spans.push(Span::styled(c.to_string(), comment));
    }
    spans
}

pub(crate) fn json_highlight_line(line: &str, base: Style, theme: &ThemeSpec) -> Vec<Span<'static>> {
    // Minimal JSON-ish highlighter:
    // - keys ("...":) in light blue
    let normal = base.patch(theme.syntax_text.to_style());
    let key_style = base.patch(theme.syntax_key.to_style());

    let mut spans: Vec<Span<'static>> = Vec::new();
    let Some(start) = line.find('\"') else {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    };
    let rest = &line[start + 1..];
    let Some(end_rel) = rest.find('\"') else {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    };
    let end = start + 1 + end_rel;
    let after = &line[end + 1..];
    // Only treat it as a key if a ':' follows (allow whitespace).
    let after_trim = after.trim_start();
    if !after_trim.starts_with(':') {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    }

    let prefix = &line[..start];
    let key = &line[start..=end];
    let rest = &line[end + 1..];
    if !prefix.is_empty() {
        spans.push(Span::styled(prefix.to_string(), normal));
    }
    spans.push(Span::styled(key.to_string(), key_style));
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), normal));
    }
    spans
}

pub(crate) fn split_yaml_comment(line: &str) -> (&str, Option<&str>) {
    // Find a '#' that is not inside single/double quotes.
    let mut in_s = false;
    let mut in_d = false;
    let mut prev_bs = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => {
                in_s = !in_s;
                prev_bs = false;
            }
            '\"' if !in_s && !prev_bs => {
                in_d = !in_d;
                prev_bs = false;
            }
            '\\' if in_d => {
                prev_bs = !prev_bs;
            }
            '#' if !in_s && !in_d => {
                return (&line[..i], Some(&line[i..]));
            }
            _ => prev_bs = false,
        }
    }
    (line, None)
}

pub(crate) fn split_yaml_key(line: &str) -> Option<(&str, &str, &str)> {
    // Attempts to split "<prefix><key>:<rest>" where key is outside quotes.
    let mut in_s = false;
    let mut in_d = false;
    let mut prev_bs = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => {
                in_s = !in_s;
                prev_bs = false;
            }
            '\"' if !in_s && !prev_bs => {
                in_d = !in_d;
                prev_bs = false;
            }
            '\\' if in_d => {
                prev_bs = !prev_bs;
            }
            ':' if !in_s && !in_d => {
                let (left, _right) = line.split_at(i);
                // Walk back to find key token (support "- key:" too).
                let bytes = left.as_bytes();
                let mut j = bytes.len();
                while j > 0 && bytes[j - 1].is_ascii_whitespace() {
                    j -= 1;
                }
                let key_end = j;
                while j > 0 {
                    let b = bytes[j - 1];
                    if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                        j -= 1;
                    } else {
                        break;
                    }
                }
                let key_start = j;
                if key_start == key_end {
                    return None;
                }
                let prefix = &left[..key_start];
                let key = &left[key_start..key_end];
                let rest = &line[key_end..];
                return Some((prefix, key, rest));
            }
            _ => prev_bs = false,
        }
    }
    None
}
