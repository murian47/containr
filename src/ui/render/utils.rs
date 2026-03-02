use anyhow::Context as _;
use image::Rgba;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Wrap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::state::app::App;
use crate::ui::theme;

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

    let path = resolve_output_path(path)?;
    if path.exists() && !force {
        anyhow::bail!("file exists (use save! to overwrite)");
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).context("failed to create parent dir")?;
    }
    fs::write(&path, text).context("failed to write file")?;
    Ok(path)
}

fn has_explicit_parent(path: &Path) -> bool {
    path.parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty() && parent != Path::new("."))
}

pub(in crate::ui) fn resolve_output_path(path: &str) -> anyhow::Result<PathBuf> {
    let path = expand_user_path(path);
    if path.is_absolute() || has_explicit_parent(&path) {
        return Ok(path);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(path))
}

pub(in crate::ui) fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

pub(in crate::ui) fn shell_row_highlight(app: &App) -> Style {
    app.theme.list_selected.to_style()
}

pub(in crate::ui) fn focus_marker_style(app: &App) -> Style {
    let base = app.theme.panel.to_style();
    let accent = app.theme.active.to_style();
    base.fg(accent.fg.unwrap_or(Color::White))
        .bg(base.bg.unwrap_or(Color::Reset))
        .add_modifier(accent.add_modifier)
}

pub(in crate::ui) fn draw_focus_accent(
    f: &mut ratatui::Frame,
    app: &App,
    area: Rect,
    active: bool,
) {
    if !active || area.width == 0 || area.height == 0 {
        return;
    }
    let accent = focus_marker_style(app);
    let line = "▎\n".repeat(area.height.saturating_sub(1) as usize) + "▎";
    let col = Rect {
        x: area.x,
        y: area.y,
        width: 1,
        height: area.height,
    };
    f.render_widget(
        Paragraph::new(Span::styled(line, accent)).wrap(Wrap { trim: false }),
        col,
    );
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

pub(in crate::ui) fn theme_color(spec: &str) -> Color {
    theme::parse_color(spec)
}
