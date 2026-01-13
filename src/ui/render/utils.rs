use anyhow::Context as _;
use crate::ui::App;
use ratatui::style::Style;
use std::fs;
use std::path::PathBuf;

pub(in crate::ui) fn shell_escape_sh_arg(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
    {
        return text.to_string();
    }
    let escaped = text.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

pub(in crate::ui) fn is_container_stopped(status: &str) -> bool {
    let s = status.trim();
    // docker ps STATUS values: "Up ...", "Exited (...) ...", "Created", "Dead"
    !(s.starts_with("Up") || s.starts_with("Restarting"))
}

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
    crate::ui::render::text::truncate_end(s, max)
}
