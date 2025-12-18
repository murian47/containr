//! On-disk configuration handling.
//!
//! The config file is JSON and stored under `$XDG_CONFIG_HOME/containr/config.json`
//! (fallback: `$HOME/.config/containr/config.json`).
//! No secrets are stored; only non-sensitive connection metadata and UI preferences.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub target: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default = "default_docker_cmd")]
    pub docker_cmd: String,
}

fn default_docker_cmd() -> String {
    "docker".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBinding {
    // Key chord like "F10", "C-,", "C-S-C" etc.
    pub key: String,
    // Scope like "global" or "view:logs".
    #[serde(default = "default_key_scope")]
    pub scope: String,
    // Command to execute, like ":q!" or ":messages".
    pub cmd: String,
}

fn default_key_scope() -> String {
    "global".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainrConfig {
    // Versioned on-disk format for forward compatibility.
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub last_server: Option<String>,
    #[serde(default = "default_refresh_secs")]
    pub refresh_secs: u64,
    #[serde(default = "default_logs_tail")]
    pub logs_tail: usize,
    #[serde(default = "default_cmd_history_max")]
    pub cmd_history_max: usize,
    #[serde(default)]
    pub cmd_history: Vec<String>,
    #[serde(default = "default_active_theme")]
    pub active_theme: String,
    #[serde(default = "default_templates_dir")]
    pub templates_dir: String,
    // Per-view split layout preference for the main view ("horizontal" | "vertical").
    // Keys are view slugs like "containers", "images", ...
    #[serde(default)]
    pub view_layout: HashMap<String, String>,
    #[serde(default)]
    pub keymap: Vec<KeyBinding>,
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
}

fn default_version() -> u32 {
    9
}

fn default_refresh_secs() -> u64 {
    5
}

fn default_logs_tail() -> usize {
    500
}

fn default_cmd_history_max() -> usize {
    200
}

fn default_templates_dir() -> String {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Path::new(&dir)
            .join("containr")
            .join("templates")
            .to_string_lossy()
            .to_string();
    }
    if let Ok(home) = std::env::var("HOME") {
        return Path::new(&home)
            .join(".config")
            .join("containr")
            .join("templates")
            .to_string_lossy()
            .to_string();
    }
    "templates".to_string()
}

fn default_active_theme() -> String {
    "default".to_string()
}

impl Default for ContainrConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            last_server: None,
            refresh_secs: default_refresh_secs(),
            logs_tail: default_logs_tail(),
            cmd_history_max: default_cmd_history_max(),
            cmd_history: Vec::new(),
            active_theme: default_active_theme(),
            templates_dir: default_templates_dir(),
            view_layout: HashMap::new(),
            keymap: Vec::new(),
            servers: Vec::new(),
        }
    }
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    // Prefer XDG base dir spec, fall back to ~/.config.
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(Path::new(&dir).join("containr").join("config.json"));
    }
    let home = std::env::var("HOME").context("HOME is not set (and XDG_CONFIG_HOME not set)")?;
    Ok(Path::new(&home)
        .join(".config")
        .join("containr")
        .join("config.json"))
}

fn legacy_config_paths() -> anyhow::Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        out.push(Path::new(&dir).join("containr").join("servers.json"));
        out.push(Path::new(&dir).join("containr").join("serverlist.json"));
        out.push(Path::new(&dir).join("mcdoc").join("servers.json"));
        out.push(Path::new(&dir).join("mcdoc").join("serverlist.json"));
        out.push(Path::new(&dir).join("dockdash").join("serverlist.json"));
        return Ok(out);
    }
    let home = std::env::var("HOME").context("HOME is not set (and XDG_CONFIG_HOME not set)")?;
    out.push(
        Path::new(&home)
            .join(".config")
            .join("containr")
            .join("servers.json"),
    );
    out.push(
        Path::new(&home)
            .join(".config")
            .join("containr")
            .join("serverlist.json"),
    );
    out.push(
        Path::new(&home)
            .join(".config")
            .join("mcdoc")
            .join("servers.json"),
    );
    out.push(
        Path::new(&home)
            .join(".config")
            .join("mcdoc")
            .join("serverlist.json"),
    );
    out.push(
        Path::new(&home)
            .join(".config")
            .join("dockdash")
            .join("serverlist.json"),
    );
    Ok(out)
}

pub fn load_or_default(path: &Path) -> anyhow::Result<ContainrConfig> {
    // Missing config is not an error; we start with an empty list.
    if !path.exists() {
        if let Ok(legacy_paths) = legacy_config_paths() {
            for legacy in legacy_paths {
                if !legacy.exists() {
                    continue;
                }
                let bytes = fs::read(&legacy).with_context(|| {
                    format!("failed to read legacy config: {}", legacy.display())
                })?;
                let cfg: ContainrConfig = serde_json::from_slice(&bytes).with_context(|| {
                    format!("failed to parse legacy JSON: {}", legacy.display())
                })?;
                // Migrate by writing the new config file (do not delete the old one).
                let _ = save(path, &cfg);
                return Ok(cfg);
            }
        }
        return Ok(ContainrConfig::default());
    }
    let bytes =
        fs::read(path).with_context(|| format!("failed to read config: {}", path.display()))?;
    let cfg: ContainrConfig = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse JSON: {}", path.display()))?;
    Ok(cfg)
}

pub fn save(path: &Path, cfg: &ContainrConfig) -> anyhow::Result<()> {
    // Only non-secret connection metadata is stored (no passwords or tokens).
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(cfg).context("failed to serialize config")?;
    fs::write(path, bytes)
        .with_context(|| format!("failed to write config: {}", path.display()))?;
    Ok(())
}
