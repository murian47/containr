//! On-disk configuration handling.
//!
//! The config file is JSON and stored under `$XDG_CONFIG_HOME/containr/config.json`
//! (fallback: `$HOME/.config/containr/config.json`).
//! No secrets are stored; only non-sensitive connection metadata and UI preferences.

use anyhow::Context as _;
use crate::shell_parse::parse_shell_tokens;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct DockerCmd {
    tokens: Vec<String>,
}

impl DockerCmd {
    pub fn new(tokens: Vec<String>) -> Self {
        let tokens = tokens
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>();
        Self { tokens }
    }

    pub fn empty() -> Self {
        Self { tokens: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    pub fn from_shell(input: &str) -> anyhow::Result<Self> {
        let tokens = parse_shell_tokens(input).map_err(|e| anyhow::anyhow!(e))?;
        if tokens.is_empty() {
            Ok(DockerCmd::default())
        } else {
            Ok(DockerCmd::new(tokens))
        }
    }

    pub fn to_shell(&self) -> String {
        if self.tokens.is_empty() {
            return String::new();
        }
        self.tokens
            .iter()
            .map(|t| shell_escape_token(t))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn to_compose_shell(&self) -> String {
        if self.tokens.is_empty() {
            return String::new();
        }
        let first_is_compose_bin = self
            .tokens
            .first()
            .map(|t| t.ends_with("compose"))
            .unwrap_or(false);
        let has_compose_subcmd = self.tokens.iter().any(|t| t == "compose");
        if first_is_compose_bin || has_compose_subcmd {
            self.to_shell()
        } else {
            format!("{} compose", self.to_shell())
        }
    }
}

impl Default for DockerCmd {
    fn default() -> Self {
        Self::new(vec!["docker".to_string()])
    }
}

impl std::fmt::Display for DockerCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.tokens.join(" "))
    }
}

impl<'de> Deserialize<'de> for DockerCmd {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DockerCmdVisitor;

        impl<'de> serde::de::Visitor<'de> for DockerCmdVisitor {
            type Value = DockerCmd;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("string or array of strings")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let tokens = parse_shell_tokens(v).map_err(E::custom)?;
                if tokens.is_empty() {
                    Ok(DockerCmd::default())
                } else {
                    Ok(DockerCmd::new(tokens))
                }
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut tokens: Vec<String> = Vec::new();
                while let Some(v) = seq.next_element::<String>()? {
                    if !v.trim().is_empty() {
                        tokens.push(v);
                    }
                }
                if tokens.is_empty() {
                    Ok(DockerCmd::default())
                } else {
                    Ok(DockerCmd::new(tokens))
                }
            }
        }

        deserializer.deserialize_any(DockerCmdVisitor)
    }
}

fn shell_escape_token(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/:@=".contains(c))
    {
        return text.to_string();
    }
    let escaped = text.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub target: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default = "default_docker_cmd")]
    pub docker_cmd: DockerCmd,
}

fn default_docker_cmd() -> DockerCmd {
    DockerCmd::default()
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
    #[serde(default)]
    pub editor_cmd: String,
    // Per-view split layout preference for the main view ("horizontal" | "vertical").
    // Keys are view slugs like "containers", "images", ...
    #[serde(default)]
    pub view_layout: HashMap<String, String>,
    #[serde(default)]
    pub keymap: Vec<KeyBinding>,
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
    #[serde(default)]
    pub git_autocommit: bool,
    #[serde(default)]
    pub git_autocommit_confirm: bool,
    #[serde(default = "default_image_update_concurrency")]
    pub image_update_concurrency: usize,
    #[serde(default)]
    pub image_update_debug: bool,
    #[serde(default)]
    pub image_update_autocheck: bool,
    #[serde(default = "default_kitty_graphics")]
    pub kitty_graphics: bool,
    #[serde(default)]
    pub log_dock_enabled: bool,
    #[serde(default = "default_log_dock_height")]
    pub log_dock_height: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistryAuth {
    Anonymous,
    Basic,
    BearerToken,
    GithubPat,
}

fn default_registry_auth() -> RegistryAuth {
    RegistryAuth::Anonymous
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub host: String,
    #[serde(default = "default_registry_auth")]
    pub auth: RegistryAuth,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub secret_keyring: Option<String>,
    #[serde(default)]
    pub test_repo: Option<String>,
}

impl std::fmt::Debug for RegistryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryEntry")
            .field("host", &self.host)
            .field("auth", &self.auth)
            .field("username", &self.username)
            .field("secret", &self.secret.as_ref().map(|_| "****"))
            .field(
                "secret_keyring",
                &self.secret_keyring.as_ref().map(|name| format!("key:{name}")),
            )
            .field("test_repo", &self.test_repo)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistriesConfig {
    #[serde(default = "default_registries_version")]
    pub version: u32,
    #[serde(default)]
    pub age_identity: String,
    #[serde(default)]
    pub registries: Vec<RegistryEntry>,
    #[serde(default)]
    pub default_registry: Option<String>,
}

fn default_registries_version() -> u32 {
    1
}

impl Default for RegistriesConfig {
    fn default() -> Self {
        Self {
            version: default_registries_version(),
            age_identity: String::new(),
            registries: Vec::new(),
            default_registry: None,
        }
    }
}

fn default_version() -> u32 {
    10
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

fn default_image_update_concurrency() -> usize {
    4
}

fn default_log_dock_height() -> u16 {
    5
}

fn default_kitty_graphics() -> bool {
    true
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
            editor_cmd: String::new(),
            view_layout: HashMap::new(),
            keymap: Vec::new(),
            servers: Vec::new(),
            git_autocommit: false,
            git_autocommit_confirm: false,
            image_update_concurrency: default_image_update_concurrency(),
            image_update_debug: false,
            image_update_autocheck: false,
            kitty_graphics: default_kitty_graphics(),
            log_dock_enabled: false,
            log_dock_height: default_log_dock_height(),
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

pub fn registries_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("registries.json")
}

pub fn load_registries(config_path: &Path) -> anyhow::Result<RegistriesConfig> {
    let path = registries_path(config_path);
    if !path.exists() {
        let mut cfg = RegistriesConfig::default();
        cfg.age_identity = "~/.config/containr/age.key".to_string();
        cfg.registries.push(RegistryEntry {
            host: "docker.io".to_string(),
            auth: RegistryAuth::Anonymous,
            username: None,
            secret: None,
            secret_keyring: None,
            test_repo: None,
        });
        let _ = save_registries(&path, &cfg);
        return Ok(cfg);
    }
    let bytes =
        fs::read(&path).with_context(|| format!("failed to read registries: {}", path.display()))?;
    let cfg: RegistriesConfig = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse registries: {}", path.display()))?;
    Ok(cfg)
}

pub fn save_registries(path: &Path, cfg: &RegistriesConfig) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create registries dir: {}", parent.display()))?;
    }
    let bytes =
        serde_json::to_vec_pretty(cfg).context("failed to serialize registries config")?;
    fs::write(path, bytes)
        .with_context(|| format!("failed to write registries: {}", path.display()))?;
    Ok(())
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
