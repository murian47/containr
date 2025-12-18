//! Theme command implementation (`:theme ...`).
//!
//! Theme files are stored next to the main config:
//! - `$XDG_CONFIG_HOME/containr/themes/<name>.json`
//! - fallback: `$HOME/.config/containr/themes/<name>.json`
//!
//! The active theme name is persisted in the main config file.

use super::super::{App, MsgLevel, ShellInteractive, shell_escape_sh_arg};
use crate::ui::theme;
use anyhow::Context as _;
use std::fs;

fn validate_theme_name(raw: &str) -> anyhow::Result<String> {
    let name = raw.trim();
    anyhow::ensure!(!name.is_empty(), "theme name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "theme name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(!name.starts_with('.'), "theme name must not start with '.'");
    anyhow::ensure!(name != "." && name != "..", "invalid theme name");
    Ok(name.to_string())
}

pub fn set_theme(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = validate_theme_name(name)?;
    let spec = theme::load_theme(&app.config_path, &name)?;
    app.theme_name = name.clone();
    app.theme = spec;
    app.persist_config();
    app.set_info(format!("theme: {name}"));
    Ok(())
}

pub fn new_theme(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = validate_theme_name(name)?;
    if name == "default" {
        anyhow::bail!("theme name is reserved: default");
    }
    theme::ensure_default_theme_exists(&app.config_path)?;
    let path = theme::theme_path(&app.config_path, &name);
    anyhow::ensure!(!path.exists(), "theme already exists: {}", path.display());
    let mut spec = theme::default_theme_spec();
    spec.name = name.clone();
    theme::save_theme(&app.config_path, &name, &spec)?;
    // Open editor immediately.
    edit_theme(app, &name)?;
    Ok(())
}

pub fn edit_theme(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = validate_theme_name(name)?;
    theme::ensure_default_theme_exists(&app.config_path)?;
    let path = theme::theme_path(&app.config_path, &name);
    if !path.exists() {
        // Create as copy of default.
        let mut spec = theme::default_theme_spec();
        spec.name = name.clone();
        theme::save_theme(&app.config_path, &name, &spec)?;
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let cmd = format!(
        "{} {}",
        editor,
        shell_escape_sh_arg(&path.to_string_lossy())
    );
    if name == app.theme_name {
        app.theme_refresh_after_edit = Some(name);
    }
    app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
    Ok(())
}

pub fn delete_theme(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = validate_theme_name(name)?;
    anyhow::ensure!(name != "default", "cannot delete default theme");
    let path = theme::theme_path(&app.config_path, &name);
    anyhow::ensure!(path.exists(), "theme does not exist: {}", path.display());
    let dir = theme::themes_dir_from_config_path(&app.config_path);
    let root = fs::canonicalize(&dir)?;
    let target = fs::canonicalize(&path)?;
    anyhow::ensure!(
        target.starts_with(&root),
        "refusing to delete outside themes dir"
    );
    fs::remove_file(&target).with_context(|| format!("failed to delete {}", target.display()))?;
    if app.theme_name == name {
        // Switch away.
        let _ = set_theme(app, "default");
    }
    Ok(())
}

pub fn reload_active_theme_after_edit(app: &mut App, name: &str) {
    match theme::load_theme(&app.config_path, name) {
        Ok(spec) => {
            if app.theme_name == name {
                app.theme = spec;
            }
        }
        Err(e) => {
            app.log_msg(
                MsgLevel::Warn,
                format!("failed to reload theme '{name}': {:#}", e),
            );
        }
    }
}
