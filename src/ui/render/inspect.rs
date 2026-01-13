use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::ui::{App, InspectLine};

pub(crate) fn build_inspect_lines(
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

pub(crate) fn collect_expandable_paths(root: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_expandable_paths_inner(root, "", &mut out);
    out
}

pub(crate) fn collect_match_paths(root: Option<&Value>, query: &str) -> Vec<String> {
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

pub(crate) fn ancestors_of_pointer(pointer: &str) -> Vec<String> {
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

pub(crate) fn collect_path_rank(root: Option<&Value>) -> HashMap<String, usize> {
    let Some(root) = root else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    let mut idx = 0usize;
    collect_path_rank_inner(root, "", &mut idx, &mut out);
    out
}

pub(crate) fn current_match_pos(app: &App) -> (usize, usize) {
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
