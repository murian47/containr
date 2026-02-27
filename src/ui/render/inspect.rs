use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::ui::{App, InspectLine, InspectMode};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::highlight::highlight_log_line_literal;
use super::scroll::draw_shell_scrollbar_v;
use super::text::slice_window;
use super::utils::shell_row_highlight;

pub(in crate::ui) fn build_inspect_lines(
    root: Option<&Value>,
    expanded: &HashSet<String>,
    match_set: &HashSet<String>,
    query: &str,
) -> Vec<InspectLine> {
    let Some(root) = root else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    let mut buf = String::new();
    build_inspect_lines_inner(
        root,
        expanded,
        match_set,
        "",
        0,
        "$".to_string(),
        &q,
        &mut out,
        &mut buf,
    );
    out
}

pub(in crate::ui) fn collect_expandable_paths(root: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_expandable_paths_inner(root, "", &mut out);
    out
}

pub(in crate::ui) fn collect_match_paths(root: Option<&Value>, query: &str) -> Vec<String> {
    let Some(root) = root else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut scratch = String::new();
    collect_match_paths_inner(root, "", "$", &q, &mut out, &mut scratch);
    out
}

pub(in crate::ui) fn ancestors_of_pointer(pointer: &str) -> Vec<String> {
    if pointer.is_empty() {
        return vec!["".to_string()];
    }
    let mut out = vec!["".to_string()];
    let mut current = String::new();
    for token in pointer.split('/').skip(1) {
        current.push('/');
        current.push_str(token);
        out.push(current.clone());
    }
    out
}

pub(in crate::ui) fn collect_path_rank(root: Option<&Value>) -> HashMap<String, usize> {
    let Some(root) = root else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    let mut idx = 0usize;
    collect_path_rank_inner(root, "", &mut idx, &mut out);
    out
}

pub(in crate::ui) fn current_match_pos(app: &App) -> (usize, usize) {
    let total = app.inspect.match_paths.len();
    if total == 0 {
        return (0, 0);
    }
    let path = app
        .inspect.lines
        .get(app.inspect.selected)
        .map(|l| l.path.as_str())
        .unwrap_or("");
    let idx = app
        .inspect.match_paths
        .iter()
        .position(|p| p == path)
        .map(|i| i + 1)
        .unwrap_or(0);
    (idx, total)
}

pub(in crate::ui) fn draw_shell_inspect_view(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    // Reuse inspect tree lines computed in app.inspect.lines.
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let content = cols[0];
    let vbar_area = cols[1];

    let view_height = content.height.max(1) as usize;
    let total_lines = app.inspect.lines.len();
    let max_scroll = total_lines.saturating_sub(view_height);
    let cursor = app.inspect.selected.min(total_lines.saturating_sub(1));
    let mut scroll_top = app.inspect.scroll_top.min(max_scroll);
    if cursor < scroll_top {
        scroll_top = cursor;
    } else if cursor >= scroll_top.saturating_add(view_height) {
        scroll_top = cursor
            .saturating_add(1)
            .saturating_sub(view_height)
            .min(max_scroll);
    }
    app.inspect.scroll_top = scroll_top;

    let start = scroll_top;
    let end = (start + view_height).min(total_lines);
    let avail_w = content.width.max(1) as usize;

    // Clamp horizontal scroll so it does not "virtually" exceed the content width.
    let mut max_len: usize = 0;
    for l in &app.inspect.lines {
        let label_len = l.label.chars().count();
        let summary_len = l.summary.chars().count();
        let line_len = l.depth.saturating_mul(2)
            + 2
            + label_len
            + if summary_len > 0 { 2 + summary_len } else { 0 };
        max_len = max_len.max(line_len);
    }
    let max_hscroll = max_len.saturating_sub(avail_w);
    app.inspect.scroll = app.inspect.scroll.min(max_hscroll);

    let q = app.inspect.query.trim();
    let mut items: Vec<ListItem> = Vec::with_capacity(end.saturating_sub(start));
    for l in app.inspect.lines.iter().take(end).skip(start) {
        let indent = "  ".repeat(l.depth);
        let glyph = if l.expandable {
            if l.expanded { "▾ " } else { "▸ " }
        } else {
            "  "
        };
        let mut text = format!("{indent}{glyph}{}", l.label);
        if !l.summary.is_empty() {
            text.push_str(": ");
            text.push_str(&l.summary);
        }
        let visible = slice_window(&text, app.inspect.scroll, avail_w);
        let line = if app.inspect.mode == InspectMode::Search && !q.is_empty() {
            highlight_log_line_literal(&visible, q)
        } else if l.matches {
            highlight_log_line_literal(&visible, q)
        } else {
            ratatui::text::Line::from(visible)
        };
        items.push(ListItem::new(line));
    }
    if items.is_empty() {
        items.push(ListItem::new(ratatui::text::Line::from("")));
    }

    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(start)));
    f.render_stateful_widget(list, content, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        scroll_top,
        max_scroll,
        total_lines,
        view_height,
        app.ascii_only,
        &app.theme,
    );
}

fn build_inspect_lines_inner(
    value: &Value,
    expanded: &HashSet<String>,
    match_set: &HashSet<String>,
    path: &str,
    depth: usize,
    label: String,
    query: &str,
    out: &mut Vec<InspectLine>,
    scratch: &mut String,
) {
    let expanded_here = expanded.contains(path);
    let (summary, expandable) = summarize(value);

    scratch.clear();
    let _ = write!(scratch, "{} {} {}", path, label, summary);
    let hay = scratch.to_lowercase();
    let matches = !query.is_empty() && (match_set.contains(path) || hay.contains(query));

    out.push(InspectLine {
        path: path.to_string(),
        depth,
        label,
        summary,
        expandable,
        expanded: expanded_here,
        matches,
    });

    if !(expandable && expanded_here) {
        return;
    }

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child = &map[key];
                let child_path = join_pointer(path, key);
                build_inspect_lines_inner(
                    child,
                    expanded,
                    match_set,
                    &child_path,
                    depth + 1,
                    key.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &idx.to_string());
                build_inspect_lines_inner(
                    child,
                    expanded,
                    match_set,
                    &child_path,
                    depth + 1,
                    idx.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        _ => {}
    }
}

fn summarize(value: &Value) -> (String, bool) {
    match value {
        Value::Null => ("null".to_string(), false),
        Value::Bool(b) => (b.to_string(), false),
        Value::Number(n) => (n.to_string(), false),
        Value::String(s) => (format!("{:?}", s), false),
        Value::Array(arr) => (format!("[{}]", arr.len()), true),
        Value::Object(map) => (format!("{{{}}}", map.len()), true),
    }
}

fn collect_expandable_paths_inner(value: &Value, path: &str, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            out.insert(path.to_string());
            for (k, v) in map {
                let p = join_pointer(path, k);
                collect_expandable_paths_inner(v, &p, out);
            }
        }
        Value::Array(arr) => {
            out.insert(path.to_string());
            for (idx, v) in arr.iter().enumerate() {
                let p = join_pointer(path, &idx.to_string());
                collect_expandable_paths_inner(v, &p, out);
            }
        }
        _ => {}
    }
}

fn join_pointer(parent: &str, token: &str) -> String {
    if parent.is_empty() {
        format!("/{}", escape_pointer_token(token))
    } else {
        format!("{}/{}", parent, escape_pointer_token(token))
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn collect_match_paths_inner(
    value: &Value,
    path: &str,
    label: &str,
    query: &str,
    out: &mut Vec<String>,
    scratch: &mut String,
) {
    let (summary, _expandable) = summarize(value);
    scratch.clear();
    let _ = write!(scratch, "{} {} {}", path, label, summary);
    if scratch.to_lowercase().contains(query) {
        out.push(path.to_string());
    }

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = join_pointer(path, key);
                collect_match_paths_inner(&map[key], &child_path, key, query, out, scratch);
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &idx.to_string());
                collect_match_paths_inner(
                    child,
                    &child_path,
                    &idx.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        _ => {}
    }
}

fn collect_path_rank_inner(
    value: &Value,
    path: &str,
    idx: &mut usize,
    out: &mut HashMap<String, usize>,
) {
    out.insert(path.to_string(), *idx);
    *idx += 1;

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = join_pointer(path, key);
                collect_path_rank_inner(&map[key], &child_path, idx, out);
            }
        }
        Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &i.to_string());
                collect_path_rank_inner(child, &child_path, idx, out);
            }
        }
        _ => {}
    }
}
