//! Theme loading and style configuration.
//!
//! Goals:
//! - Keep all colors/styles in one place.
//! - Allow multiple theme files and switching between them.
//! - Keep runtime code using semantic roles instead of hard-coded RGB values.

use anyhow::Context as _;
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StyleSpec {
    #[serde(default)]
    pub fg: String,
    #[serde(default)]
    pub bg: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub dim: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub reverse: bool,
}

impl Default for StyleSpec {
    fn default() -> Self {
        Self {
            fg: "default".to_string(),
            bg: "default".to_string(),
            bold: false,
            dim: false,
            underline: false,
            reverse: false,
        }
    }
}

impl StyleSpec {
    pub fn to_style(&self) -> Style {
        // "default" means: do not set the channel so it can inherit the surrounding widget style.
        // This is different from Color::Reset which forces the terminal's default color.
        let mut st = Style::default();
        if self.fg.trim().eq_ignore_ascii_case("default") {
            // leave fg unset
        } else {
            st = st.fg(parse_color(&self.fg));
        }
        if self.bg.trim().eq_ignore_ascii_case("default") {
            // leave bg unset
        } else {
            st = st.bg(parse_color(&self.bg));
        }
        let mut m = Modifier::empty();
        if self.bold {
            m |= Modifier::BOLD;
        }
        if self.dim {
            m |= Modifier::DIM;
        }
        if self.underline {
            m |= Modifier::UNDERLINED;
        }
        if self.reverse {
            m |= Modifier::REVERSED;
        }
        st = st.add_modifier(m);
        st
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThemeSpec {
    #[serde(default = "default_theme_version")]
    pub version: u32,
    #[serde(default = "default_theme_name")]
    pub name: String,

    // Base styles.
    #[serde(default = "default_bg")]
    pub background: StyleSpec,
    #[serde(default = "default_header")]
    pub header: StyleSpec,
    #[serde(default = "default_footer")]
    pub footer: StyleSpec,
    #[serde(default = "default_panel")]
    pub panel: StyleSpec,
    #[serde(default = "default_panel_focused")]
    pub panel_focused: StyleSpec,
    #[serde(default = "default_cmdline")]
    pub cmdline: StyleSpec,
    #[serde(default = "default_overlay")]
    pub overlay: StyleSpec,
    #[serde(default = "default_divider")]
    pub divider: StyleSpec,

    // Text roles.
    #[serde(default = "default_text")]
    pub text: StyleSpec,
    #[serde(default = "default_text_dim")]
    pub text_dim: StyleSpec,
    #[serde(default = "default_text_faint")]
    pub text_faint: StyleSpec,
    #[serde(default = "default_text_error")]
    pub text_error: StyleSpec,
    #[serde(default = "default_text_warn")]
    pub text_warn: StyleSpec,
    #[serde(default = "default_text_ok")]
    pub text_ok: StyleSpec,

    // Selection / highlights.
    #[serde(default = "default_list_selected")]
    pub list_selected: StyleSpec,
    #[serde(default = "default_table_header")]
    pub table_header: StyleSpec,
    #[serde(default = "default_active")]
    pub active: StyleSpec,
    #[serde(default = "default_marked")]
    pub marked: StyleSpec,

    // Scrollbars.
    #[serde(default = "default_scroll_track")]
    pub scroll_track: StyleSpec,
    #[serde(default = "default_scroll_thumb")]
    pub scroll_thumb: StyleSpec,

    // Syntax highlighting (YAML/JSON viewers).
    #[serde(default = "default_syntax_text")]
    pub syntax_text: StyleSpec,
    #[serde(default = "default_syntax_comment")]
    pub syntax_comment: StyleSpec,
    #[serde(default = "default_syntax_key")]
    pub syntax_key: StyleSpec,

    // Command line accents.
    #[serde(default = "default_cmdline_label")]
    pub cmdline_label: StyleSpec,
    #[serde(default = "default_cmdline_cursor")]
    pub cmdline_cursor: StyleSpec,
    #[serde(default = "default_cmdline_inactive")]
    pub cmdline_inactive: StyleSpec,
}

fn default_theme_version() -> u32 {
    1
}

fn default_theme_name() -> String {
    "default".to_string()
}

fn default_bg() -> StyleSpec {
    StyleSpec {
        fg: "#ffffff".to_string(),
        bg: "#101010".to_string(),
        ..StyleSpec::default()
    }
}

fn default_header() -> StyleSpec {
    StyleSpec {
        fg: "#ffffff".to_string(),
        bg: "#1c1c1c".to_string(),
        ..StyleSpec::default()
    }
}

fn default_footer() -> StyleSpec {
    StyleSpec {
        fg: "#c8c8c8".to_string(),
        bg: "#1c1c1c".to_string(),
        ..StyleSpec::default()
    }
}

fn default_panel() -> StyleSpec {
    StyleSpec {
        fg: "#ffffff".to_string(),
        bg: "#101010".to_string(),
        ..StyleSpec::default()
    }
}

fn default_panel_focused() -> StyleSpec {
    StyleSpec {
        fg: "#ffffff".to_string(),
        bg: "#18181e".to_string(),
        ..StyleSpec::default()
    }
}

fn default_cmdline() -> StyleSpec {
    // Command line / prompt row at the bottom.
    StyleSpec {
        fg: "#dcdcdc".to_string(),
        bg: "#101010".to_string(),
        ..StyleSpec::default()
    }
}

fn default_overlay() -> StyleSpec {
    StyleSpec {
        fg: "#ffffff".to_string(),
        bg: "#0c0c0c".to_string(),
        ..StyleSpec::default()
    }
}

fn default_divider() -> StyleSpec {
    StyleSpec {
        fg: "#2d2d2d".to_string(),
        bg: "#101010".to_string(),
        ..StyleSpec::default()
    }
}

fn default_text() -> StyleSpec {
    StyleSpec {
        fg: "#c8c8c8".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_text_dim() -> StyleSpec {
    StyleSpec {
        fg: "#8c8c8c".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_text_faint() -> StyleSpec {
    StyleSpec {
        fg: "#787878".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_text_error() -> StyleSpec {
    StyleSpec {
        fg: "#dc7878".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_text_warn() -> StyleSpec {
    StyleSpec {
        fg: "#ffbf40".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_text_ok() -> StyleSpec {
    StyleSpec {
        fg: "#7bd88f".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_list_selected() -> StyleSpec {
    // Mirrors the current "container list" selection styling.
    StyleSpec {
        fg: "default".to_string(),
        bg: "#00465a".to_string(),
        ..StyleSpec::default()
    }
}

fn default_table_header() -> StyleSpec {
    StyleSpec {
        fg: "#a0a0a0".to_string(),
        bg: "#161616".to_string(),
        ..StyleSpec::default()
    }
}

fn default_active() -> StyleSpec {
    StyleSpec {
        fg: "#ffc800".to_string(),
        bg: "default".to_string(),
        bold: true,
        ..StyleSpec::default()
    }
}

fn default_marked() -> StyleSpec {
    StyleSpec {
        fg: "#ffc800".to_string(),
        bg: "default".to_string(),
        bold: true,
        ..StyleSpec::default()
    }
}

fn default_scroll_track() -> StyleSpec {
    StyleSpec {
        fg: "#2d2d2d".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_scroll_thumb() -> StyleSpec {
    StyleSpec {
        fg: "#ffffff".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_syntax_text() -> StyleSpec {
    StyleSpec {
        fg: "#c8c8c8".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_syntax_comment() -> StyleSpec {
    StyleSpec {
        fg: "#787878".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_syntax_key() -> StyleSpec {
    StyleSpec {
        fg: "#8cbeff".to_string(),
        bg: "default".to_string(),
        ..StyleSpec::default()
    }
}

fn default_cmdline_label() -> StyleSpec {
    StyleSpec {
        fg: "#a0a0a0".to_string(),
        bg: "#101010".to_string(),
        ..StyleSpec::default()
    }
}

fn default_cmdline_cursor() -> StyleSpec {
    StyleSpec {
        fg: "#000000".to_string(),
        bg: "#dcdcdc".to_string(),
        ..StyleSpec::default()
    }
}

fn default_cmdline_inactive() -> StyleSpec {
    StyleSpec {
        fg: "#b4b4b4".to_string(),
        bg: "#101010".to_string(),
        ..StyleSpec::default()
    }
}

pub fn default_theme_spec() -> ThemeSpec {
    ThemeSpec {
        version: default_theme_version(),
        name: default_theme_name(),
        background: default_bg(),
        header: default_header(),
        footer: default_footer(),
        panel: default_panel(),
        panel_focused: default_panel_focused(),
        cmdline: default_cmdline(),
        overlay: default_overlay(),
        divider: default_divider(),
        text: default_text(),
        text_dim: default_text_dim(),
        text_faint: default_text_faint(),
        text_error: default_text_error(),
        text_warn: default_text_warn(),
        text_ok: default_text_ok(),
        list_selected: default_list_selected(),
        table_header: default_table_header(),
        active: default_active(),
        marked: default_marked(),
        scroll_track: default_scroll_track(),
        scroll_thumb: default_scroll_thumb(),
        syntax_text: default_syntax_text(),
        syntax_comment: default_syntax_comment(),
        syntax_key: default_syntax_key(),
        cmdline_label: default_cmdline_label(),
        cmdline_cursor: default_cmdline_cursor(),
        cmdline_inactive: default_cmdline_inactive(),
    }
}

pub fn themes_dir_from_config_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("themes")
}

pub fn list_theme_names(config_path: &Path) -> anyhow::Result<Vec<String>> {
    let dir = themes_dir_from_config_path(config_path);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out: Vec<String> = Vec::new();
    for ent in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let ent = ent?;
        if !ent.file_type()?.is_file() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if let Some(stem) = name.strip_suffix(".json") {
            if !stem.starts_with('.') {
                out.push(stem.to_string());
            }
        }
    }
    out.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    Ok(out)
}

pub fn ensure_default_theme_exists(config_path: &Path) -> anyhow::Result<()> {
    let dir = themes_dir_from_config_path(config_path);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join("default.json");
    if path.exists() {
        return Ok(());
    }
    save_theme(config_path, "default", &default_theme_spec())?;
    Ok(())
}

pub fn theme_path(config_path: &Path, name: &str) -> PathBuf {
    themes_dir_from_config_path(config_path).join(format!("{name}.json"))
}

pub fn load_theme(config_path: &Path, name: &str) -> anyhow::Result<ThemeSpec> {
    ensure_default_theme_exists(config_path)?;
    let path = theme_path(config_path, name);
    if !path.exists() {
        // Fallback to default.
        let fallback = theme_path(config_path, "default");
        let bytes = fs::read(&fallback)
            .with_context(|| format!("failed to read {}", fallback.display()))?;
        return serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse {}", fallback.display()));
    }
    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut spec: ThemeSpec = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if spec.name.trim().is_empty() {
        spec.name = name.to_string();
    }
    Ok(spec)
}

pub fn save_theme(config_path: &Path, name: &str, spec: &ThemeSpec) -> anyhow::Result<()> {
    let dir = themes_dir_from_config_path(config_path);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = theme_path(config_path, name);
    let bytes = serde_json::to_vec_pretty(spec).context("failed to serialize theme")?;
    fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn parse_color(s: &str) -> Color {
    let raw = s.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("default") || raw.eq_ignore_ascii_case("reset") {
        return Color::Reset;
    }
    if let Some(hex) = raw.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }
    // Best-effort named colors.
    match raw.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "white" => Color::White,
        _ => Color::Reset,
    }
}
