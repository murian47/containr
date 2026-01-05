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
