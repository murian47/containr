use anyhow::Context as _;
use crate::ui::{App, theme};
use image::Rgba;
use ratatui::style::{Color, Style};
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

pub(in crate::ui) fn color_to_rgba(color: Color, fallback: Rgba<u8>) -> Rgba<u8> {
    match color {
        Color::Rgb(r, g, b) => Rgba([r, g, b, 255]),
        Color::Black => Rgba([0, 0, 0, 255]),
        Color::Red => Rgba([255, 0, 0, 255]),
        Color::Green => Rgba([0, 255, 0, 255]),
        Color::Yellow => Rgba([255, 255, 0, 255]),
        Color::Blue => Rgba([0, 0, 255, 255]),
        Color::Magenta => Rgba([255, 0, 255, 255]),
        Color::Cyan => Rgba([0, 255, 255, 255]),
        Color::Gray => Rgba([128, 128, 128, 255]),
        Color::DarkGray => Rgba([64, 64, 64, 255]),
        Color::White => Rgba([255, 255, 255, 255]),
        _ => fallback,
    }
}

pub(in crate::ui) fn theme_color_rgba(spec: &str, fallback: Rgba<u8>) -> Rgba<u8> {
    let color = theme::parse_color(spec);
    color_to_rgba(color, fallback)
}
