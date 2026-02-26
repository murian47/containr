//! Terminal UI (TUI) entrypoint and rendering.
//!
//! High-level architecture:
//! - `run_tui` owns the terminal lifecycle and the event loop.
//! - `App` is the in-memory state machine for views, selections, and input modes.
//! - Background tasks (ssh/local docker commands) run asynchronously and feed results back via channels.
//! - Rendering reads from `App` and draws the current UI state.
//!
//! When refactoring, keep these boundaries:
//! - IO/runner functions should not mutate UI widgets directly.
//! - UI code should use semantic theme roles (`theme::ThemeSpec`) instead of hard-coded colors.

mod actions;
mod app_logging;
mod app_logs;
mod app_inspect;
mod app_registry;
mod app_selection;
mod app_state;
mod app_stacks;
mod app_theme_selector;
mod app_view;
mod commands;
mod helpers;
mod templates_ops;
mod render;
mod views;
mod state;
mod input;
mod cmd_history;
mod text_edit;
pub mod theme;

use render::layout::draw_shell_hr;
use render::details::draw_shell_main_details;
use render::sidebar::{
    draw_shell_sidebar, shell_sidebar_select_item,
};
use crate::ui::state::image_updates::{ImageUpdateResult, is_rate_limit_error};
use render::shell::draw_shell_main_list;
pub(in crate::ui) use render::messages::{
    draw_shell_messages_dock, draw_shell_messages_view, format_session_ts,
};
pub(in crate::ui) use render::root::draw;
pub(in crate::ui) use render::help::draw_shell_help_view;
pub(in crate::ui) use render::inspect::draw_shell_inspect_view;
pub(in crate::ui) use render::logs::draw_shell_logs_view;
pub(in crate::ui) use render::registries::draw_shell_registries_table;
pub(in crate::ui) use render::tables::{
    draw_shell_containers_table, draw_shell_images_table, draw_shell_networks_table,
    draw_shell_volumes_table, shell_header_style,
};
pub(in crate::ui) use actions::{service_name_from_label_list, stack_compose_dirs, template_name_from_stack};
pub(in crate::ui) use app_view::shell_cycle_focus;
pub(in crate::ui) use helpers::{
    deploy_remote_dir_for, deploy_remote_net_dir_for, ensure_template_id, extract_template_id,
    parse_kv_args, shell_single_quote,
};
pub(in crate::ui) use state::persistence::{ensure_unique_server_name, find_server_by_name};
pub(in crate::ui) use templates_ops::{
    create_net_template, create_template, delete_net_template, delete_template,
    export_net_template, export_stack_template, extract_net_template_description,
    extract_template_description, images_from_compose, maybe_autocommit_templates,
};
#[cfg(test)]
pub(crate) use crate::ui::commands::cmdline_cmd::parse_cmdline_tokens;
use render::highlight::{json_highlight_line, yaml_highlight_line};
pub(in crate::ui) use render::status::image_update_indicator;
use render::utils::{
    expand_user_path, is_container_stopped, shell_escape_sh_arg, shell_row_highlight,
    theme_color_rgba,
};
use render::stacks::stack_name_from_labels;
use cmd_history::CmdHistory;
pub(in crate::ui) use text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};

use crate::config::{self, DockerCmd, KeyBinding, ServerEntry};
use crate::docker::{
    self, ContainerAction, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow,
};
use crate::runner::Runner;
use crate::services::image_update::ImageUpdateService;
use crate::ssh::Ssh;
use anyhow::Context as _;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use image::{DynamicImage, Rgba, RgbaImage};
use reqwest::{Client, StatusCode, Url};
use reqwest::header::WWW_AUTHENTICATE;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::fs;
use std::io::{self, Read, Stdout, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    collections::{HashMap, HashSet},
};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::task::JoinSet;
use age::Decryptor;
use age::Encryptor;
use age::armor::{ArmoredWriter, Format};
use age::secrecy::ExposeSecret;
use age::x25519;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct KeySpec {
    mods: u8, // bitmask: 1=Ctrl 2=Shift 4=Alt
    code: KeyCodeNorm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyScope {
    Always,
    Global,
    View(ShellView),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyCodeNorm {
    Char(char),
    F(u8),
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

include!("keys.inc.rs");

#[derive(Debug, Default, Clone)]
struct ShellCmdlineState {
    mode: bool,
    input: String,
    cursor: usize,
    confirm: Option<ShellConfirm>,
    history: CmdHistory,
}

#[derive(Debug, Clone)]
struct ShellMessagesState {
    scroll: usize,  // cursor (absolute); usize::MAX = last
    hscroll: usize, // horizontal scroll
    return_view: ShellView,
}

#[derive(Debug, Clone)]
struct ShellHelpState {
    scroll: usize,
    return_view: ShellView,
}

#[derive(Debug, Clone)]
struct ThemeSelectorState {
    names: Vec<String>,
    selected: usize,
    scroll: usize,
    page_size: usize,
    center_on_open: bool,
    return_view: ShellView,
    base_theme_name: String,
    preview_theme: theme::ThemeSpec,
    error: Option<String>,
    search_mode: bool,
    search_input: String,
    search_cursor: usize,
}

#[derive(Debug, Clone)]
struct InspectState {
    loading: bool,
    error: Option<String>,
    value: Option<Value>,
    target: Option<InspectTarget>,
    for_id: Option<String>,
    lines: Vec<InspectLine>,
    selected: usize,
    scroll_top: usize,
    scroll: usize,
    query: String,
    expanded: HashSet<String>,
    match_paths: Vec<String>,
    path_rank: HashMap<String, usize>,
    mode: InspectMode,
    input: String,
    input_cursor: usize,
    cmd_history: CmdHistory,
}

#[derive(Debug, Clone)]
struct LogsState {
    loading: bool,
    error: Option<String>,
    text: Option<String>,
    for_id: Option<String>,
    tail: usize,
    cursor: usize,
    scroll_top: usize,
    select_anchor: Option<usize>,
    hscroll: usize,
    max_width: usize,
    mode: LogsMode,
    input: String,
    query: String,
    command: String,
    input_cursor: usize,
    command_cursor: usize,
    cmd_history: CmdHistory,
    use_regex: bool,
    regex: Option<Regex>,
    regex_error: Option<String>,
    match_lines: Vec<usize>,
    show_line_numbers: bool,
}

#[derive(Debug, Clone)]
struct TemplatesState {
    dir: PathBuf,
    kind: TemplatesKind,

    templates: Vec<TemplateEntry>,
    templates_selected: usize,
    templates_error: Option<String>,
    templates_details_scroll: usize,
    templates_refresh_after_edit: Option<String>,
    template_deploy_inflight: HashMap<String, DeployMarker>,
    git_head: Option<String>,
    git_remote_templates: HashMap<String, GitRemoteStatus>,
    dirty_templates: HashSet<String>,
    untracked_templates: HashSet<String>,

    net_templates: Vec<NetTemplateEntry>,
    net_templates_selected: usize,
    net_templates_error: Option<String>,
    net_templates_details_scroll: usize,
    net_templates_refresh_after_edit: Option<String>,
    net_template_deploy_inflight: HashMap<String, DeployMarker>,
    dirty_net_templates: HashSet<String>,
    untracked_net_templates: HashSet<String>,
    git_remote_net_templates: HashMap<String, GitRemoteStatus>,
    ai_edit_snapshot: Option<TemplateEditSnapshot>,
}

#[derive(Clone, Debug)]
struct TemplateEditSnapshot {
    kind: TemplatesKind,
    name: String,
    path: PathBuf,
    hash: Option<u64>,
}

#[allow(private_interfaces)]
pub(crate) fn shell_begin_confirm(app: &mut App, label: impl Into<String>, cmdline: impl Into<String>) {
    app.shell_cmdline.mode = true;
    app.shell_cmdline.input.clear();
    app.shell_cmdline.cursor = 0;
    app.shell_cmdline.confirm = Some(ShellConfirm {
        label: label.into(),
        cmdline: cmdline.into(),
    });
}

pub(in crate::ui) fn input_window_with_cursor(
    text: &str,
    cursor: usize,
    width: usize,
) -> (String, String, String) {
    // Returns (before_cursor, cursor_cell, after_cursor) of the visible window.
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let cursor = cursor.min(len);

    if len <= width {
        let before: String = chars.iter().take(cursor).collect();
        let at = if cursor < len {
            chars[cursor].to_string()
        } else {
            " ".to_string()
        };
        let after: String = chars.iter().skip(cursor.saturating_add(1)).collect();
        return (before, at, after);
    }

    let mut start = 0usize;
    if cursor >= width {
        start = cursor - width + 1;
    }
    if start + width > len {
        start = len - width;
    }
    let end = (start + width).min(len);
    let rel = cursor.saturating_sub(start).min(end - start);
    let before: String = chars[start..start + rel].iter().collect();
    let at = if start + rel < end {
        chars[start + rel].to_string()
    } else {
        " ".to_string()
    };
    let after_start = (start + rel + 1).min(end);
    let after: String = chars[after_start..end].iter().collect();
    (before, at, after)
}

fn is_single_letter_without_modifiers(spec: KeySpec) -> bool {
    spec.mods == 0 && matches!(spec.code, KeyCodeNorm::Char(c) if c.is_ascii_alphabetic())
}

fn cmdline_is_destructive(raw: &str) -> bool {
    let s = raw.trim().trim_start_matches(':').trim();
    if s.is_empty() {
        return false;
    }
    let mut it = s.split_whitespace();
    let Some(cmd_raw) = it.next() else {
        return false;
    };
    let cmd_raw = cmd_raw
        .strip_prefix('!')
        .unwrap_or(cmd_raw)
        .trim_matches('!');
    let cmd = cmd_raw.to_ascii_lowercase();
    let sub = it.next().unwrap_or("").to_ascii_lowercase();
    match cmd.as_str() {
        "stack" | "stacks" | "stk" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "container" | "ctr" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "template" | "tpl" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "nettemplate" | "nettpl" | "ntpl" | "nt" => {
            matches!(sub.as_str(), "rm" | "remove" | "delete")
        }
        "theme" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "server" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "image" | "img" => matches!(sub.as_str(), "rm" | "remove" | "delete" | "untag"),
        "volume" | "vol" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "network" | "net" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TemplatesKind {
    Stacks,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitRemoteStatus {
    Unknown,
    UpToDate,
    Ahead,
    Behind,
    Diverged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListMode {
    Flat,
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveView {
    Containers,
    Stacks,
    Images,
    Volumes,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ShellView {
    Dashboard,
    Stacks,
    Containers,
    Images,
    Volumes,
    Networks,
    Templates,
    Registries,
    Inspect,
    Logs,
    Help,
    Messages,
    ThemeSelector,
}

impl ShellView {
    fn slug(self) -> &'static str {
        match self {
            ShellView::Dashboard => "dashboard",
            ShellView::Stacks => "stacks",
            ShellView::Containers => "containers",
            ShellView::Images => "images",
            ShellView::Volumes => "volumes",
            ShellView::Networks => "networks",
            ShellView::Templates => "templates",
            ShellView::Registries => "registries",
            ShellView::Inspect => "inspect",
            ShellView::Logs => "logs",
            ShellView::Help => "help",
            ShellView::Messages => "messages",
            ShellView::ThemeSelector => "themes",
        }
    }

    fn title(self) -> &'static str {
        match self {
            ShellView::Dashboard => "Dashboard",
            ShellView::Stacks => "Stacks",
            ShellView::Containers => "Containers",
            ShellView::Images => "Images",
            ShellView::Volumes => "Volumes",
            ShellView::Networks => "Networks",
            ShellView::Templates => "Templates",
            ShellView::Registries => "Registries",
            ShellView::Inspect => "Inspect",
            ShellView::Logs => "Logs",
            ShellView::Help => "Help",
            ShellView::Messages => "Messages",
            ShellView::ThemeSelector => "Themes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellFocus {
    Sidebar,
    List,
    Details,
    Dock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellSplitMode {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellSidebarItem {
    Separator,
    Gap,
    Server(usize),
    Module(ShellView),
    Action(ShellAction),
}

#[derive(Clone, Debug)]
enum ShellInteractive {
    RunCommand { cmd: String },
    RunLocalCommand { cmd: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MsgLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
struct SessionMsg {
    at: OffsetDateTime,
    level: MsgLevel,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellAction {
    Inspect,
    Logs,
    Start,
    Stop,
    Restart,
    Delete,
    StackUpdate,
    StackUpdateAll,
    Console,
    ImageUntag,
    ImageForceRemove,
    VolumeRemove,
    NetworkRemove,
    RegistryTest,
    TemplateAi,
    TemplateEdit,
    TemplateNew,
    TemplateDelete,
    TemplateDeploy,
    TemplateRedeploy,
}

impl ShellAction {
    fn label(self) -> &'static str {
        match self {
            ShellAction::Inspect => "Inspect",
            ShellAction::Logs => "Logs",
            ShellAction::Start => "Start",
            ShellAction::Stop => "Stop",
            ShellAction::Restart => "Restart",
            ShellAction::Delete => "Delete",
            ShellAction::StackUpdate => "Update",
            ShellAction::StackUpdateAll => "Update all",
            ShellAction::Console => "Console",
            ShellAction::ImageUntag => "Untag",
            ShellAction::ImageForceRemove => "Remove",
            ShellAction::VolumeRemove => "Remove",
            ShellAction::NetworkRemove => "Remove",
            ShellAction::RegistryTest => "Test",
            ShellAction::TemplateAi => "AI",
            ShellAction::TemplateEdit => "Edit",
            ShellAction::TemplateNew => "New",
            ShellAction::TemplateDelete => "Delete",
            ShellAction::TemplateDeploy => "Deploy",
            ShellAction::TemplateRedeploy => "Redeploy",
        }
    }

    fn ctrl_hint(self) -> &'static str {
        match self {
            ShellAction::Inspect => "^i",
            ShellAction::Logs => "^l",
            ShellAction::Start => "^s",
            ShellAction::Stop => "^o",
            ShellAction::Restart => "^r",
            ShellAction::Delete => "^d",
            ShellAction::StackUpdate => "^u",
            ShellAction::StackUpdateAll => "^U",
            // Console: ^c = bash, ^C = sh (Ctrl+Shift+C)
            ShellAction::Console => "^c",
            // Non-container actions: keep a separate chord to avoid ambiguity
            ShellAction::ImageUntag => "^u",
            ShellAction::ImageForceRemove => "^d",
            ShellAction::VolumeRemove => "^d",
            ShellAction::NetworkRemove => "^d",
            ShellAction::RegistryTest => "^y",
            ShellAction::TemplateAi => "^a",
            ShellAction::TemplateEdit => "^e",
            ShellAction::TemplateNew => "^n",
            ShellAction::TemplateDelete => "^d",
            ShellAction::TemplateDeploy => "^y",
            ShellAction::TemplateRedeploy => "^Y",
        }
    }
}

fn shell_module_shortcut(view: ShellView) -> char {
    match view {
        ShellView::Dashboard => 'd',
        ShellView::Stacks => 's',
        ShellView::Containers => 'c',
        ShellView::Images => 'i',
        ShellView::Volumes => 'v',
        ShellView::Networks => 'n',
        ShellView::Templates => 't',
        ShellView::Registries => 'r',
        ShellView::Inspect => 'i',
        ShellView::Logs => 'l',
        ShellView::Help => '?',
        // Not a primary module; used only for internal navigation/help display.
        ShellView::Messages => 'g',
        ShellView::ThemeSelector => 't',
    }
}

fn build_server_shortcuts(servers: &[ServerEntry]) -> Vec<char> {
    // First 1..9 use digits. Remaining use deterministic "random-looking" uppercase letters.
    let mut out: Vec<char> = Vec::with_capacity(servers.len());
    let mut used: HashSet<char> = HashSet::new();

    for (i, _) in servers.iter().enumerate() {
        if i < 9 {
            let ch = char::from_digit((i + 1) as u32, 10).unwrap_or('?');
            out.push(ch);
            used.insert(ch);
        } else {
            out.push('\0');
        }
    }

    // Avoid letters that could be confused with common module letters in uppercase.
    for ch in ['C', 'S', 'M', 'I', 'V', 'N', 'L'] {
        used.insert(ch);
    }

    let pool: Vec<char> = ('A'..='Z').filter(|c| !used.contains(c)).collect();
    if pool.is_empty() {
        for i in 9..servers.len() {
            out[i] = 'A';
        }
        return out;
    }

    // Stable assignment based on server name.
    for i in 9..servers.len() {
        let name = &servers[i].name;
        let mut h: u64 = 0xcbf29ce484222325;
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let start = (h as usize) % pool.len();
        let mut chosen = None;
        for off in 0..pool.len() {
            let c = pool[(start + off) % pool.len()];
            if !used.contains(&c) {
                chosen = Some(c);
                break;
            }
        }
        let c = chosen.unwrap_or(pool[start]);
        out[i] = c;
        used.insert(c);
    }
    out
}

#[derive(Clone, Debug)]
enum ViewEntry {
    StackHeader {
        name: String,
        total: usize,
        running: usize,
        expanded: bool,
    },
    UngroupedHeader {
        total: usize,
        running: usize,
    },
    Container {
        id: String,
        indent: usize,
    },
}

#[derive(Clone, Debug)]
struct StackEntry {
    name: String,
    total: usize,
    running: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StackDetailsFocus {
    Containers,
    Networks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogsMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectKind {
    Container,
    Image,
    Volume,
    Network,
}

#[derive(Debug, Clone)]
struct InspectTarget {
    kind: InspectKind,
    key: String,
    arg: String,
    label: String,
}

#[derive(Debug, Clone)]
struct InspectLine {
    path: String,     // JSON pointer
    depth: usize,     // indentation
    label: String,    // key/index label (already printable)
    summary: String,  // preview text (no newlines)
    expandable: bool, // object/array
    expanded: bool,   // current state
    matches: bool,    // search match
}

#[derive(Clone, Debug)]
struct TemplateEntry {
    name: String,
    dir: PathBuf,
    compose_path: PathBuf,
    has_compose: bool,
    desc: String,
    template_id: Option<String>,
}

#[derive(Clone, Debug)]
struct NetTemplateEntry {
    name: String,
    dir: PathBuf,
    cfg_path: PathBuf,
    has_cfg: bool,
    desc: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct NetworkTemplateIpv4 {
    subnet: Option<String>,
    gateway: Option<String>,
    #[serde(rename = "ip_range")]
    ip_range: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct NetworkTemplateSpec {
    name: String,
    #[allow(dead_code)]
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default, rename = "ipvlan_mode")]
    ipvlan_mode: Option<String>,
    #[serde(default)]
    internal: Option<bool>,
    #[serde(default)]
    attachable: Option<bool>,
    #[serde(default)]
    ipv4: Option<NetworkTemplateIpv4>,
    #[serde(default)]
    options: Option<HashMap<String, String>>,
    #[serde(default)]
    labels: Option<HashMap<String, String>>,
}

const IMAGE_UPDATE_TTL_SECS: i64 = 24 * 60 * 60;
const RATE_LIMIT_WINDOW_SECS: i64 = 6 * 60 * 60;
const RATE_LIMIT_MAX: usize = 100;
const RATE_LIMIT_WARN: usize = 80;

#[derive(Clone, Debug, Serialize, Deserialize)]
enum ImageUpdateKind {
    UpToDate,
    UpdateAvailable,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ImageUpdateEntry {
    checked_at: i64,
    status: ImageUpdateKind,
    local_digest: Option<String>,
    remote_digest: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TemplateDeployEntry {
    server_name: String,
    timestamp: i64,
    #[serde(default)]
    commit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RegistryTestEntry {
    checked_at: i64,
    ok: bool,
    message: String,
}

#[derive(Clone, Debug)]
struct RegistryAuthResolved {
    auth: config::RegistryAuth,
    username: Option<String>,
    secret: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct RateLimitEntry {
    hits: Vec<i64>,
    limited_until: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
struct LocalState {
    version: u32,
    #[serde(default)]
    image_updates: HashMap<String, ImageUpdateEntry>,
    #[serde(default)]
    rate_limits: HashMap<String, RateLimitEntry>,
    #[serde(default)]
    template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    #[serde(default)]
    net_template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    #[serde(default)]
    registry_tests: HashMap<String, RegistryTestEntry>,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct DeployMarker {
    started: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct ActionMarker {
    action: ContainerAction,
    until: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct SimpleMarker {
    until: Instant,
}

#[derive(Clone, Debug)]
enum ActionErrorKind {
    InUse,
    Other,
}

#[derive(Clone, Debug)]
struct LastActionError {
    at: OffsetDateTime,
    action: String,
    kind: ActionErrorKind,
    message: String,
}

#[derive(Clone, Debug)]
struct StackUpdateService {
    name: String,
    container_id: String,
    image: String,
}

fn classify_action_error(msg: &str) -> ActionErrorKind {
    let s = msg.to_ascii_lowercase();
    if s.contains("in use")
        || s.contains("being used")
        || s.contains("has active endpoints")
        || s.contains("active endpoints")
        || s.contains("is being used")
    {
        ActionErrorKind::InUse
    } else {
        ActionErrorKind::Other
    }
}

fn now_local() -> OffsetDateTime {
    OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc())
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn image_updates_path() -> PathBuf {
    if let Ok(root) = std::env::var("XDG_STATE_HOME") {
        let root = PathBuf::from(root);
        return root.join("containr").join("state.json");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("containr")
            .join("state.json");
    }
    PathBuf::from("state.json")
}

fn load_local_state(
) -> (
    PathBuf,
    HashMap<String, ImageUpdateEntry>,
    HashMap<String, RateLimitEntry>,
    HashMap<String, Vec<TemplateDeployEntry>>,
    HashMap<String, Vec<TemplateDeployEntry>>,
    HashMap<String, RegistryTestEntry>,
) {
    let path = image_updates_path();
    let data = fs::read_to_string(&path).ok();
    let value = data
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    let image_updates = value
        .as_ref()
        .and_then(|v| v.get("image_updates"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let rate_limits = value
        .as_ref()
        .and_then(|v| v.get("rate_limits"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut template_deploys: HashMap<String, Vec<TemplateDeployEntry>> = HashMap::new();
    if let Some(v) = value.as_ref().and_then(|v| v.get("template_deploys")) {
        if let Some(obj) = v.as_object() {
            for (key, entry) in obj {
                if entry.is_array() {
                    if let Ok(list) = serde_json::from_value::<Vec<TemplateDeployEntry>>(entry.clone())
                    {
                        if !list.is_empty() {
                            template_deploys.insert(key.clone(), list);
                        }
                    }
                    continue;
                }
                if let Ok(single) = serde_json::from_value::<TemplateDeployEntry>(entry.clone()) {
                    template_deploys.insert(key.clone(), vec![single]);
                    continue;
                }
                let server_name = entry
                    .get("server_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = entry
                    .get("timestamp")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if !server_name.trim().is_empty() && timestamp > 0 {
                    template_deploys.insert(
                        key.clone(),
                        vec![TemplateDeployEntry {
                            server_name,
                            timestamp,
                            commit: None,
                        }],
                    );
                }
            }
        }
    }
    let mut net_template_deploys: HashMap<String, Vec<TemplateDeployEntry>> = HashMap::new();
    if let Some(v) = value.as_ref().and_then(|v| v.get("net_template_deploys")) {
        if let Some(obj) = v.as_object() {
            for (key, entry) in obj {
                if entry.is_array() {
                    if let Ok(list) = serde_json::from_value::<Vec<TemplateDeployEntry>>(entry.clone())
                    {
                        if !list.is_empty() {
                            net_template_deploys.insert(key.clone(), list);
                        }
                    }
                    continue;
                }
                if let Ok(single) = serde_json::from_value::<TemplateDeployEntry>(entry.clone()) {
                    net_template_deploys.insert(key.clone(), vec![single]);
                    continue;
                }
                let server_name = entry
                    .get("server_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = entry
                    .get("timestamp")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if !server_name.trim().is_empty() && timestamp > 0 {
                    net_template_deploys.insert(
                        key.clone(),
                        vec![TemplateDeployEntry {
                            server_name,
                            timestamp,
                            commit: None,
                        }],
                    );
                }
            }
        }
    }
    let registry_tests = value
        .as_ref()
        .and_then(|v| v.get("registry_tests"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    (
        path,
        image_updates,
        rate_limits,
        template_deploys,
        net_template_deploys,
        registry_tests,
    )
}

fn truncate_msg(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max.saturating_sub(3) {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum ActionRequest {
    Container {
        action: ContainerAction,
        id: String,
    },
    RegistryTest {
        host: String,
        auth: RegistryAuthResolved,
        test_repo: Option<String>,
    },
    TemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_compose: PathBuf,
        pull: bool,
        force_recreate: bool,
        server_name: String,
        template_id: String,
        template_commit: Option<String>,
    },
    StackUpdate {
        stack_name: String,
        runner: Runner,
        docker: DockerCfg,
        compose_dirs: Vec<String>,
        pull: bool,
        dry: bool,
        force: bool,
        services: Vec<StackUpdateService>,
    },
    NetTemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_cfg: PathBuf,
        force: bool,
        server_name: String,
    },
    TemplateFromNetwork {
        name: String,
        source: String,
        network_id: String,
        templates_dir: PathBuf,
    },
    TemplateFromStack {
        name: String,
        stack_name: String,
        source: String,
        container_ids: Vec<String>,
        templates_dir: PathBuf,
    },
    TemplateFromContainer {
        name: String,
        source: String,
        container_id: String,
        templates_dir: PathBuf,
    },
    ImageUpdateCheck {
        image: String,
        debug: bool,
    },
    ImageUntag {
        marker_key: String,
        reference: String,
    },
    ImageForceRemove {
        marker_key: String,
        id: String,
    },
    ImagePush {
        marker_key: String,
        source_ref: String,
        target_ref: String,
        registry_host: String,
        auth: Option<RegistryAuthResolved>,
    },
    VolumeRemove {
        name: String,
    },
    NetworkRemove {
        id: String,
    },
}

struct App {
    containers: Vec<ContainerRow>,
    images: Vec<ImageRow>,
    volumes: Vec<VolumeRow>,
    networks: Vec<NetworkRow>,
    image_referenced_by_id: HashMap<String, bool>,
    image_referenced_count_by_id: HashMap<String, usize>,
    image_running_count_by_id: HashMap<String, usize>,
    image_containers_by_id: HashMap<String, Vec<String>>,
    volume_referenced_by_name: HashMap<String, bool>,
    volume_referenced_count_by_name: HashMap<String, usize>,
    volume_running_count_by_name: HashMap<String, usize>,
    volume_containers_by_name: HashMap<String, Vec<String>>,
    network_referenced_count_by_id: HashMap<String, usize>,
    network_containers_by_id: HashMap<String, Vec<String>>,
    images_unused_only: bool,
    volumes_unused_only: bool,
    usage_refresh_needed: bool,
    usage_loading: bool,
    selected: usize,
    active_view: ActiveView,
    list_mode: ListMode,
    view: Vec<ViewEntry>,
    view_dirty: bool,
    stack_collapsed: HashSet<String>,
    container_idx_by_id: HashMap<String, usize>,
    marked: HashSet<String>,          // container ids
    marked_images: HashSet<String>,   // image row keys (ref:repo:tag or id:<sha256..>)
    marked_volumes: HashSet<String>,  // volume names
    marked_networks: HashSet<String>, // network ids
    images_selected: usize,
    volumes_selected: usize,
    networks_selected: usize,
    last_refresh: Option<Instant>,
    last_loop_at: Option<Instant>,
    reset_screen: bool,
    conn_error: Option<String>,
    last_error: Option<String>,
    loading: bool,
    loading_since: Option<Instant>,
    action_inflight: HashMap<String, ActionMarker>,
    image_action_inflight: HashMap<String, SimpleMarker>,
    volume_action_inflight: HashMap<String, SimpleMarker>,
    network_action_inflight: HashMap<String, SimpleMarker>,
    stack_update_inflight: HashMap<String, DeployMarker>,
    stack_update_containers: HashMap<String, Vec<String>>,
    container_action_error: HashMap<String, LastActionError>,
    image_action_error: HashMap<String, LastActionError>,
    volume_action_error: HashMap<String, LastActionError>,
    network_action_error: HashMap<String, LastActionError>,
    stack_update_error: HashMap<String, LastActionError>,
    template_action_error: HashMap<String, LastActionError>,
    net_template_action_error: HashMap<String, LastActionError>,
    inspect: InspectState,

    servers: Vec<ServerEntry>,
    active_server: Option<String>,
    server_selected: usize,
    server_all_selected: bool,
    config_path: std::path::PathBuf,
    current_target: String,

    logs: LogsState,
    dashboard: DashboardState,
    dashboard_all: DashboardAllState,
    dashboard_image: Option<DashboardImageState>,

    ip_cache: HashMap<String, (String, Instant)>,
    ip_refresh_needed: bool,
    should_quit: bool,
    ascii_only: bool,
    kitty_graphics: bool,

    theme_name: String,
    theme: theme::ThemeSpec,
    header_logo_seed: u64,

    shell_view: ShellView,
    shell_last_main_view: ShellView,
    shell_focus: ShellFocus,
    shell_sidebar_collapsed: bool,
    shell_sidebar_hidden: bool,
    shell_sidebar_selected: usize,
    shell_split_mode: ShellSplitMode,
    shell_split_by_view: HashMap<String, ShellSplitMode>,
    shell_server_shortcuts: Vec<char>,
    shell_pending_interactive: Option<ShellInteractive>,
    shell_cmdline: ShellCmdlineState,
    shell_help: ShellHelpState,
    theme_selector: ThemeSelectorState,
    refresh_secs: u64,
    refresh_paused: bool,
    refresh_pause_reason: Option<String>,
    refresh_error_streak: u32,
    cmd_history_max: usize,
    git_autocommit: bool,
    git_autocommit_confirm: bool,
    editor_cmd: String,
    image_update_concurrency: usize,
    image_update_debug: bool,
    image_update_autocheck: bool,

    session_msgs: Vec<SessionMsg>,
    messages_seen_len: usize,
    shell_msgs: ShellMessagesState,
    log_dock_enabled: bool,
    log_dock_height: u16,

    keymap: Vec<KeyBinding>,
    keymap_parsed: HashMap<(KeyScope, KeySpec), String>,
    keymap_defaults: HashMap<(KeyScope, KeySpec), String>,

    templates_state: TemplatesState,
    image_updates: HashMap<String, ImageUpdateEntry>,
    image_updates_inflight: HashSet<String>,
    image_updates_path: PathBuf,
    rate_limits: HashMap<String, RateLimitEntry>,
    template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    net_template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    unknown_template_ids_warned: HashSet<String>,
    registries_cfg: config::RegistriesConfig,
    registry_auths: HashMap<String, RegistryAuthResolved>,
    registry_tests: HashMap<String, RegistryTestEntry>,

    theme_refresh_after_edit: Option<String>,

    stacks: Vec<StackEntry>,
    stacks_selected: usize,
    stacks_details_scroll: usize,
    stacks_networks_scroll: usize,
    stack_details_focus: StackDetailsFocus,
    stacks_only_running: bool,

    registries_selected: usize,
    registries_details_scroll: usize,

    container_details_scroll: usize,
    image_details_scroll: usize,
    volume_details_scroll: usize,
    network_details_scroll: usize,
    container_details_id: Option<String>,
    image_details_id: Option<String>,
    volume_details_id: Option<String>,
    network_details_id: Option<String>,
}

#[derive(Clone, Debug)]
struct ShellConfirm {
    label: String,
    cmdline: String, // command line without leading ':'
}

impl App {
    pub(crate) fn editor_cmd(&self) -> String {
        let configured = self.editor_cmd.trim();
        if !configured.is_empty() {
            return configured.to_string();
        }
        std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
    }

    fn new(
        servers: Vec<ServerEntry>,
        keymap: Vec<KeyBinding>,
        active_server: Option<String>,
        config_path: std::path::PathBuf,
        view_layout: HashMap<String, String>,
        theme_name: String,
        theme: theme::ThemeSpec,
        dashboard_picker: Option<Picker>,
        git_autocommit: bool,
        git_autocommit_confirm: bool,
        editor_cmd: String,
        image_update_concurrency: usize,
        image_update_debug: bool,
        image_update_autocheck: bool,
        kitty_graphics: bool,
        log_dock_enabled: bool,
        log_dock_height: u16,
        registries_cfg: config::RegistriesConfig,
    ) -> Self {
        let mut server_selected = 0usize;
        if let Some(name) = &active_server {
            if let Some(idx) = servers.iter().position(|s| &s.name == name) {
                server_selected = idx;
            }
        }
        let header_logo_seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_nanos() as u64
            ^ (std::process::id() as u64);
        let (
            image_updates_path,
            mut image_updates,
            mut rate_limits,
            template_deploys,
            net_template_deploys,
            registry_tests,
        ) = load_local_state();
        let now = now_unix();
        image_updates.retain(|_, v| now.saturating_sub(v.checked_at) <= IMAGE_UPDATE_TTL_SECS);
        rate_limits.retain(|_, v| {
            v.hits.retain(|ts| now.saturating_sub(*ts) <= RATE_LIMIT_WINDOW_SECS);
            if let Some(until) = v.limited_until {
                if now >= until {
                    v.limited_until = None;
                }
            }
            !v.hits.is_empty() || v.limited_until.is_some()
        });
        let theme_name_clone = theme_name.clone();
        let theme_clone = theme.clone();
        let dashboard_all = DashboardAllState {
            hosts: servers
                .iter()
                .map(|s| DashboardHostState {
                    name: s.name.clone(),
                    loading: false,
                    error: None,
                    snap: None,
                    latency_ms: None,
                })
                .collect(),
        };
        let mut app = Self {
            containers: Vec::new(),
            images: Vec::new(),
            volumes: Vec::new(),
            networks: Vec::new(),
            image_referenced_by_id: HashMap::new(),
            image_referenced_count_by_id: HashMap::new(),
            image_running_count_by_id: HashMap::new(),
            image_containers_by_id: HashMap::new(),
            images_unused_only: false,
            volume_referenced_by_name: HashMap::new(),
            volume_referenced_count_by_name: HashMap::new(),
            volume_running_count_by_name: HashMap::new(),
            volume_containers_by_name: HashMap::new(),
            network_referenced_count_by_id: HashMap::new(),
            network_containers_by_id: HashMap::new(),
            volumes_unused_only: false,
            usage_refresh_needed: true,
            usage_loading: false,
            selected: 0,
            active_view: ActiveView::Containers,
            list_mode: ListMode::Flat,
            view: Vec::new(),
            view_dirty: true,
            stack_collapsed: HashSet::new(),
            container_idx_by_id: HashMap::new(),
            marked: HashSet::new(),
            marked_images: HashSet::new(),
            marked_volumes: HashSet::new(),
            marked_networks: HashSet::new(),
            images_selected: 0,
            volumes_selected: 0,
            networks_selected: 0,
            last_refresh: None,
            last_loop_at: Some(Instant::now()),
            reset_screen: false,
            conn_error: None,
            last_error: None,
            loading: true,
            loading_since: Some(Instant::now()),
            action_inflight: HashMap::new(),
            image_action_inflight: HashMap::new(),
            volume_action_inflight: HashMap::new(),
            network_action_inflight: HashMap::new(),
            stack_update_inflight: HashMap::new(),
            stack_update_containers: HashMap::new(),
            container_action_error: HashMap::new(),
            image_action_error: HashMap::new(),
            volume_action_error: HashMap::new(),
            network_action_error: HashMap::new(),
            stack_update_error: HashMap::new(),
            template_action_error: HashMap::new(),
            net_template_action_error: HashMap::new(),
            inspect: InspectState {
                loading: false,
                error: None,
                value: None,
                target: None,
                for_id: None,
                lines: Vec::new(),
                selected: 0,
                scroll_top: 0,
                scroll: 0,
                query: String::new(),
                expanded: HashSet::new(),
                match_paths: Vec::new(),
                path_rank: HashMap::new(),
                mode: InspectMode::Normal,
                input: String::new(),
                input_cursor: 0,
                cmd_history: CmdHistory::new(),
            },

            servers,
            active_server,
            server_selected,
            server_all_selected: false,
            config_path,
            current_target: String::new(),

            logs: LogsState {
                loading: false,
                error: None,
                text: None,
                for_id: None,
                tail: 500,
                cursor: 0,
                scroll_top: 0,
                select_anchor: None,
                hscroll: 0,
                max_width: 0,
                mode: LogsMode::Normal,
                input: String::new(),
                query: String::new(),
                command: String::new(),
                input_cursor: 0,
                command_cursor: 0,
                cmd_history: CmdHistory::new(),
                use_regex: false,
                regex: None,
                regex_error: None,
                match_lines: Vec::new(),
                show_line_numbers: false,
            },
            dashboard: DashboardState {
                loading: true,
                last_disk_count: 0,
                suppress_image_frames: 0,
                ..DashboardState::default()
            },
            dashboard_all,
            dashboard_image: if kitty_graphics {
                dashboard_picker.map(|p| init_dashboard_image(p, &theme))
            } else {
                None
            },

            ip_cache: HashMap::new(),
            ip_refresh_needed: true,
            should_quit: false,
            ascii_only: false,
            kitty_graphics,
            theme_name,
            theme,
            header_logo_seed,
            shell_view: ShellView::Dashboard,
            shell_last_main_view: ShellView::Dashboard,
            shell_focus: ShellFocus::Sidebar,
            shell_sidebar_collapsed: false,
            shell_sidebar_hidden: false,
            shell_sidebar_selected: 0,
            shell_split_mode: ShellSplitMode::Horizontal,
            shell_split_by_view: view_layout
                .into_iter()
                .filter_map(|(k, v)| {
                    let mode = match v.to_ascii_lowercase().as_str() {
                        "h" | "hor" | "horizontal" => Some(ShellSplitMode::Horizontal),
                        "v" | "ver" | "vertical" => Some(ShellSplitMode::Vertical),
                        _ => None,
                    }?;
                    Some((k, mode))
                })
                .collect(),
            shell_server_shortcuts: Vec::new(),
            shell_pending_interactive: None,
            shell_cmdline: ShellCmdlineState {
                mode: false,
                input: String::new(),
                cursor: 0,
                confirm: None,
                history: CmdHistory::new(),
            },
            shell_help: ShellHelpState {
                scroll: 0,
                return_view: ShellView::Dashboard,
            },
            theme_selector: ThemeSelectorState {
                names: Vec::new(),
                selected: 0,
                scroll: 0,
                page_size: 0,
                center_on_open: false,
                return_view: ShellView::Dashboard,
                base_theme_name: theme_name_clone,
                preview_theme: theme_clone,
                error: None,
                search_mode: false,
                search_input: String::new(),
                search_cursor: 0,
            },
            refresh_secs: 5,
            refresh_paused: false,
            refresh_pause_reason: None,
            refresh_error_streak: 0,
            cmd_history_max: 200,
            git_autocommit,
            git_autocommit_confirm,
            editor_cmd,
            image_update_concurrency: image_update_concurrency.max(1),
            image_update_debug,
            image_update_autocheck,

            session_msgs: Vec::new(),
            messages_seen_len: 0,
            shell_msgs: ShellMessagesState {
                scroll: 0,
                hscroll: 0,
                return_view: ShellView::Dashboard,
            },
            log_dock_enabled,
            log_dock_height,

            keymap,
            keymap_parsed: HashMap::new(),
            keymap_defaults: HashMap::new(),

            templates_state: TemplatesState {
                dir: PathBuf::from("templates"),
                kind: TemplatesKind::Stacks,
                templates: Vec::new(),
                templates_selected: 0,
                templates_error: None,
                templates_details_scroll: 0,
                templates_refresh_after_edit: None,
                template_deploy_inflight: HashMap::new(),
                git_head: None,
                git_remote_templates: HashMap::new(),
                git_remote_net_templates: HashMap::new(),
                dirty_templates: HashSet::new(),
                untracked_templates: HashSet::new(),
                net_templates: Vec::new(),
                net_templates_selected: 0,
                net_templates_error: None,
                net_templates_details_scroll: 0,
                net_templates_refresh_after_edit: None,
                net_template_deploy_inflight: HashMap::new(),
                dirty_net_templates: HashSet::new(),
                untracked_net_templates: HashSet::new(),
                ai_edit_snapshot: None,
            },
            theme_refresh_after_edit: None,

            stacks: Vec::new(),
            stacks_selected: 0,
            stacks_details_scroll: 0,
            stacks_networks_scroll: 0,
            stack_details_focus: StackDetailsFocus::Containers,
            stacks_only_running: false,
            registries_selected: 0,
            registries_details_scroll: 0,
            image_updates,
            image_updates_inflight: HashSet::new(),
            image_updates_path,
            rate_limits,
            template_deploys,
            net_template_deploys,
            unknown_template_ids_warned: HashSet::new(),
            registries_cfg,
            registry_auths: HashMap::new(),
            registry_tests,

            container_details_scroll: 0,
            image_details_scroll: 0,
            volume_details_scroll: 0,
            network_details_scroll: 0,
            container_details_id: None,
            image_details_id: None,
            volume_details_id: None,
            network_details_id: None,
        };
        app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
        if app.servers.len() > 1 {
            app.shell_sidebar_selected = app.server_selected.saturating_add(1);
        } else {
            app.shell_sidebar_selected = app.server_selected;
        }
        app.rebuild_keymap();
        if let Some(mode) = app.get_view_split_mode(app.shell_view) {
            app.shell_split_mode = mode;
        }
        app.resolve_registry_auths();
        app
    }

    fn refresh_templates(&mut self) {
        self.templates_state.templates_error = None;
        self.templates_state.templates.clear();
        self.templates_state.templates_details_scroll = 0;
        self.templates_state.git_head =
            commands::git_cmd::git_head_short(&self.templates_state.dir);
        self.refresh_template_git_status();

        self.migrate_templates_layout_if_needed();

        let dir = self.stack_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.templates_state.templates_error =
                Some(format!("failed to create templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.templates_state.templates_error =
                    Some(format!("failed to read templates dir: {e}"));
                return;
            }
        };

        let mut out: Vec<TemplateEntry> = Vec::new();
        for ent in entries.flatten() {
            let path = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let compose_path = path.join("compose.yaml");
            let has_compose = compose_path.exists();
            let desc = if has_compose {
                extract_template_description(&compose_path).unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            let template_id = if has_compose {
                extract_template_id(&compose_path)
            } else {
                None
            };
            out.push(TemplateEntry {
                name,
                dir: path,
                compose_path,
                has_compose,
                desc,
                template_id,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.templates_state.templates = out;
        if self.templates_state.templates_selected >= self.templates_state.templates.len() {
            self.templates_state.templates_selected =
                self.templates_state.templates.len().saturating_sub(1);
        }
        for t in &self.templates_state.templates {
            let Some(id) = t.template_id.as_ref() else {
                continue;
            };
            if self.template_deploys.contains_key(id) {
                continue;
            }
            if let Some(list) = self.template_deploys.remove(&t.name) {
                self.template_deploys.insert(id.clone(), list);
            }
        }
        let known: HashSet<String> = self
            .templates_state
            .templates
            .iter()
            .filter_map(|t| t.template_id.clone())
            .collect();
        self.template_deploys.retain(|id, _| known.contains(id));
    }

    fn dashboard_image_enabled(&self) -> bool {
        if !self.kitty_graphics {
            return false;
        }
        self.dashboard_image
            .as_ref()
            .map(|state| state.enabled)
            .unwrap_or(false)
    }

    fn set_kitty_graphics(&mut self, enabled: bool) -> bool {
        if enabled {
            if self.ascii_only {
                return false;
            }
            if self.dashboard_image.is_none() {
                let picker = Picker::from_query_stdio().ok();
                if let Some(p) = picker {
                    self.dashboard_image = Some(init_dashboard_image(p, &self.theme));
                } else {
                    return false;
                }
            }
            self.kitty_graphics = true;
            self.reset_dashboard_image();
        } else {
            self.kitty_graphics = false;
            self.dashboard_image = None;
        }
        true
    }

    fn update_dashboard_image(&mut self, area: ratatui::layout::Rect) {
        let Some(state) = &mut self.dashboard_image else {
            return;
        };
        if !state.enabled {
            return;
        }
        let Some(snap) = self.dashboard.snap.as_ref() else {
            return;
        };
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mem_ratio = if snap.mem_total_bytes == 0 {
            0.0
        } else {
            (snap.mem_used_bytes as f32) / (snap.mem_total_bytes as f32)
        };
        let cpu_ratio = if snap.cpu_cores == 0 {
            0.0
        } else {
            (snap.load1 / (snap.cpu_cores as f32)).clamp(0.0, 1.0)
        };
        let mut ratios: Vec<f32> = Vec::new();
        ratios.push(cpu_ratio);
        ratios.push(mem_ratio);
        if snap.disks.is_empty() {
            let disk_ratio = if snap.disk_total_bytes == 0 {
                0.0
            } else {
                (snap.disk_used_bytes as f32) / (snap.disk_total_bytes as f32)
            };
            ratios.push(disk_ratio);
        } else {
            for disk in &snap.disks {
                let total = disk.total_bytes.max(1) as f32;
                ratios.push((disk.used_bytes as f32) / total);
            }
        }
        let key = format!(
            "{:.2}-{:.2}-{}-{}x{}",
            cpu_ratio,
            mem_ratio,
            ratios.len(),
            area.width,
            area.height
        );
        if state.last_key.as_deref() == Some(&key) {
            return;
        }
        let (fw, fh) = state.picker.font_size();
        let px_w = (area.width as u32).saturating_mul(fw.max(1) as u32);
        let px_h = (area.height as u32).saturating_mul(fh.max(1) as u32);
        if px_w == 0 || px_h == 0 {
            return;
        }
        let img = build_dashboard_image(&self.theme, &ratios, px_w, px_h);
        let dyn_img = DynamicImage::ImageRgba8(img);
        state.protocol = Some(state.picker.new_resize_protocol(dyn_img));
        state.last_key = Some(key);
    }

    fn reset_dashboard_image(&mut self) {
        if let Some(state) = &mut self.dashboard_image {
            apply_dashboard_theme(state, &self.theme);
            state.protocol = None;
            state.last_key = None;
        }
    }













    fn switch_server(
        &mut self,
        idx: usize,
        conn_tx: &watch::Sender<Connection>,
        refresh_tx: &mpsc::UnboundedSender<()>,
        dash_refresh_tx: &mpsc::UnboundedSender<()>,
        dash_all_enabled_tx: &watch::Sender<bool>,
    ) {
        let Some(s) = self.servers.get(idx).cloned() else {
            return;
        };
        self.server_selected = idx;
        self.server_all_selected = false;
        self.active_server = Some(s.name.clone());
        self.clear_all_marks();
        self.action_inflight.clear();
        self.image_action_inflight.clear();
        self.volume_action_inflight.clear();
        self.network_action_inflight.clear();
        self.stack_update_inflight.clear();
        self.stack_update_error.clear();
        self.stack_update_containers.clear();

        let runner = if s.target == "local" {
            Runner::Local
        } else {
            Runner::Ssh(Ssh {
                target: s.target.clone(),
                identity: s.identity.clone(),
                port: s.port,
            })
        };
        self.current_target = runner.key();
        self.clear_conn_error();
        self.start_loading(true);
        self.dashboard.loading = true;
        self.dashboard.error = None;
        self.dashboard.snap = None;
        self.reset_dashboard_image();
        self.dashboard.last_disk_count = self
            .dashboard
            .snap
            .as_ref()
            .map(|s| s.disks.len())
            .unwrap_or(0);
        let _ = conn_tx.send(Connection {
            runner,
            docker: DockerCfg {
                docker_cmd: s.docker_cmd,
            },
        });
        let _ = dash_all_enabled_tx.send(false);

        // Persist last_server only; no secrets stored.
        self.persist_config();
        let _ = refresh_tx.send(());
        let _ = dash_refresh_tx.send(());

        self.set_main_view(ShellView::Dashboard);
        shell_sidebar_select_item(self, ShellSidebarItem::Server(idx));
    }

    fn switch_server_all(
        &mut self,
        dash_all_enabled_tx: &watch::Sender<bool>,
        dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    ) {
        if self.servers.len() <= 1 {
            return;
        }
        self.server_all_selected = true;
        self.active_server = None;
        self.current_target.clear();
        self.clear_conn_error();
        self.dashboard.loading = false;
        let mut hosts: Vec<DashboardHostState> = Vec::new();
        for s in &self.servers {
            if let Some(existing) = self.dashboard_all.hosts.iter().find(|h| h.name == s.name) {
                let mut h = existing.clone();
                h.loading = true;
                h.error = None;
                hosts.push(h);
            } else {
                hosts.push(DashboardHostState {
                    name: s.name.clone(),
                    loading: true,
                    error: None,
                    snap: None,
                    latency_ms: None,
                });
            }
        }
        self.dashboard_all.hosts = hosts;
        let _ = dash_all_enabled_tx.send(true);
        let _ = dash_all_refresh_tx.send(());
        self.set_main_view(ShellView::Dashboard);
    }

    fn selected_template(&self) -> Option<&TemplateEntry> {
        self.templates_state
            .templates
            .get(self.templates_state.templates_selected)
    }

    fn net_templates_dir(&self) -> PathBuf {
        self.templates_state.dir.join("networks")
    }

    fn stack_templates_dir(&self) -> PathBuf {
        self.templates_state.dir.join("stacks")
    }

    fn migrate_templates_layout_if_needed(&mut self) {
        // Old layout: <templates_dir>/<name>/compose.yaml and <templates_dir>/networks/...
        // New layout: <templates_dir>/stacks/<name>/compose.yaml and <templates_dir>/networks/...
        let stacks = self.stack_templates_dir();
        if stacks.exists() {
            return;
        }
        let root = self.templates_state.dir.clone();
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut to_move: Vec<(String, PathBuf)> = Vec::new();
        for ent in entries.flatten() {
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "networks" || name == "stacks" {
                continue;
            }
            to_move.push((name, ent.path()));
        }
        if to_move.is_empty() {
            return;
        }
        if let Err(e) = fs::create_dir_all(&stacks) {
            self.log_msg(
                MsgLevel::Warn,
                format!(
                    "failed to create stacks templates dir '{}': {e}",
                    stacks.display()
                ),
            );
            return;
        }
        for (name, from) in to_move {
            let to = stacks.join(&name);
            if to.exists() {
                self.log_msg(
                    MsgLevel::Warn,
                    format!(
                        "template migration skipped: '{}' already exists in stacks/",
                        name
                    ),
                );
                continue;
            }
            if let Err(e) = fs::rename(&from, &to) {
                self.log_msg(
                    MsgLevel::Warn,
                    format!("template migration failed for '{}': {}", name, e),
                );
            }
        }
    }

    fn refresh_net_templates(&mut self) {
        self.templates_state.net_templates_error = None;
        self.templates_state.net_templates.clear();
        self.templates_state.net_templates_details_scroll = 0;
        self.refresh_template_git_status();

        self.migrate_templates_layout_if_needed();

        let dir = self.net_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.templates_state.net_templates_error =
                Some(format!("failed to create net templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.templates_state.net_templates_error =
                    Some(format!("failed to read net templates dir: {e}"));
                return;
            }
        };

        let mut out: Vec<NetTemplateEntry> = Vec::new();
        for ent in entries.flatten() {
            let path = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let cfg_path = path.join("network.json");
            let has_cfg = cfg_path.exists();
            let desc = if has_cfg {
                extract_net_template_description(&cfg_path).unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            out.push(NetTemplateEntry {
                name,
                dir: path,
                cfg_path,
                has_cfg,
                desc,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.templates_state.net_templates = out;
        if self.templates_state.net_templates_selected >= self.templates_state.net_templates.len() {
            self.templates_state.net_templates_selected =
                self.templates_state.net_templates.len().saturating_sub(1);
        }
    }

    fn refresh_template_git_status(&mut self) {
        self.templates_state.dirty_templates.clear();
        self.templates_state.dirty_net_templates.clear();
        self.templates_state.untracked_templates.clear();
        self.templates_state.untracked_net_templates.clear();
        self.templates_state.git_remote_templates.clear();
        self.templates_state.git_remote_net_templates.clear();
        let dir = self.templates_state.dir.clone();
        if !commands::git_cmd::is_git_repo(&dir) {
            return;
        }
        let out = match commands::git_cmd::run_git(&dir, &["status", "--porcelain", "-uall"]) {
            Ok(out) => out,
            Err(e) => {
                self.log_msg(MsgLevel::Warn, format!("git status failed: {:#}", e));
                return;
            }
        };
        for line in out.lines() {
            let untracked = line.starts_with("??");
            let path = parse_git_status_path(line);
            let Some(path) = path else { continue };
            if let Some(rest) = path.strip_prefix("stacks/") {
                if let Some(name) = rest.split('/').next() {
                    if !name.trim().is_empty() {
                        self.templates_state.dirty_templates.insert(name.to_string());
                        if untracked {
                            self.templates_state.untracked_templates.insert(name.to_string());
                        }
                    }
                }
            } else if let Some(rest) = path.strip_prefix("networks/") {
                if let Some(name) = rest.split('/').next() {
                    if !name.trim().is_empty() {
                        self.templates_state
                            .dirty_net_templates
                            .insert(name.to_string());
                        if untracked {
                            self.templates_state
                                .untracked_net_templates
                                .insert(name.to_string());
                        }
                    }
                }
            }
        }

        if commands::git_cmd::run_git(&dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).is_err() {
            return;
        }

        let stacks_dir = dir.join("stacks");
        if let Ok(entries) = fs::read_dir(&stacks_dir) {
            for ent in entries.flatten() {
                let Ok(ft) = ent.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let rel = format!("stacks/{name}");
                let status = git_remote_status_for_path(&dir, &rel);
                self.templates_state
                    .git_remote_templates
                    .insert(name, status);
            }
        }

        let nets_dir = dir.join("networks");
        if let Ok(entries) = fs::read_dir(&nets_dir) {
            for ent in entries.flatten() {
                let Ok(ft) = ent.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let rel = format!("networks/{name}");
                let status = git_remote_status_for_path(&dir, &rel);
                self.templates_state
                    .git_remote_net_templates
                    .insert(name, status);
            }
        }
    }

    fn selected_net_template(&self) -> Option<&NetTemplateEntry> {
        self.templates_state
            .net_templates
            .get(self.templates_state.net_templates_selected)
    }

    fn capture_template_ai_snapshot(&mut self, kind: TemplatesKind, name: String, path: PathBuf) {
        let hash = file_content_hash(&path);
        self.templates_state.ai_edit_snapshot = Some(TemplateEditSnapshot {
            kind,
            name,
            path,
            hash,
        });
    }

    fn apply_template_ai_snapshot_if_kind(&mut self, kind: TemplatesKind) {
        let Some(snapshot) = self.templates_state.ai_edit_snapshot.as_ref() else {
            return;
        };
        if snapshot.kind != kind {
            return;
        }
        let snapshot = self.templates_state.ai_edit_snapshot.take().unwrap();
        if commands::git_cmd::is_git_repo(&self.templates_state.dir) {
            return;
        }
        let next_hash = file_content_hash(&snapshot.path);
        if next_hash != snapshot.hash {
            match snapshot.kind {
                TemplatesKind::Stacks => {
                    self.templates_state.dirty_templates.insert(snapshot.name);
                }
                TemplatesKind::Networks => {
                    self.templates_state
                        .dirty_net_templates
                        .insert(snapshot.name);
                }
            }
        }
    }





    fn rebuild_keymap(&mut self) {
        self.keymap_parsed.clear();
        self.keymap_defaults = build_default_keymap();
        let mut invalid: Vec<(String, String)> = Vec::new();
        for kb in &self.keymap {
            let cmd = kb.cmd.trim();
            let scope = match parse_scope(&kb.scope) {
                Some(s) => s,
                None => {
                    invalid.push((kb.scope.clone(), "invalid scope".to_string()));
                    continue;
                }
            };
            match parse_key_spec(&kb.key) {
                Ok(spec) => {
                    if is_single_letter_without_modifiers(spec) && cmdline_is_destructive(cmd) {
                        invalid.push((
                            format!("{} {}", kb.scope.trim(), kb.key.trim()),
                            "refusing to bind destructive command to unmodified single letter"
                                .to_string(),
                        ));
                        continue;
                    }
                    // Empty cmd means "disabled" for this key in this scope.
                    self.keymap_parsed
                        .insert((scope, spec), cmd.trim_start_matches(':').to_string());
                }
                Err(e) => {
                    invalid.push((kb.key.clone(), e));
                }
            }
        }
        for (key, err) in invalid {
            self.log_msg(
                MsgLevel::Warn,
                format!("invalid key binding '{key}': {err}"),
            );
        }
    }

}

#[derive(Clone, Debug)]
struct Connection {
    runner: Runner,
    docker: DockerCfg,
}

fn extract_container_ip(v: &Value) -> Option<String> {
    // Prefer user-defined networks.
    let ip = v
        .pointer("/NetworkSettings/Networks")
        .and_then(|n| n.as_object())
        .and_then(|map| {
            for (_name, net) in map {
                if let Some(ip) = net.get("IPAddress").and_then(|x| x.as_str()) {
                    let ip = ip.trim();
                    if !ip.is_empty() {
                        return Some(ip.to_string());
                    }
                }
            }
            None
        })
        .or_else(|| {
            v.pointer("/NetworkSettings/IPAddress")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    ip
}

#[derive(Debug, Clone, Default)]
struct UsageSnapshot {
    image_ref_count_by_id: HashMap<String, usize>,
    image_run_count_by_id: HashMap<String, usize>,
    image_containers_by_id: HashMap<String, Vec<String>>,
    volume_ref_count_by_name: HashMap<String, usize>,
    volume_run_count_by_name: HashMap<String, usize>,
    volume_containers_by_name: HashMap<String, Vec<String>>,
    network_ref_count_by_id: HashMap<String, usize>,
    network_containers_by_id: HashMap<String, Vec<String>>,
    ip_by_container_id: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct DashboardSnapshot {
    os: String,
    kernel: String,
    arch: String,
    uptime: String,
    engine: String,
    containers_running: u32,
    containers_total: u32,
    cpu_cores: u32,
    load1: f32,
    load5: f32,
    load15: f32,
    mem_used_bytes: u64,
    mem_total_bytes: u64,
    disk_used_bytes: u64,
    disk_total_bytes: u64,
    disks: Vec<DiskEntry>,
    nics: Vec<NicEntry>,
    collected_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
struct DashboardState {
    loading: bool,
    error: Option<String>,
    snap: Option<DashboardSnapshot>,
    last_disk_count: usize,
    suppress_image_frames: u8,
}

#[derive(Clone, Debug)]
struct DashboardHostState {
    name: String,
    loading: bool,
    error: Option<String>,
    snap: Option<DashboardSnapshot>,
    latency_ms: Option<u128>,
}

#[derive(Clone, Debug, Default)]
struct DashboardAllState {
    hosts: Vec<DashboardHostState>,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            loading: false,
            error: None,
            snap: None,
            last_disk_count: 0,
            suppress_image_frames: 0,
        }
    }
}

struct DashboardImageState {
    enabled: bool,
    picker: Picker,
    protocol: Option<StatefulProtocol>,
    last_key: Option<String>,
}

fn init_dashboard_image(mut picker: Picker, theme: &theme::ThemeSpec) -> DashboardImageState {
    let fallback = Rgba([16, 16, 16, 255]);
    let panel_raw = theme.panel.bg.trim();
    let panel_bg = theme_color_rgba(&theme.panel.bg, fallback);
    let bg = if panel_raw.eq_ignore_ascii_case("default") || panel_raw.eq_ignore_ascii_case("reset") {
        theme_color_rgba(&theme.background.bg, fallback)
    } else {
        panel_bg
    };
    picker.set_background_color(bg);
    let enabled = picker.protocol_type() == ProtocolType::Kitty;
    DashboardImageState {
        enabled,
        picker,
        protocol: None,
        last_key: None,
    }
}

fn apply_dashboard_theme(state: &mut DashboardImageState, theme: &theme::ThemeSpec) {
    let fallback = Rgba([16, 16, 16, 255]);
    let panel_raw = theme.panel.bg.trim();
    let panel_bg = theme_color_rgba(&theme.panel.bg, fallback);
    let bg = if panel_raw.eq_ignore_ascii_case("default") || panel_raw.eq_ignore_ascii_case("reset") {
        theme_color_rgba(&theme.background.bg, fallback)
    } else {
        panel_bg
    };
    state.picker.set_background_color(bg);
    state.enabled = state.picker.protocol_type() == ProtocolType::Kitty;
    state.protocol = None;
    state.last_key = None;
}

#[derive(Clone, Debug)]
struct DiskEntry {
    source: String,
    fs_type: String,
    mount: String,
    used_bytes: u64,
    total_bytes: u64,
}

#[derive(Clone, Debug)]
struct NicEntry {
    name: String,
    addr: String,
}

fn build_dashboard_image(
    theme: &theme::ThemeSpec,
    ratios: &[f32],
    width: u32,
    height: u32,
) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    let fallback_bg = Rgba([16, 16, 16, 255]);
    let panel_raw = theme.panel.bg.trim();
    let bg = if panel_raw.eq_ignore_ascii_case("default") || panel_raw.eq_ignore_ascii_case("reset") {
        theme_color_rgba(&theme.background.bg, fallback_bg)
    } else {
        theme_color_rgba(&theme.panel.bg, fallback_bg)
    };
    let faint = theme_color_rgba(&theme.header.bg, Rgba([40, 40, 40, 255]));
    let ok = theme_color_rgba(&theme.text_ok.fg, Rgba([90, 200, 120, 255]));
    let warn = theme_color_rgba(&theme.text_warn.fg, Rgba([255, 190, 64, 255]));
    let err = theme_color_rgba(&theme.text_error.fg, Rgba([220, 120, 120, 255]));

    for p in img.pixels_mut() {
        *p = bg;
    }

    let mut fill_rect = |x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>| {
        let max_x = width.saturating_sub(1);
        let max_y = height.saturating_sub(1);
        let end_x = (x + w).min(width);
        let end_y = (y + h).min(height);
        for yy in y..end_y {
            if yy > max_y {
                break;
            }
            for xx in x..end_x {
                if xx > max_x {
                    break;
                }
                img.put_pixel(xx, yy, color);
            }
        }
    };

    let lerp = |a: u8, b: u8, t: f32| -> u8 {
        let t = t.clamp(0.0, 1.0);
        (a as f32 + (b as f32 - a as f32) * t).round() as u8
    };
    let lerp_rgba = |a: Rgba<u8>, b: Rgba<u8>, t: f32| -> Rgba<u8> {
        Rgba([
            lerp(a[0], b[0], t),
            lerp(a[1], b[1], t),
            lerp(a[2], b[2], t),
            255,
        ])
    };

    let ratios: Vec<f32> = ratios.iter().map(|r| r.clamp(0.0, 1.0)).collect();
    if ratios.is_empty() {
        return img;
    }
    let margin_x = 2u32;
    let bar_w = width.saturating_sub(margin_x * 2);
    let rows = ratios.len().max(1) as u32;
    let row_h = (height / rows).max(1);
    let pad = (row_h / 6).min(2);
    let bar_h = row_h.saturating_sub(pad * 2).max(3);
    for (idx, ratio) in ratios.iter().enumerate() {
        let row_top = idx as u32 * row_h;
        let y = row_top + (row_h.saturating_sub(bar_h)) / 2;
        fill_rect(margin_x, y, bar_w, bar_h, faint);
        let fill_w = ((bar_w as f32) * ratio).round() as u32;
        for xx in 0..fill_w {
            let t = if bar_w <= 1 { 1.0 } else { (xx as f32) / (bar_w as f32 - 1.0) };
            let color = if t <= 0.7 {
                lerp_rgba(ok, warn, t / 0.7)
            } else {
                lerp_rgba(warn, err, (t - 0.7) / 0.3)
            };
            fill_rect(margin_x + xx, y, 1, bar_h, color);
        }
    }

    img
}

fn dashboard_command(docker_cmd: &DockerCmd) -> String {
    if docker_cmd.is_empty() {
        return String::new();
    }
    // Single round-trip via SSH/Local runner to collect basic host metrics.
    // Keep dependencies minimal: rely on /proc and coreutils if present.
    const OS: &str = "__CONTAINR_DASH_OS__";
    const KERNEL: &str = "__CONTAINR_DASH_KERNEL__";
    const ARCH: &str = "__CONTAINR_DASH_ARCH__";
    const UPTIME: &str = "__CONTAINR_DASH_UPTIME__";
    const CORES: &str = "__CONTAINR_DASH_CORES__";
    const LOAD: &str = "__CONTAINR_DASH_LOAD__";
    const MEM: &str = "__CONTAINR_DASH_MEM__";
    const DISK: &str = "__CONTAINR_DASH_DISK__";
    const NICS: &str = "__CONTAINR_DASH_NICS__";
    const ENGINE: &str = "__CONTAINR_DASH_ENGINE__";
    const CONTAINERS: &str = "__CONTAINR_DASH_CONTAINERS__";

    let docker_fmt = "{{{{.Server.Version}}}}|{{{{.Server.Os}}}}|{{{{.Server.Arch}}}}|{{{{.Server.ApiVersion}}}}";
    let dc = docker_cmd.to_shell();
    format!(
        "uname_s=$(uname -s 2>/dev/null || echo unknown); \
         echo {OS}; \
         if [ -r /etc/os-release ]; then . /etc/os-release && echo \"$PRETTY_NAME\"; \
         elif [ \"$uname_s\" = Darwin ]; then sw_vers -productName 2>/dev/null | tr -d '\\n'; echo \" $(sw_vers -productVersion 2>/dev/null)\"; \
         else uname -s 2>/dev/null; fi; \
         echo {KERNEL}; uname -r 2>/dev/null || true; \
         echo {ARCH}; uname -m 2>/dev/null || true; \
         echo {UPTIME}; ( uptime -p 2>/dev/null || uptime 2>/dev/null || cat /proc/uptime 2>/dev/null || true ); \
         echo {CORES}; ( nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || grep -c '^processor' /proc/cpuinfo 2>/dev/null || echo 1 ); \
         echo {LOAD}; ( cat /proc/loadavg 2>/dev/null || sysctl -n vm.loadavg 2>/dev/null | tr -d '{{}}' || uptime 2>/dev/null || true ); \
         echo {MEM}; ( \
           if [ -r /proc/meminfo ]; then cat /proc/meminfo 2>/dev/null; \
           elif [ \"$uname_s\" = Darwin ]; then \
             total=$(sysctl -n hw.memsize 2>/dev/null || echo 0); \
             pagesize=$(sysctl -n hw.pagesize 2>/dev/null || echo 4096); \
             vm=$(vm_stat 2>/dev/null); \
             free=$(echo \"$vm\" | awk '/Pages free/ {{print $3}}' | tr -d '.'); \
             inactive=$(echo \"$vm\" | awk '/Pages inactive/ {{print $3}}' | tr -d '.'); \
             speculative=$(echo \"$vm\" | awk '/Pages speculative/ {{print $3}}' | tr -d '.'); \
             avail_pages=$((free+inactive+speculative)); \
             avail=$((avail_pages*pagesize)); \
             used=$((total-avail)); \
             echo \"MEM_TOTAL=$total MEM_AVAIL=$avail MEM_USED=$used\"; \
           fi ); \
         echo {DISK}; ( df -B1 -P -T 2>/dev/null || df -k -P 2>/dev/null || true ); \
         echo {NICS}; ( \
           if [ -d /sys/class/net ]; then \
             for i in /sys/class/net/*; do \
               iface=$(basename \"$i\"); \
               [ -e \"$i/device\" ] || continue; \
               case \"$iface\" in \
                 lo|br*|bond*|team*|vlan*|veth*|docker*|virbr*|cni*|flannel*|kube*|tap*|tun*) continue ;; \
               esac; \
               ip -o -4 addr show dev \"$iface\" 2>/dev/null | awk '{{print $2, $4}}'; \
             done; \
           elif [ \"$uname_s\" = Darwin ]; then \
             networksetup -listallhardwareports 2>/dev/null | awk '/Device:/ {{print $2}}' | while read -r dev; do \
               ip=$(ipconfig getifaddr \"$dev\" 2>/dev/null || true); \
               if [ -n \"$ip\" ]; then echo \"$dev $ip\"; fi; \
             done; \
           fi ); \
         echo {ENGINE}; ( {dc} version --format '{docker_fmt}' 2>/dev/null || {dc} --version 2>/dev/null || true ); \
         echo {CONTAINERS}; ( {dc} ps -q 2>/dev/null | wc -l | tr -d ' ' ); ( {dc} ps -a -q 2>/dev/null | wc -l | tr -d ' ' )",
        OS = OS,
        KERNEL = KERNEL,
        ARCH = ARCH,
        UPTIME = UPTIME,
        CORES = CORES,
        LOAD = LOAD,
        MEM = MEM,
        DISK = DISK,
        NICS = NICS,
        ENGINE = ENGINE,
        CONTAINERS = CONTAINERS,
        dc = dc,
        docker_fmt = docker_fmt,
    )
}

fn format_uptime_from_proc(raw: &str) -> Option<String> {
    // /proc/uptime: "<seconds> <idle_seconds>"
    let secs = raw.split_whitespace().next()?.parse::<f64>().ok()?;
    let mut secs = secs.max(0.0).round() as u64;
    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3600;
    secs %= 3600;
    let minutes = secs / 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    parts.push(format!("{minutes}m"));
    Some(parts.join(" "))
}

fn normalize_uptime_line(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return "-".to_string();
    }

    // BSD/macOS uptime often looks like:
    // "14:03  up 5 days,  3:02, 3 users, load averages: 1.11 1.08 1.05"
    // Keep only the actual uptime segment.
    if !s.starts_with("up ") {
        if let Some((_, rest)) = s.split_once(" up ") {
            s = format!("up {}", rest.trim());
        }
    }

    if let Some((left, _)) = s.split_once(", load average") {
        s = left.trim().to_string();
    }
    if let Some((left, _)) = s.split_once(", load averages") {
        s = left.trim().to_string();
    }

    // Remove trailing user count (", 3 users" / ", 1 user").
    if let Some(idx) = s.rfind(",") {
        let tail = s[idx + 1..].trim();
        let mut it = tail.split_whitespace();
        if let (Some(n), Some(u)) = (it.next(), it.next()) {
            if n.chars().all(|c| c.is_ascii_digit()) && (u == "user" || u == "users") {
                s = s[..idx].trim().to_string();
            }
        }
    }

    // Normalize shorthand clock-style uptime to Linux-like wording:
    // "up 6:15" -> "up 6 hours, 15 minutes"
    // "up 5 days, 6:15" -> "up 5 days, 6 hours, 15 minutes"
    let mut out_parts: Vec<String> = Vec::new();
    let core = s.strip_prefix("up ").unwrap_or(&s).trim();
    for part in core.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        if let Some((h, m)) = part.split_once(':') {
            let h_ok = h.chars().all(|c| c.is_ascii_digit());
            let m_ok = m.chars().all(|c| c.is_ascii_digit());
            if h_ok && m_ok {
                let hours = h.parse::<u32>().unwrap_or(0);
                let mins = m.parse::<u32>().unwrap_or(0);
                if hours > 0 {
                    let unit = if hours == 1 { "hour" } else { "hours" };
                    out_parts.push(format!("{hours} {unit}"));
                }
                let unit = if mins == 1 { "minute" } else { "minutes" };
                out_parts.push(format!("{mins} {unit}"));
                continue;
            }
        }
        out_parts.push(part.to_string());
    }

    if out_parts.is_empty() {
        s
    } else {
        format!("up {}", out_parts.join(", "))
    }
}

fn parse_dashboard_output(out: &str) -> anyhow::Result<DashboardSnapshot> {
    const OS: &str = "__CONTAINR_DASH_OS__";
    const KERNEL: &str = "__CONTAINR_DASH_KERNEL__";
    const ARCH: &str = "__CONTAINR_DASH_ARCH__";
    const UPTIME: &str = "__CONTAINR_DASH_UPTIME__";
    const CORES: &str = "__CONTAINR_DASH_CORES__";
    const LOAD: &str = "__CONTAINR_DASH_LOAD__";
    const MEM: &str = "__CONTAINR_DASH_MEM__";
    const DISK: &str = "__CONTAINR_DASH_DISK__";
    const NICS: &str = "__CONTAINR_DASH_NICS__";
    const ENGINE: &str = "__CONTAINR_DASH_ENGINE__";
    const CONTAINERS: &str = "__CONTAINR_DASH_CONTAINERS__";

    let mut cur: Option<&'static str> = None;
    let mut sec: HashMap<&'static str, Vec<String>> = HashMap::new();
    for line in out.lines() {
        let t = line.trim_end_matches('\r');
        cur = match t.trim() {
            OS => Some(OS),
            KERNEL => Some(KERNEL),
            ARCH => Some(ARCH),
            UPTIME => Some(UPTIME),
            CORES => Some(CORES),
            LOAD => Some(LOAD),
            MEM => Some(MEM),
            DISK => Some(DISK),
            NICS => Some(NICS),
            ENGINE => Some(ENGINE),
            CONTAINERS => Some(CONTAINERS),
            _ => cur,
        };
        if matches!(
            t.trim(),
            OS | KERNEL | ARCH | UPTIME | CORES | LOAD | MEM | DISK | NICS | ENGINE | CONTAINERS
        ) {
            if let Some(k) = cur {
                sec.entry(k).or_default();
            }
            continue;
        }
        if let Some(k) = cur {
            sec.entry(k).or_default().push(t.to_string());
        }
    }

    let first = |k: &'static str| -> String {
        sec.get(k)
            .and_then(|xs| xs.iter().find(|s| !s.trim().is_empty()).cloned())
            .unwrap_or_else(|| "-".to_string())
    };

    let os = first(OS);
    let kernel = first(KERNEL);
    let arch = first(ARCH);

    let uptime_raw = first(UPTIME);
    let uptime = if uptime_raw.contains("up ") || uptime_raw.starts_with("up ") {
        normalize_uptime_line(&uptime_raw)
    } else if let Some(u) = format_uptime_from_proc(&uptime_raw) {
        u
    } else {
        normalize_uptime_line(&uptime_raw)
    };

    let cpu_cores = first(CORES).trim().parse::<u32>().unwrap_or(1).max(1);

    let load_raw = first(LOAD);
    let mut load1 = 0.0f32;
    let mut load5 = 0.0f32;
    let mut load15 = 0.0f32;
    if let Some(line) = sec
        .get(LOAD)
        .and_then(|xs| xs.iter().find(|s| !s.trim().is_empty()))
    {
        let cleaned = line.replace('{', "").replace('}', "");
        let toks: Vec<&str> = cleaned.split_whitespace().collect();
        if toks.len() >= 3 {
            load1 = toks[0].parse::<f32>().unwrap_or(0.0);
            load5 = toks[1].parse::<f32>().unwrap_or(0.0);
            load15 = toks[2].parse::<f32>().unwrap_or(0.0);
        }
    } else if let Some(line) = load_raw.split("load average:").nth(1) {
        let toks: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if toks.len() >= 3 {
            load1 = toks[0].parse::<f32>().unwrap_or(0.0);
            load5 = toks[1].parse::<f32>().unwrap_or(0.0);
            load15 = toks[2].parse::<f32>().unwrap_or(0.0);
        }
    }

    let mut mem_total_kb: Option<u64> = None;
    let mut mem_avail_kb: Option<u64> = None;
    let mut mem_total_bytes: Option<u64> = None;
    let mut mem_avail_bytes: Option<u64> = None;
    let mut mem_used_bytes: Option<u64> = None;
    if let Some(lines) = sec.get(MEM) {
        for l in lines {
            let l = l.trim();
            if l.contains("MEM_TOTAL=") {
                for part in l.split_whitespace() {
                    if let Some(rest) = part.strip_prefix("MEM_TOTAL=") {
                        mem_total_bytes = rest.parse::<u64>().ok();
                    }
                    if let Some(rest) = part.strip_prefix("MEM_AVAIL=") {
                        mem_avail_bytes = rest.parse::<u64>().ok();
                    }
                    if let Some(rest) = part.strip_prefix("MEM_USED=") {
                        mem_used_bytes = rest.parse::<u64>().ok();
                    }
                }
            }
            if let Some(rest) = l.strip_prefix("MemTotal:") {
                mem_total_kb = rest.split_whitespace().next().and_then(|x| x.parse().ok());
            }
            if let Some(rest) = l.strip_prefix("MemAvailable:") {
                mem_avail_kb = rest.split_whitespace().next().and_then(|x| x.parse().ok());
            }
            if (mem_total_kb.is_some() && mem_avail_kb.is_some())
                || (mem_total_bytes.is_some() && (mem_avail_bytes.is_some() || mem_used_bytes.is_some()))
            {
                break;
            }
        }
    }
    let mem_total_bytes = mem_total_bytes.unwrap_or_else(|| mem_total_kb.unwrap_or(0).saturating_mul(1024));
    let mem_avail_bytes = mem_avail_bytes.unwrap_or_else(|| mem_avail_kb.unwrap_or(0).saturating_mul(1024));
    let mem_used_bytes = mem_used_bytes.unwrap_or_else(|| mem_total_bytes.saturating_sub(mem_avail_bytes));

    let mut disk_entries: Vec<DiskEntry> = Vec::new();
    if let Some(lines) = sec.get(DISK) {
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with("Filesystem") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                continue;
            }
            let (source, fs_type, total_bytes, used_bytes, mount) = if parts.len() >= 7 {
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].parse::<u64>().unwrap_or(0),
                    parts[3].parse::<u64>().unwrap_or(0),
                    parts[6].to_string(),
                )
            } else {
                // `df -k -P` (e.g. macOS fallback) reports 1K blocks, not bytes.
                (
                    parts[0].to_string(),
                    String::new(),
                    parts[1].parse::<u64>().unwrap_or(0).saturating_mul(1024),
                    parts[2].parse::<u64>().unwrap_or(0).saturating_mul(1024),
                    parts[5].to_string(),
                )
            };
            disk_entries.push(DiskEntry {
                source,
                fs_type,
                mount,
                used_bytes,
                total_bytes,
            });
        }
    }

    let disk_entries = collapse_disks(filter_disk_entries(disk_entries));
    let mut disk_used_bytes = 0u64;
    let mut disk_total_bytes = 0u64;
    let is_macos = os.to_ascii_lowercase().contains("mac");
    if is_macos {
        if let Some(data) = disk_entries
            .iter()
            .find(|d| d.mount == "/System/Volumes/Data")
        {
            disk_used_bytes = data.used_bytes;
            disk_total_bytes = data.total_bytes;
        } else if let Some(root) = disk_entries.iter().find(|d| d.mount == "/") {
            disk_used_bytes = root.used_bytes;
            disk_total_bytes = root.total_bytes;
        } else if let Some(first) = disk_entries.first() {
            disk_used_bytes = first.used_bytes;
            disk_total_bytes = first.total_bytes;
        }
    } else if let Some(root) = disk_entries.iter().find(|d| d.mount == "/") {
        disk_used_bytes = root.used_bytes;
        disk_total_bytes = root.total_bytes;
    } else if let Some(first) = disk_entries.first() {
        disk_used_bytes = first.used_bytes;
        disk_total_bytes = first.total_bytes;
    }

    let engine_raw = first(ENGINE);
    let engine = engine_raw.trim().to_string();

    let containers_raw = first(CONTAINERS);
    let mut containers_running = 0u32;
    let mut containers_total = 0u32;
    if let Some(lines) = sec.get(CONTAINERS) {
        let mut nums: Vec<u32> = Vec::new();
        for l in lines {
            let t = l.trim();
            if t.is_empty() {
                continue;
            }
            if let Ok(v) = t.parse::<u32>() {
                nums.push(v);
            } else if t.contains('/') {
                let parts: Vec<&str> = t.split('/').collect();
                if parts.len() >= 2 {
                    containers_running = parts[0].trim().parse::<u32>().unwrap_or(0);
                    containers_total = parts[1].trim().parse::<u32>().unwrap_or(0);
                    nums.clear();
                    break;
                }
            }
        }
        if nums.len() >= 2 {
            containers_running = nums[0];
            containers_total = nums[1];
        }
    } else if containers_raw.contains('/') {
        let parts: Vec<&str> = containers_raw.split('/').collect();
        if parts.len() >= 2 {
            containers_running = parts[0].trim().parse::<u32>().unwrap_or(0);
            containers_total = parts[1].trim().parse::<u32>().unwrap_or(0);
        }
    }

    let mut nics: Vec<NicEntry> = Vec::new();
    if let Some(lines) = sec.get(NICS) {
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            let Some(addr) = parts.next() else {
                continue;
            };
            let addr = addr.split('/').next().unwrap_or(addr).to_string();
            nics.push(NicEntry {
                name: name.to_string(),
                addr,
            });
        }
    }
    nics.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(DashboardSnapshot {
        os,
        kernel,
        arch,
        uptime,
        engine,
        containers_running,
        containers_total,
        cpu_cores,
        load1,
        load5,
        load15,
        mem_used_bytes,
        mem_total_bytes,
        disk_used_bytes,
        disk_total_bytes,
        disks: disk_entries,
        nics,
        collected_at: now_local(),
    })
}

fn filter_disk_entries(mut entries: Vec<DiskEntry>) -> Vec<DiskEntry> {
    if entries.is_empty() {
        return entries;
    }

    let excluded_types = [
        "tmpfs",
        "devtmpfs",
        "udev",
        "overlay",
        "proc",
        "sysfs",
        "cgroup",
        "cgroup2",
        "squashfs",
        "autofs",
        "fusectl",
        // Network filesystems/shares should not be treated as local host disks.
        "nfs",
        "nfs4",
        "cifs",
        "smbfs",
        "sshfs",
        "fuse.sshfs",
        "fuse.glusterfs",
        "ceph",
        "ceph-fuse",
        "9p",
    ];

    entries.retain(|e| {
        let ty = e.fs_type.to_ascii_lowercase();
        let mount = e.mount.as_str();
        if excluded_types.iter().any(|t| ty == *t) {
            return false;
        }
        if mount.starts_with("/run") || mount.starts_with("/dev") || mount.starts_with("/sys") {
            return false;
        }
        if mount.starts_with("/proc") || mount.starts_with("/snap") {
            return false;
        }
        if mount.starts_with("/var/lib/docker/overlay2") {
            return false;
        }
        // Ignore /boot mounts per user request.
        if mount.starts_with("/boot") {
            return false;
        }
        true
    });

    entries.sort_by(|a, b| {
        let rank = |m: &str| -> u8 {
            if m == "/" {
                0
            } else if m.starts_with("/var/lib/docker") {
                1
            } else if m.starts_with("/data") {
                2
            } else if m.starts_with("/mnt") {
                3
            } else if m.starts_with("/srv") {
                4
            } else {
                5
            }
        };
        let ra = rank(&a.mount);
        let rb = rank(&b.mount);
        ra.cmp(&rb).then_with(|| a.mount.cmp(&b.mount))
    });

    entries
}

fn collapse_disks(mut entries: Vec<DiskEntry>) -> Vec<DiskEntry> {
    let has_zfs = entries.iter().any(|e| e.fs_type == "zfs");
    let has_btrfs = entries.iter().any(|e| e.fs_type == "btrfs");
    if !has_zfs && !has_btrfs {
        let mut selected: Vec<DiskEntry> = Vec::new();
        for e in &entries {
            let m = e.mount.as_str();
            if m == "/"
                || m == "/System/Volumes/Data"
                || m == "/var/lib/docker"
                || m.starts_with("/mnt/")
                || m.starts_with("/data/")
                || m.starts_with("/srv/")
            {
                selected.push(e.clone());
            }
        }
        if selected.is_empty() {
            selected = entries;
        }
        selected.truncate(5);
        return selected;
    }
    if entries.is_empty() {
        return entries;
    }

    let mut out: Vec<DiskEntry> = Vec::new();
    let mut other: Vec<DiskEntry> = Vec::new();
    let mut zfs: Vec<DiskEntry> = Vec::new();
    let mut btrfs: Vec<DiskEntry> = Vec::new();

    for e in entries.drain(..) {
        if e.fs_type == "zfs" {
            zfs.push(e);
        } else if e.fs_type == "btrfs" {
            btrfs.push(e);
        } else {
            other.push(e);
        }
    }

    if let Some(root) = other.iter().find(|e| e.mount == "/") {
        out.push(root.clone());
    }

    if !zfs.is_empty() {
        let mut pools: HashMap<String, (u64, u64)> = HashMap::new();
        for e in &zfs {
            let pool = e.source.split('/').next().unwrap_or(&e.source).to_string();
            let entry = pools.entry(pool).or_insert((0, 0));
            entry.0 = entry.0.max(e.total_bytes);
            entry.1 = entry.1.saturating_add(e.used_bytes);
        }
        let mut pool_rows: Vec<DiskEntry> = pools
            .into_iter()
            .map(|(pool, (total, used))| DiskEntry {
                source: pool,
                fs_type: "zfs".to_string(),
                mount: String::new(),
                used_bytes: used,
                total_bytes: total,
            })
            .collect();
        pool_rows.sort_by_key(|e| std::cmp::Reverse(e.total_bytes));
        out.extend(pool_rows);
    }

    if !btrfs.is_empty() {
        let mut max_total = 0u64;
        let mut max_used = 0u64;
        for e in &btrfs {
            max_total = max_total.max(e.total_bytes);
            max_used = max_used.max(e.used_bytes);
        }
        out.push(DiskEntry {
            source: "btrfs".to_string(),
            fs_type: "btrfs".to_string(),
            mount: String::new(),
            used_bytes: max_used,
            total_bytes: max_total,
        });
    }

    for e in other {
        let m = e.mount.as_str();
        if m == "/"
            || m == "/var/lib/docker"
            || m.starts_with("/mnt/")
            || m.starts_with("/data/")
            || m.starts_with("/srv/")
        {
            if !out
                .iter()
                .any(|x| x.mount == e.mount && !x.mount.is_empty())
            {
                out.push(e);
            }
        }
    }

    out.truncate(5);
    out
}

fn normalize_image_id(id: &str) -> String {
    let s = id.trim();
    if s.is_empty() {
        return "".to_string();
    }
    if s.starts_with("sha256:") {
        return s.to_string();
    }
    format!("sha256:{}", s)
}

pub(in crate::ui) fn template_id_from_labels(labels: &str) -> Option<String> {
    for part in labels.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k.trim() == "app.containr.template_id" {
            let value = v.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn template_commit_from_labels(labels: &str) -> Option<String> {
    for part in labels.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k.trim() == "app.containr.commit" {
            let value = v.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn add_label_mapping(map: &mut YamlMapping, key: &str, value: &str) {
    let k = YamlValue::String(key.to_string());
    let v = YamlValue::String(value.to_string());
    map.insert(k, v);
}

fn add_label_sequence(seq: &mut Vec<YamlValue>, key: &str, value: &str) {
    let needle = format!("{key}={value}");
    if seq
        .iter()
        .any(|v| v.as_str().map(|s| s == needle).unwrap_or(false))
    {
        return;
    }
    seq.push(YamlValue::String(needle));
}

fn inject_template_labels(
    value: &mut YamlValue,
    template_id: &str,
    template_commit: Option<&str>,
) -> anyhow::Result<()> {
    let obj = value
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("compose root is not a mapping"))?;
    for key in ["services", "networks", "volumes"] {
        let Some(section) = obj.get_mut(&YamlValue::String(key.to_string())) else {
            continue;
        };
        let Some(items) = section.as_mapping_mut() else {
            continue;
        };
        for (_, item) in items.iter_mut() {
            let Some(item_map) = item.as_mapping_mut() else {
                continue;
            };
            let label_key = YamlValue::String("labels".to_string());
            if let Some(labels) = item_map.get_mut(&label_key) {
                match labels {
                    YamlValue::Mapping(m) => {
                        add_label_mapping(m, "app.containr.template_id", template_id);
                        if let Some(commit) = template_commit {
                            add_label_mapping(m, "app.containr.commit", commit);
                        }
                    }
                    YamlValue::Sequence(seq) => {
                        add_label_sequence(seq, "app.containr.template_id", template_id);
                        if let Some(commit) = template_commit {
                            add_label_sequence(seq, "app.containr.commit", commit);
                        }
                    }
                    _ => {
                        let mut m = YamlMapping::new();
                        add_label_mapping(&mut m, "app.containr.template_id", template_id);
                        if let Some(commit) = template_commit {
                            add_label_mapping(&mut m, "app.containr.commit", commit);
                        }
                        *labels = YamlValue::Mapping(m);
                    }
                }
            } else {
                let mut m = YamlMapping::new();
                add_label_mapping(&mut m, "app.containr.template_id", template_id);
                if let Some(commit) = template_commit {
                    add_label_mapping(&mut m, "app.containr.commit", commit);
                }
                item_map.insert(label_key, YamlValue::Mapping(m));
            }
        }
    }
    Ok(())
}

fn load_age_identities(path: &Path) -> anyhow::Result<Vec<Box<dyn age::Identity>>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read age identity: {}", path.display()))?;
    let mut ids: Vec<Box<dyn age::Identity>> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with("AGE-SECRET-KEY-") {
            continue;
        }
        let id = x25519::Identity::from_str(line)
            .map_err(|_| anyhow::anyhow!("invalid age identity"))?;
        ids.push(Box::new(id));
    }
    anyhow::ensure!(!ids.is_empty(), "no age identities found");
    Ok(ids)
}

fn ensure_age_identity(path: &Path) -> anyhow::Result<x25519::Identity> {
    if path.exists() {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read age identity: {}", path.display()))?;
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !line.starts_with("AGE-SECRET-KEY-") {
                continue;
            }
            let id = x25519::Identity::from_str(line)
                .map_err(|_| anyhow::anyhow!("invalid age identity"))?;
            return Ok(id);
        }
        anyhow::bail!("no age identities found");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create identity dir: {}", parent.display()))?;
    }
    let id = x25519::Identity::generate();
    let id_line = id.to_string();
    let content = format!("# containr age identity\n{}\n", id_line.expose_secret());
    fs::write(path, content)
        .with_context(|| format!("failed to write age identity: {}", path.display()))?;
    Ok(id)
}

fn encrypt_age_secret(secret: &str, identity: &x25519::Identity) -> anyhow::Result<String> {
    let recipient = identity.to_public();
    let encryptor =
        Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| anyhow::anyhow!("failed to configure age recipient"))?;
    let mut out = Vec::new();
    let armor = ArmoredWriter::wrap_output(&mut out, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armor)?;
    writer.write_all(secret.as_bytes())?;
    let armor = writer.finish()?;
    let _ = armor.finish()?;
    let encoded = String::from_utf8(out).context("encrypted secret is not valid utf-8")?;
    Ok(encoded)
}

fn decrypt_age_secret(secret: &str, identities: &[Box<dyn age::Identity>]) -> anyhow::Result<String> {
    let data = secret.as_bytes();
    let reader: Box<dyn std::io::Read> = if secret.contains("BEGIN AGE ENCRYPTED FILE") {
        Box::new(age::armor::ArmoredReader::new(std::io::Cursor::new(data)))
    } else {
        Box::new(std::io::Cursor::new(data))
    };
    let decryptor = Decryptor::new(reader)?;
    let mut out = String::new();
    let mut r = decryptor.decrypt(identities.iter().map(|id| id.as_ref() as &dyn age::Identity))?;
    r.read_to_string(&mut out)?;
    Ok(out.trim().to_string())
}

fn is_local_registry_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    host == "localhost"
        || host.starts_with("localhost:")
        || host == "127.0.0.1"
        || host.starts_with("127.0.0.1:")
        || host == "::1"
        || host.starts_with("[::1]")
}

fn registry_api_base_url(host: &str) -> anyhow::Result<String> {
    let host = host.trim();
    anyhow::ensure!(!host.is_empty(), "registry host is empty");
    if host.starts_with("http://") || host.starts_with("https://") {
        let url = Url::parse(host).context("invalid registry url")?;
        let scheme = url.scheme();
        let host_str = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("registry url missing host"))?;
        let host_str = if host_str.eq_ignore_ascii_case("docker.io")
            || host_str.eq_ignore_ascii_case("index.docker.io")
        {
            "registry-1.docker.io"
        } else {
            host_str
        };
        let mut base = format!("{scheme}://{host_str}");
        if let Some(port) = url.port() {
            base.push_str(&format!(":{port}"));
        }
        return Ok(base);
    }
    let host_norm = if host.eq_ignore_ascii_case("docker.io")
        || host.eq_ignore_ascii_case("index.docker.io")
    {
        "registry-1.docker.io".to_string()
    } else {
        host.to_string()
    };
    let scheme = if is_local_registry_host(host) {
        "http"
    } else {
        "https"
    };
    Ok(format!("{scheme}://{host_norm}"))
}

fn parse_www_authenticate_params(value: &str, scheme: &str) -> Option<HashMap<String, String>> {
    let value_trim = value.trim();
    let scheme_lc = scheme.to_ascii_lowercase();
    let value_lc = value_trim.to_ascii_lowercase();
    let prefix = format!("{scheme_lc} ");
    let pos = value_lc.find(&prefix)?;
    let params_str = &value_trim[pos + prefix.len()..];
    let mut params: HashMap<String, String> = HashMap::new();
    for part in params_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.splitn(2, '=');
        let key = it.next()?.trim();
        let val = it.next().unwrap_or("").trim().trim_matches('"');
        if !key.is_empty() {
            params.insert(key.to_string(), val.to_string());
        }
    }
    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

async fn registry_fetch_token(
    client: &Client,
    realm: &str,
    service: Option<&str>,
    scope: Option<&str>,
    basic: Option<(&str, &str)>,
) -> anyhow::Result<String> {
    let mut url = Url::parse(realm).context("invalid token realm url")?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(service) = service {
            if !service.trim().is_empty() {
                pairs.append_pair("service", service);
            }
        }
        if let Some(scope) = scope {
            if !scope.trim().is_empty() {
                pairs.append_pair("scope", scope);
            }
        }
    }
    let mut req = client.get(url);
    if let Some((user, pass)) = basic {
        req = req.basic_auth(user, Some(pass));
    }
    let resp = req.send().await.context("token request failed")?;
    if !resp.status().is_success() {
        anyhow::bail!("token request failed: http {}", resp.status());
    }
    let body = resp.text().await.context("invalid token response")?;
    let value: Value = serde_json::from_str(&body).context("invalid token response")?;
    if let Some(token) = value
        .get("token")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("access_token").and_then(|v| v.as_str()))
    {
        return Ok(token.to_string());
    }
    anyhow::bail!("token response missing token");
}

fn token_context(realm: &str, service: Option<&str>, scope: Option<&str>) -> String {
    let service = service.unwrap_or("-");
    let scope = scope.unwrap_or("-");
    format!("realm={realm} service={service} scope={scope}")
}

fn normalize_test_repo(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches('/');
    let raw = raw.split('@').next().unwrap_or(raw);
    let raw = raw.split(':').next().unwrap_or(raw);
    raw.to_string()
}

async fn registry_test(
    host: &str,
    auth: &RegistryAuthResolved,
    test_repo: Option<&str>,
) -> anyhow::Result<String> {
    let base = registry_api_base_url(host)?;
    let repo = test_repo
        .map(normalize_test_repo)
        .filter(|v| !v.is_empty());
    let url = format!("{base}/v2/");
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build http client")?;
    let host_lc = host.trim().to_ascii_lowercase();
    if host_lc == "ghcr.io" && matches!(auth.auth, config::RegistryAuth::Anonymous) && repo.is_none()
    {
        anyhow::bail!("ghcr.io anonymous test requires test-repo");
    }

    let mut request = client.get(&url);
    match auth.auth {
        config::RegistryAuth::BearerToken => {
            if let Some(token) = auth.secret.as_deref() {
                request = request.bearer_auth(token);
            }
        }
        _ => {}
    }
    let resp = request
        .send()
        .await
        .context("registry request failed")?;
    if resp.status().is_success() {
        return Ok(format!("ok ({})", resp.status()));
    }
    if resp.status() != StatusCode::UNAUTHORIZED {
        anyhow::bail!("registry request failed: http {}", resp.status());
    }

    let auth_header = resp
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if parse_www_authenticate_params(&auth_header, "basic").is_some() {
        let (user, pass) = match auth.auth {
            config::RegistryAuth::Basic | config::RegistryAuth::GithubPat => {
                let user = auth.username.as_deref().unwrap_or("");
                let pass = auth.secret.as_deref().unwrap_or("");
                if user.is_empty() || pass.is_empty() {
                    anyhow::bail!("registry credentials missing for {host}");
                }
                (user, pass)
            }
            _ => anyhow::bail!("registry requires basic auth"),
        };
        let resp = client
            .get(&url)
            .basic_auth(user, Some(pass))
            .send()
            .await
            .context("registry basic auth request failed")?;
        if resp.status().is_success() {
            return Ok(format!("ok ({})", resp.status()));
        }
        anyhow::bail!("registry basic auth failed: http {}", resp.status());
    }

    let params = parse_www_authenticate_params(&auth_header, "bearer")
        .ok_or_else(|| anyhow::anyhow!("registry auth challenge missing bearer details"))?;
    let realm = params
        .get("realm")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("registry auth challenge missing realm"))?;
    let service = params.get("service").cloned();
    let mut scope = params.get("scope").cloned();
    if let Some(repo) = repo.as_deref() {
        scope = Some(format!("repository:{repo}:pull"));
    }
    let service = service.as_deref();
    let scope = scope.as_deref();
    let basic = match auth.auth {
        config::RegistryAuth::Anonymous => None,
        config::RegistryAuth::Basic | config::RegistryAuth::GithubPat => {
            let user = auth.username.as_deref().unwrap_or("");
            let pass = auth.secret.as_deref().unwrap_or("");
            if user.is_empty() || pass.is_empty() {
                anyhow::bail!("registry credentials missing for {host}");
            }
            Some((user, pass))
        }
        config::RegistryAuth::BearerToken => None,
    };
    let mut used_ghcr = false;
    let token = match auth.auth {
        config::RegistryAuth::BearerToken => auth
            .secret
            .clone()
            .ok_or_else(|| anyhow::anyhow!("registry token missing for {host}"))?,
        _ => match registry_fetch_token(&client, &realm, service, scope, basic).await {
            Ok(token) => token,
            Err(e) => {
                if host_lc == "lscr.io" {
                    let repo = repo
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("lscr.io test requires test-repo"))?;
                    let ghcr_realm = "https://ghcr.io/token";
                    let ghcr_scope = format!("repository:{repo}:pull");
                    let ghcr_service = "ghcr.io";
                    match registry_fetch_token(
                        &client,
                        ghcr_realm,
                        Some(ghcr_service),
                        Some(&ghcr_scope),
                        basic,
                    )
                    .await
                    {
                        Ok(token) => {
                            used_ghcr = true;
                            token
                        }
                        Err(e2) => {
                            let ctx = token_context(
                                ghcr_realm,
                                Some(ghcr_service),
                                Some(&ghcr_scope),
                            );
                            anyhow::bail!("token request failed: {:#} ({ctx})", e2);
                        }
                    }
                } else {
                    let ctx = token_context(&realm, service, scope);
                    anyhow::bail!("token request failed: {:#} ({ctx})", e);
                }
            }
        },
    };
    let test_base = if used_ghcr {
        "https://ghcr.io".to_string()
    } else {
        base
    };
    let test_url = if let Some(repo) = repo.as_deref() {
        format!("{test_base}/v2/{repo}/tags/list")
    } else {
        url.clone()
    };
    let resp = client
        .get(&test_url)
        .bearer_auth(token)
        .send()
        .await
        .context("registry bearer auth request failed")?;
    if resp.status().is_success() {
        let hint = if used_ghcr { " via ghcr.io" } else { "" };
        return Ok(format!("ok ({}){hint}", resp.status()));
    }
    if resp.status() == StatusCode::NOT_FOUND && repo.is_some() {
        anyhow::bail!("registry repository not found (check test-repo)");
    }
    anyhow::bail!("registry bearer auth failed: http {}", resp.status());
}

fn render_compose_with_template_id(
    path: &Path,
    template_id: &str,
    template_commit: Option<&str>,
) -> anyhow::Result<tempfile::TempPath> {
    let data = fs::read_to_string(path)?;
    let mut yaml: YamlValue =
        serde_yaml::from_str(&data).map_err(|e| anyhow::anyhow!("compose parse failed: {}", e))?;
    inject_template_labels(&mut yaml, template_id, template_commit)?;
    let rendered =
        serde_yaml::to_string(&yaml).map_err(|e| anyhow::anyhow!("compose render failed: {}", e))?;
    let mut tmp = tempfile::Builder::new()
        .prefix("containr-compose-")
        .suffix(".yaml")
        .tempfile()?;
    tmp.write_all(rendered.as_bytes())?;
    Ok(tmp.into_temp_path())
}

pub async fn run_tui(
    runner: Runner,
    cfg: DockerCfg,
    refresh: Duration,
    logs_tail: usize,
    cmd_history_max: usize,
    cmd_history: Vec<String>,
    templates_dir: String,
    view_layout: HashMap<String, String>,
    active_theme: String,
    servers: Vec<ServerEntry>,
    keymap: Vec<KeyBinding>,
    active_server: Option<String>,
    config_path: std::path::PathBuf,
    ascii_only: bool,
    git_autocommit: bool,
    git_autocommit_confirm: bool,
    editor_cmd: String,
    image_update_concurrency: usize,
    image_update_debug: bool,
    image_update_autocheck: bool,
    kitty_graphics: bool,
    log_dock_enabled: bool,
    log_dock_height: u16,
) -> anyhow::Result<()> {
    const SLEEP_GAP_SECS: u64 = 120;
    const ERROR_PAUSE_THRESHOLD: u32 = 3;
    let mut terminal = setup_terminal().context("failed to setup terminal")?;
    let dashboard_picker = if ascii_only || !kitty_graphics {
        None
    } else {
        Picker::from_query_stdio().ok()
    };
    let (theme_spec, theme_err) = match theme::load_theme(&config_path, &active_theme) {
        Ok(t) => (t, None),
        Err(e) => (theme::default_theme_spec(), Some(e)),
    };
    let mut registries_err: Option<anyhow::Error> = None;
    let registries_cfg = match config::load_registries(&config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            registries_err = Some(e);
            config::RegistriesConfig::default()
        }
    };
    let mut app = App::new(
        servers,
        keymap,
        active_server,
        config_path,
        view_layout,
        active_theme,
        theme_spec,
        dashboard_picker,
        git_autocommit,
        git_autocommit_confirm,
        editor_cmd,
        image_update_concurrency,
        image_update_debug,
        image_update_autocheck,
        kitty_graphics,
        log_dock_enabled,
        log_dock_height,
        registries_cfg,
    );
    if let Some(e) = theme_err {
        app.log_msg(MsgLevel::Warn, format!("failed to load theme: {:#}", e));
    }
    if let Some(e) = registries_err {
        app.log_msg(MsgLevel::Warn, format!("failed to load registries: {:#}", e));
    }
    app.current_target = runner.key();
    if cfg.docker_cmd.is_empty() {
        app.current_target.clear();
        app.loading = false;
        app.loading_since = None;
        app.dashboard.loading = false;
    }
    app.ascii_only = ascii_only;
    app.refresh_secs = refresh.as_secs().max(1);
    app.logs.tail = logs_tail.max(1);
    app.cmd_history_max = cmd_history_max.clamp(1, 5000);
    app.set_cmd_history_entries(cmd_history);
    app.templates_state.dir = expand_user_path(&templates_dir);
    app.refresh_templates();

    // Background fetch: container list, inspect, logs, and actions are done via
    // background tasks so the UI stays responsive.
    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<(
        String,
        anyhow::Result<(
            Vec<ContainerRow>,
            Vec<ImageRow>,
            Vec<VolumeRow>,
            Vec<NetworkRow>,
        )>,
    )>();
    let (refresh_tx, mut refresh_rx) = mpsc::unbounded_channel::<()>();

    let (inspect_req_tx, mut inspect_req_rx) = mpsc::unbounded_channel::<InspectTarget>();
    let (inspect_res_tx, mut inspect_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<Value>)>();

    let (action_req_tx, mut action_req_rx) = mpsc::unbounded_channel::<ActionRequest>();
    let (image_update_req_tx, mut image_update_req_rx) =
        mpsc::unbounded_channel::<(String, bool)>();
    let (action_res_tx, mut action_res_rx) =
        mpsc::unbounded_channel::<(ActionRequest, anyhow::Result<String>)>();

    let (logs_req_tx, mut logs_req_rx) = mpsc::unbounded_channel::<(String, usize)>();
    let (logs_res_tx, mut logs_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<String>)>();

    let (dash_refresh_tx, mut dash_refresh_rx) = mpsc::unbounded_channel::<()>();
    let (dash_res_tx, mut dash_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<DashboardSnapshot>)>();
    let (dash_all_refresh_tx, mut dash_all_refresh_rx) = mpsc::unbounded_channel::<()>();
    let (dash_all_res_tx, mut dash_all_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<DashboardSnapshot>, u128)>();

    let (ip_req_tx, mut ip_req_rx) = mpsc::unbounded_channel::<Vec<String>>();
    let (ip_res_tx, mut ip_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<HashMap<String, String>>)>();

    let (usage_req_tx, mut usage_req_rx) = mpsc::unbounded_channel::<Vec<String>>();
    let (usage_res_tx, mut usage_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<UsageSnapshot>)>();

    let (conn_tx, conn_rx) = watch::channel(Connection {
        runner: runner.clone(),
        docker: cfg.clone(),
    });
    let (dash_all_enabled_tx, dash_all_enabled_rx) = watch::channel(false);
    let (_dash_all_servers_tx, dash_all_servers_rx) = watch::channel(app.servers.clone());

    let (refresh_interval_tx, refresh_interval_rx) =
        watch::channel(Duration::from_secs(app.refresh_secs.max(1)));
    let (refresh_pause_tx, refresh_pause_rx) = watch::channel(false);
    let (image_update_limit_tx, image_update_limit_rx) =
        watch::channel(app.image_update_concurrency.max(1));
    let fetch_task = tokio::spawn(async move {
        let mut refresh_interval_rx = refresh_interval_rx;
        let mut pause_rx = refresh_pause_rx;
        let mut interval = tokio::time::interval(*refresh_interval_rx.borrow());
        let conn_rx = conn_rx;
        let mut conn_rx = conn_rx;
        loop {
            tokio::select! {
              _ = interval.tick() => {}
              maybe = refresh_rx.recv() => {
                if maybe.is_none() {
                  break;
                }
              }
              changed = refresh_interval_rx.changed() => {
                if changed.is_err() {
                  break;
                }
                interval = tokio::time::interval(*refresh_interval_rx.borrow());
              }
              changed = pause_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
              changed = conn_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
            }

            if *pause_rx.borrow() {
                continue;
            }

            let conn = conn_rx.borrow().clone();
            if conn.docker.docker_cmd.is_empty() {
                continue;
            }
            let key = conn.runner.key();
            let cmd = docker::overview_command(&conn.docker);
            let child = match conn.runner.spawn_killable(&cmd) {
                Ok(c) => c,
                Err(e) => {
                    let _ = result_tx.send((key, Err(e)));
                    continue;
                }
            };

            let mut child_opt = Some(child);
            let output = tokio::select! {
              out = async {
                if let Some(child) = child_opt.take() {
                    child.wait_with_output().await
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "overview child already consumed",
                    ))
                }
              } => out,
              changed_res = conn_rx.changed() => {
                // Server switch: kill the in-flight SSH command to avoid waiting
                // for slow "docker stats" on the old server.
                match changed_res {
                  Ok(_) => {
                    if let Some(mut child) = child_opt.take() {
                      let _ = child.kill().await;
                      let _ = child.wait().await;
                    }
                    continue;
                  }
                  Err(_) => {
                    if let Some(mut child) = child_opt.take() {
                      let _ = child.kill().await;
                      let _ = child.wait().await;
                    }
                    break;
                  }
                }
              }
            };

            let res = match output {
                Ok(out) => {
                    if out.status.success() {
                        match String::from_utf8(out.stdout) {
                            Ok(s) => docker::parse_overview_output(&s),
                            Err(e) => Err(anyhow::anyhow!("ssh stdout was not valid UTF-8: {}", e)),
                        }
                    } else {
                        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                        Err(anyhow::anyhow!(
                            "ssh failed: {}",
                            if stderr.is_empty() {
                                "<no stderr>"
                            } else {
                                &stderr
                            }
                        ))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("failed to run ssh: {}", e)),
            };

            let _ = result_tx.send((key, res));
        }
    });

    let dash_conn_rx = conn_tx.subscribe();
    let dash_refresh_interval_rx = refresh_interval_tx.subscribe();
    let dash_pause_rx = refresh_pause_tx.subscribe();
    let dash_task = tokio::spawn(async move {
        let mut dash_refresh_interval_rx = dash_refresh_interval_rx;
        let mut pause_rx = dash_pause_rx;
        let mut interval = tokio::time::interval(*dash_refresh_interval_rx.borrow());
        let mut conn_rx = dash_conn_rx;
        loop {
            tokio::select! {
              _ = interval.tick() => {}
              maybe = dash_refresh_rx.recv() => {
                if maybe.is_none() {
                  break;
                }
              }
              changed = dash_refresh_interval_rx.changed() => {
                if changed.is_err() {
                  break;
                }
                interval = tokio::time::interval(*dash_refresh_interval_rx.borrow());
              }
              changed = pause_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
              changed = conn_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
            }

            if *pause_rx.borrow() {
                continue;
            }

            let conn = conn_rx.borrow().clone();
            if conn.docker.docker_cmd.is_empty() {
                continue;
            }
            let key = conn.runner.key();
            let cmd = dashboard_command(&conn.docker.docker_cmd);
            let child = match conn.runner.spawn_killable(&cmd) {
                Ok(c) => c,
                Err(e) => {
                    let _ = dash_res_tx.send((key, Err(e)));
                    continue;
                }
            };

            let mut child_opt = Some(child);
            let output = tokio::select! {
              out = async {
                let child = child_opt.take().expect("child already taken");
                child.wait_with_output().await
              } => out,
              changed = conn_rx.changed() => {
                // Server switch: kill the in-flight command to avoid waiting.
                if changed.is_ok() {
                  if let Some(mut child) = child_opt.take() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                  }
                  continue;
                }
                if let Some(mut child) = child_opt.take() {
                  let _ = child.kill().await;
                  let _ = child.wait().await;
                }
                break;
              }
            };

            let res = match output {
                Ok(out) => {
                    if out.status.success() {
                        let s = String::from_utf8_lossy(&out.stdout).to_string();
                        parse_dashboard_output(&s)
                    } else {
                        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                        Err(anyhow::anyhow!(
                            "ssh failed: {}",
                            if stderr.is_empty() {
                                "<no stderr>"
                            } else {
                                &stderr
                            }
                        ))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("failed to run ssh: {}", e)),
            };

            let _ = dash_res_tx.send((key, res));
        }
    });

    let dash_all_refresh_interval_rx = refresh_interval_tx.subscribe();
    let dash_all_pause_rx = refresh_pause_tx.subscribe();
    let dash_all_enabled_rx = dash_all_enabled_rx.clone();
    let _dash_all_task = tokio::spawn(async move {
        let mut dash_all_refresh_interval_rx = dash_all_refresh_interval_rx;
        let mut pause_rx = dash_all_pause_rx;
        let mut enabled_rx = dash_all_enabled_rx;
        let mut servers_rx = dash_all_servers_rx;
        let mut interval = tokio::time::interval(*dash_all_refresh_interval_rx.borrow());
        loop {
            tokio::select! {
              _ = interval.tick() => {}
              maybe = dash_all_refresh_rx.recv() => {
                if maybe.is_none() {
                  break;
                }
              }
              changed = dash_all_refresh_interval_rx.changed() => {
                if changed.is_err() {
                  break;
                }
                interval = tokio::time::interval(*dash_all_refresh_interval_rx.borrow());
              }
              changed = pause_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
              changed = enabled_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
              changed = servers_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
            }

            if *pause_rx.borrow() {
                continue;
            }
            if !*enabled_rx.borrow() {
                continue;
            }

            let servers = servers_rx.borrow().clone();
            let concurrency = 6usize;
            let sem = Arc::new(Semaphore::new(concurrency));
            let mut set = JoinSet::new();
            for s in servers {
                if s.docker_cmd.is_empty() {
                    continue;
                }
                let sem = sem.clone();
                let tx = dash_all_res_tx.clone();
                set.spawn(async move {
                    let _permit = sem.acquire().await;
                    let runner = if s.target == "local" {
                        Runner::Local
                    } else {
                        Runner::Ssh(Ssh {
                            target: s.target.clone(),
                            identity: s.identity.clone(),
                            port: s.port,
                        })
                    };
                    let cmd = dashboard_command(&s.docker_cmd);
                    let start = Instant::now();
                    let child = match runner.spawn_killable(&cmd) {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = tx.send((s.name.clone(), Err(e), 0));
                            return;
                        }
                    };
                    let output = child.wait_with_output().await;
                    let latency_ms = start.elapsed().as_millis();
                    let res = match output {
                        Ok(out) => {
                            if out.status.success() {
                                let s = String::from_utf8_lossy(&out.stdout).to_string();
                                parse_dashboard_output(&s)
                            } else {
                                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                                Err(anyhow::anyhow!(
                                    "ssh failed: {}",
                                    if stderr.is_empty() { "<no stderr>" } else { &stderr }
                                ))
                            }
                        }
                        Err(e) => Err(anyhow::anyhow!("failed to run ssh: {}", e)),
                    };
                    let _ = tx.send((s.name.clone(), res, latency_ms));
                });
            }
            while set.join_next().await.is_some() {}
        }
    });

    let inspect_conn_rx = conn_tx.subscribe();
    let inspect_task = tokio::spawn(async move {
        let inspect_conn_rx = inspect_conn_rx;
        while let Some(req) = inspect_req_rx.recv().await {
            let conn = inspect_conn_rx.borrow().clone();
            let res = match req.kind {
                InspectKind::Container => {
                    docker::fetch_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
                InspectKind::Image => {
                    docker::fetch_image_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
                InspectKind::Volume => {
                    docker::fetch_volume_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
                InspectKind::Network => {
                    docker::fetch_network_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
            };
            let res = res.and_then(|raw| {
                serde_json::from_str::<Value>(&raw).context("inspect output was not JSON")
            });
            let _ = inspect_res_tx.send((req.key, res));
        }
    });

    let action_conn_rx = conn_tx.subscribe();
    let action_res_tx_action = action_res_tx.clone();
    let action_task = tokio::spawn(async move {
        let action_conn_rx = action_conn_rx;
        while let Some(req) = action_req_rx.recv().await {
            if let ActionRequest::ImageUpdateCheck { image, debug } = &req {
                let _ = image_update_req_tx.send((image.clone(), *debug));
                continue;
            }
            let conn = action_conn_rx.borrow().clone();
            let res = match &req {
                ActionRequest::Container { action, id } => {
                    docker::container_action(&conn.runner, &conn.docker, *action, id).await
                }
                ActionRequest::RegistryTest {
                    host,
                    auth,
                    test_repo,
                } => {
                    registry_test(host, auth, test_repo.as_deref()).await
                }
                ActionRequest::TemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_compose,
                    pull,
                    force_recreate,
                    template_commit,
                    ..
                } => perform_template_deploy(
                    runner,
                    docker,
                    name,
                    local_compose,
                    *pull,
                    *force_recreate,
                    template_commit.as_deref(),
                )
                .await,
                ActionRequest::StackUpdate {
                    stack_name,
                    runner,
                    docker,
                    compose_dirs,
                    pull,
                    dry,
                    force,
                    services,
                } => perform_stack_update(
                    runner,
                    docker,
                    stack_name,
                    compose_dirs,
                    *pull,
                    *dry,
                    *force,
                    services,
                )
                .await,
                ActionRequest::NetTemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_cfg,
                    force,
                    ..
                } => perform_net_template_deploy(runner, docker, name, local_cfg, *force).await,
                ActionRequest::TemplateFromStack {
                    name,
                    stack_name,
                    source,
                    container_ids,
                    templates_dir,
                } => export_stack_template(
                    &conn.runner,
                    &conn.docker,
                    name,
                    source,
                    Some(stack_name),
                    container_ids,
                    templates_dir,
                )
                .await,
                ActionRequest::TemplateFromContainer {
                    name,
                    source,
                    container_id,
                    templates_dir,
                } => export_stack_template(
                    &conn.runner,
                    &conn.docker,
                    name,
                    source,
                    None,
                    std::slice::from_ref(container_id),
                    templates_dir,
                )
                .await,
                ActionRequest::TemplateFromNetwork {
                    name,
                    source,
                    network_id,
                    templates_dir,
                } => export_net_template(
                    &conn.runner,
                    &conn.docker,
                    name,
                    source,
                    network_id,
                    templates_dir,
                )
                .await,
                ActionRequest::ImageUpdateCheck { .. } => {
                    unreachable!("image update checks are handled in the dispatcher")
                }
                ActionRequest::ImageUntag { reference, .. } => {
                    docker::image_remove(&conn.runner, &conn.docker, reference).await
                }
                ActionRequest::ImageForceRemove { id, .. } => {
                    docker::image_remove_force(&conn.runner, &conn.docker, id).await
                }
                ActionRequest::ImagePush {
                    source_ref,
                    target_ref,
                    registry_host,
                    auth,
                    ..
                } => perform_image_push(
                    &conn.runner,
                    &conn.docker,
                    source_ref,
                    target_ref,
                    registry_host,
                    auth.as_ref(),
                )
                .await,
                ActionRequest::VolumeRemove { name } => {
                    docker::volume_remove(&conn.runner, &conn.docker, name).await
                }
                ActionRequest::NetworkRemove { id } => {
                    docker::network_remove(&conn.runner, &conn.docker, id).await
                }
            };
            let _ = action_res_tx_action.send((req, res));
        }
    });

    let image_update_conn_rx = conn_tx.subscribe();
    let image_update_res_tx = action_res_tx.clone();
    let image_update_task = tokio::spawn(async move {
        let image_update_conn_rx = image_update_conn_rx;
        let mut image_update_limit_rx = image_update_limit_rx;
        let mut semaphore = Arc::new(Semaphore::new(
            (*image_update_limit_rx.borrow()).max(1),
        ));
        loop {
            tokio::select! {
                maybe = image_update_req_rx.recv() => {
                    let Some((image, debug)) = maybe else {
                        break;
                    };
                    let permit = semaphore.clone().acquire_owned().await;
                    let conn = image_update_conn_rx.borrow().clone();
                    let image_update_res_tx = image_update_res_tx.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let svc = ImageUpdateService::new(&conn.runner, &conn.docker, debug);
                        let res = svc.check_image_update(&image).await;
                        let _ = image_update_res_tx
                            .send((ActionRequest::ImageUpdateCheck { image, debug }, res));
                    });
                }
                changed = image_update_limit_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let next = (*image_update_limit_rx.borrow()).max(1);
                    semaphore = Arc::new(Semaphore::new(next));
                }
            }
        }
    });

    let logs_conn_rx = conn_tx.subscribe();
    let logs_task = tokio::spawn(async move {
        let logs_conn_rx = logs_conn_rx;
        while let Some((id, tail)) = logs_req_rx.recv().await {
            let conn = logs_conn_rx.borrow().clone();
            let res = docker::fetch_logs(&conn.runner, &conn.docker, &id, tail.max(1)).await;
            let _ = logs_res_tx.send((id, res));
        }
    });

    let ip_conn_rx = conn_tx.subscribe();
    let ip_task = tokio::spawn(async move {
        let ip_conn_rx = ip_conn_rx;
        while let Some(ids) = ip_req_rx.recv().await {
            let conn = ip_conn_rx.borrow().clone();
            let key = conn.runner.key();
            let res = async {
                let raw = docker::fetch_inspects(&conn.runner, &conn.docker, &ids).await?;
                let v =
                    serde_json::from_str::<Value>(&raw).context("inspect output was not JSON")?;
                let arr = v
                    .as_array()
                    .context("inspect output was not a JSON array")?;
                let mut map: HashMap<String, String> = HashMap::new();
                for item in arr {
                    let id = item
                        .get("Id")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| item.get("Id").map(|x| x.to_string()))
                        .unwrap_or_default();
                    if id.is_empty() {
                        continue;
                    }
                    if let Some(ip) = extract_container_ip(item) {
                        map.insert(id, ip);
                    }
                }
                Ok::<_, anyhow::Error>(map)
            }
            .await;
            let _ = ip_res_tx.send((key, res));
        }
    });

    let usage_conn_rx = conn_tx.subscribe();
    let usage_task = tokio::spawn(async move {
        let usage_conn_rx = usage_conn_rx;
        while let Some(ids) = usage_req_rx.recv().await {
            let conn = usage_conn_rx.borrow().clone();
            let key = conn.runner.key();
            let res = async {
                const CHUNK: usize = 40;
                let mut snapshot = UsageSnapshot::default();

                for chunk in ids.chunks(CHUNK) {
                    let raw = docker::fetch_inspects(&conn.runner, &conn.docker, chunk).await?;
                    let v = serde_json::from_str::<Value>(&raw)
                        .context("inspect output was not JSON")?;
                    let arr = v
                        .as_array()
                        .context("inspect output was not a JSON array")?;
                    for item in arr {
                        let id = item
                            .get("Id")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        if id.is_empty() {
                            continue;
                        }
                        if let Some(ip) = extract_container_ip(item) {
                            snapshot.ip_by_container_id.insert(id.clone(), ip);
                        }

                        let running = item
                            .pointer("/State/Running")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false);

                        let image_id = item
                            .get("Image")
                            .and_then(|x| x.as_str())
                            .map(normalize_image_id)
                            .unwrap_or_default();
                        if !image_id.is_empty() {
                            *snapshot
                                .image_ref_count_by_id
                                .entry(image_id.clone())
                                .or_insert(0) += 1;
                            if running {
                                *snapshot
                                    .image_run_count_by_id
                                    .entry(image_id.clone())
                                    .or_insert(0) += 1;
                            }
                        }

                        let cname = item
                            .get("Name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.trim_start_matches('/').to_string())
                            .unwrap_or_else(|| "-".to_string());

                        if !image_id.is_empty() {
                            snapshot
                                .image_containers_by_id
                                .entry(image_id.clone())
                                .or_default()
                                .push(cname.clone());
                        }

                        if let Some(nets) = item
                            .pointer("/NetworkSettings/Networks")
                            .and_then(|x| x.as_object())
                        {
                            for (_name, net) in nets {
                                let Some(net_id) = net.get("NetworkID").and_then(|x| x.as_str())
                                else {
                                    continue;
                                };
                                let net_id = net_id.trim();
                                if net_id.is_empty() {
                                    continue;
                                }
                                *snapshot
                                    .network_ref_count_by_id
                                    .entry(net_id.to_string())
                                    .or_insert(0) += 1;
                                snapshot
                                    .network_containers_by_id
                                    .entry(net_id.to_string())
                                    .or_default()
                                    .push(cname.clone());
                            }
                        }

                        if let Some(mounts) = item.get("Mounts").and_then(|x| x.as_array()) {
                            for m in mounts {
                                let ty = m.get("Type").and_then(|x| x.as_str()).unwrap_or("");
                                if ty != "volume" {
                                    continue;
                                }
                                let name = m.get("Name").and_then(|x| x.as_str()).unwrap_or("");
                                if name.trim().is_empty() {
                                    continue;
                                }
                                let name = name.trim().to_string();
                                *snapshot
                                    .volume_ref_count_by_name
                                    .entry(name.clone())
                                    .or_insert(0) += 1;
                                if running {
                                    *snapshot
                                        .volume_run_count_by_name
                                        .entry(name.clone())
                                        .or_insert(0) += 1;
                                }
                                snapshot
                                    .volume_containers_by_name
                                    .entry(name)
                                    .or_default()
                                    .push(cname.clone());
                            }
                        }
                    }
                }

                for v in snapshot.image_containers_by_id.values_mut() {
                    v.sort();
                    v.dedup();
                }
                for v in snapshot.volume_containers_by_name.values_mut() {
                    v.sort();
                    v.dedup();
                }
                for v in snapshot.network_containers_by_id.values_mut() {
                    v.sort();
                    v.dedup();
                }

                Ok::<_, anyhow::Error>(snapshot)
            }
            .await;
            let _ = usage_res_tx.send((key, res));
        }
    });

    let _ = refresh_tx.send(());
    let _ = dash_refresh_tx.send(());

    loop {
        if app.should_quit {
            break;
        }
        // Avoid stale "in-progress" markers if the background action result gets lost.
        let now = Instant::now();
        if let Some(last) = app.last_loop_at {
            if now.duration_since(last) > Duration::from_secs(SLEEP_GAP_SECS) {
                if !app.refresh_paused {
                    app.refresh_paused = true;
                    app.refresh_pause_reason = Some("sleep".to_string());
                    app.refresh_error_streak = 0;
                    let _ = refresh_pause_tx.send(true);
                    app.log_msg(
                        MsgLevel::Info,
                        "refresh paused after sleep (press r to retry)",
                    );
                }
                app.reset_screen = true;
            }
        }
        app.last_loop_at = Some(now);
        app.action_inflight.retain(|_, m| now < m.until);
        app.image_action_inflight.retain(|_, m| now < m.until);
        app.volume_action_inflight.retain(|_, m| now < m.until);
        app.network_action_inflight.retain(|_, m| now < m.until);

        while let Ok((key, res)) = result_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            match res {
                Ok((containers, images, volumes, networks)) => {
                    app.images = images;
                    app.volumes = volumes;
                    app.networks = networks;
                    app.images_selected = app
                        .images_selected
                        .min(app.images_visible_len().saturating_sub(1));
                    app.volumes_selected = app
                        .volumes_selected
                        .min(app.volumes_visible_len().saturating_sub(1));
                    app.networks_selected = app
                        .networks_selected
                        .min(app.networks.len().saturating_sub(1));

                    app.set_containers(containers);
                    app.prune_image_marks();
                    app.prune_volume_marks();
                    app.prune_network_marks();
                    app.usage_refresh_needed = true;
                    app.reconcile_noncontainer_action_markers();
                    app.last_refresh = Some(Instant::now());
                    app.refresh_error_streak = 0;
                    app.clear_conn_error();
                    app.clear_last_error();
                }
                Err(e) => {
                    app.loading = false;
                    app.loading_since = None;
                    if app.refresh_paused {
                        continue;
                    }
                    app.refresh_error_streak = app.refresh_error_streak.saturating_add(1);
                    if app.refresh_error_streak >= ERROR_PAUSE_THRESHOLD {
                        app.refresh_paused = true;
                        app.refresh_pause_reason = Some("connection".to_string());
                        let _ = refresh_pause_tx.send(true);
                        app.reset_screen = true;
                        app.log_msg(
                            MsgLevel::Info,
                            "refresh paused after connection errors (press r to retry)",
                        );
                        continue;
                    }
                    app.set_conn_error(format!("{:#}", e));
                }
            }
        }

        while let Ok((key, res)) = ip_res_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            match res {
                Ok(map) => {
                    let now = Instant::now();
                    for (id, ip) in map {
                        app.ip_cache.insert(id, (ip, now));
                    }
                }
                Err(e) => {
                    // Non-fatal; keep the table responsive.
                    app.set_warn(format!("ip lookup failed: {:#}", e));
                }
            }
        }

        while let Ok((key, res)) = dash_res_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            app.dashboard.loading = false;
            match res {
                Ok(snap) => {
                    app.dashboard.error = None;
                    app.dashboard.snap = Some(snap);
                    app.dashboard.last_disk_count = app
                        .dashboard
                        .snap
                        .as_ref()
                        .map(|s| s.disks.len())
                        .unwrap_or(0);
                }
                Err(e) => {
                    let msg = format!("{:#}", e);
                    if app.dashboard.error.as_deref() != Some(&msg) {
                        app.log_msg(MsgLevel::Warn, format!("dashboard failed: {msg}"));
                    }
                    app.dashboard.error = Some(msg);
                }
            }
        }

        while let Ok((name, res, latency_ms)) = dash_all_res_rx.try_recv() {
            let host = app
                .dashboard_all
                .hosts
                .iter_mut()
                .find(|h| h.name == name);
            if let Some(host) = host {
                host.loading = false;
                host.latency_ms = Some(latency_ms);
                match res {
                    Ok(snap) => {
                        host.error = None;
                        host.snap = Some(snap);
                    }
                    Err(e) => {
                        host.error = Some(format!("{:#}", e));
                    }
                }
            }
        }

        while let Ok((key, res)) = usage_res_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            app.usage_loading = false;
            match res {
                Ok(snap) => {
                    // Apply IPs as a bonus (so the container table can show IP faster).
                    let now = Instant::now();
                    for (id, ip) in snap.ip_by_container_id {
                        app.ip_cache.insert(id, (ip, now));
                    }

                    // Images by ImageID.
                    app.image_referenced_by_id.clear();
                    app.image_referenced_count_by_id.clear();
                    app.image_running_count_by_id.clear();
                    app.image_containers_by_id.clear();
                    for img in &app.images {
                        let id = normalize_image_id(&img.id);
                        let refs = snap.image_ref_count_by_id.get(&id).copied().unwrap_or(0);
                        let runs = snap.image_run_count_by_id.get(&id).copied().unwrap_or(0);
                        let ctrs = snap
                            .image_containers_by_id
                            .get(&id)
                            .cloned()
                            .unwrap_or_default();
                        app.image_referenced_by_id.insert(img.id.clone(), refs > 0);
                        app.image_referenced_count_by_id
                            .insert(img.id.clone(), refs);
                        app.image_running_count_by_id.insert(img.id.clone(), runs);
                        app.image_containers_by_id.insert(img.id.clone(), ctrs);
                    }

                    // Volumes by name.
                    app.volume_referenced_by_name.clear();
                    app.volume_referenced_count_by_name.clear();
                    app.volume_running_count_by_name.clear();
                    app.volume_containers_by_name.clear();
                    for v in &app.volumes {
                        let refs = snap
                            .volume_ref_count_by_name
                            .get(&v.name)
                            .copied()
                            .unwrap_or(0);
                        let runs = snap
                            .volume_run_count_by_name
                            .get(&v.name)
                            .copied()
                            .unwrap_or(0);
                        let ctrs = snap
                            .volume_containers_by_name
                            .get(&v.name)
                            .cloned()
                            .unwrap_or_default();
                        app.volume_referenced_by_name
                            .insert(v.name.clone(), refs > 0);
                        app.volume_referenced_count_by_name
                            .insert(v.name.clone(), refs);
                        app.volume_running_count_by_name
                            .insert(v.name.clone(), runs);
                        app.volume_containers_by_name.insert(v.name.clone(), ctrs);
                    }

                    // Networks by NetworkID.
                    app.network_referenced_count_by_id.clear();
                    app.network_containers_by_id.clear();
                    for n in &app.networks {
                        let refs = snap
                            .network_ref_count_by_id
                            .get(&n.id)
                            .copied()
                            .unwrap_or(0);
                        let ctrs = snap
                            .network_containers_by_id
                            .get(&n.id)
                            .cloned()
                            .unwrap_or_default();
                        app.network_referenced_count_by_id
                            .insert(n.id.clone(), refs);
                        app.network_containers_by_id.insert(n.id.clone(), ctrs);
                    }

                    // Clamp selections in case the unused-only toggles depend on usage.
                    app.images_selected = app
                        .images_selected
                        .min(app.images_visible_len().saturating_sub(1));
                    app.volumes_selected = app
                        .volumes_selected
                        .min(app.volumes_visible_len().saturating_sub(1));
                    app.networks_selected = app
                        .networks_selected
                        .min(app.networks.len().saturating_sub(1));
                    app.usage_refresh_needed = false;
                }
                Err(e) => {
                    app.set_warn(format!("usage lookup failed: {:#}", e));
                }
            }
        }

        // Kick off IP refresh opportunistically after container list updates.
        if app.ip_refresh_needed && !app.containers.is_empty() {
            const TTL: Duration = Duration::from_secs(60);
            const MAX_IDS: usize = 40;
            let now = Instant::now();
            let mut ids: Vec<String> = Vec::new();
            for c in &app.containers {
                if is_container_stopped(&c.status) {
                    continue;
                }
                let expired = app
                    .ip_cache
                    .get(&c.id)
                    .map(|(_, at)| now.duration_since(*at) > TTL)
                    .unwrap_or(true);
                if expired {
                    ids.push(c.id.clone());
                    if ids.len() >= MAX_IDS {
                        break;
                    }
                }
            }
            if !ids.is_empty() {
                let _ = ip_req_tx.send(ids);
            }
            app.ip_refresh_needed = false;
        }

        // Kick off usage refresh after overview updates (accurate image/volume usage).
        if app.usage_refresh_needed && !app.containers.is_empty() {
            const MAX_IDS: usize = 200;
            let ids: Vec<String> = app
                .containers
                .iter()
                .take(MAX_IDS)
                .map(|c| c.id.clone())
                .collect();
            if !ids.is_empty() {
                app.usage_loading = true;
                let _ = usage_req_tx.send(ids);
            }
            app.usage_refresh_needed = false;
        }

        while let Ok((id, res)) = inspect_res_rx.try_recv() {
            if app.inspect.for_id.as_deref() != Some(&id) {
                continue;
            }
            app.inspect.loading = false;
            match res {
                Ok(value) => {
                    app.inspect.value = Some(value);
                    app.inspect.error = None;
                    app.rebuild_inspect_lines();
                }
                Err(e) => {
                    app.inspect.value = None;
                    let msg = format!("{:#}", e);
                    app.inspect.error = Some(msg.clone());
                    app.log_msg(MsgLevel::Error, format!("inspect failed: {msg}"));
                    app.rebuild_inspect_lines();
                }
            }
        }

        while let Ok((req, res)) = action_res_rx.try_recv() {
            match res {
                Ok(out) => {
                    app.clear_last_error();
                    match &req {
                        ActionRequest::Container { id, .. } => {
                            app.container_action_error.remove(id);
                        }
                        ActionRequest::RegistryTest { host, .. } => {
                            let key = host.to_ascii_lowercase();
                            app.registry_tests.insert(
                                key,
                                RegistryTestEntry {
                                    checked_at: now_unix(),
                                    ok: true,
                                    message: truncate_msg(&out, 200),
                                },
                            );
                            app.save_local_state();
                            app.log_msg(
                                MsgLevel::Info,
                                format!("registry test ok for {host}: {out}"),
                            );
                        }
                        ActionRequest::TemplateDeploy {
                            name,
                            local_compose,
                            pull,
                            server_name,
                            template_id,
                            template_commit,
                            ..
                        } => {
                            app.templates_state.template_deploy_inflight.remove(name);
                            app.template_action_error.remove(name);
                            app.set_info(format!("deployed template {name}"));
                            if !server_name.trim().is_empty() && !template_id.trim().is_empty() {
                                let entry = TemplateDeployEntry {
                                    server_name: server_name.clone(),
                                    timestamp: now_unix(),
                                    commit: template_commit.clone(),
                                };
                                app.template_deploys
                                    .entry(template_id.clone())
                                    .or_default()
                                    .push(entry);
                                app.save_local_state();
                            }
                            if app.image_update_autocheck && *pull {
                                let images = images_from_compose(local_compose);
                                if !images.is_empty() {
                                    actions::check_image_updates(&mut app, images, &action_req_tx);
                                }
                            }
                        }
                        ActionRequest::StackUpdate { stack_name, dry, .. } => {
                            app.stack_update_inflight.remove(stack_name);
                            app.stack_update_error.remove(stack_name);
                            app.stack_update_containers.remove(stack_name);
                            app.set_info(format!("stack update finished for {stack_name}"));
                            if out.trim().is_empty() {
                                continue;
                            }
                            if *dry || out.lines().count() > 1 {
                                let label = if *dry {
                                    "stack update dry-run output"
                                } else {
                                    "stack update output"
                                };
                                app.log_msg(
                                    MsgLevel::Info,
                                    format!("{label} for {stack_name}:"),
                                );
                                for line in out.lines() {
                                    app.log_msg(MsgLevel::Info, line.to_string());
                                }
                            } else {
                                let msg = truncate_msg(&out, 200);
                                app.log_msg(
                                    MsgLevel::Info,
                                    format!("stack update ok for {stack_name}: {msg}"),
                                );
                            }
                        }
                        ActionRequest::NetTemplateDeploy { name, server_name, .. } => {
                            app.templates_state
                                .net_template_deploy_inflight
                                .remove(name);
                            app.net_template_action_error.remove(name);
                            if out.trim() == "exists" {
                                app.set_warn(format!(
                                    "network '{name}' already exists (use :nettemplate deploy! to recreate)"
                                ));
                            } else {
                                app.set_info(format!("deployed network template {name}"));
                                if !server_name.trim().is_empty() {
                                    let entry = TemplateDeployEntry {
                                        server_name: server_name.clone(),
                                        timestamp: now_unix(),
                                        commit: None,
                                    };
                                    app.net_template_deploys
                                        .entry(name.to_string())
                                        .or_default()
                                        .push(entry);
                                    app.save_local_state();
                                }
                            }
                        }
                        ActionRequest::TemplateFromNetwork { name, .. } => {
                            app.refresh_net_templates();
                            if let Some(idx) = app
                                .templates_state
                                .net_templates
                                .iter()
                                .position(|t| t.name == *name)
                            {
                                app.templates_state.net_templates_selected = idx;
                            }
                            app.set_info(format!("saved network template {name}"));
                            if let Some(server_name) = app.active_server.clone() {
                                if !server_name.trim().is_empty() {
                                    let entry = TemplateDeployEntry {
                                        server_name,
                                        timestamp: now_unix(),
                                        commit: None,
                                    };
                                    app.net_template_deploys
                                        .entry(name.to_string())
                                        .or_default()
                                        .push(entry);
                                    app.save_local_state();
                                }
                            }
                        }
                        ActionRequest::TemplateFromStack { name, stack_name, .. } => {
                            app.refresh_templates();
                            if let Some(idx) = app
                                .templates_state
                                .templates
                                .iter()
                                .position(|t| t.name == *name)
                            {
                                app.templates_state.templates_selected = idx;
                            }
                            app.set_info(format!("saved template {name} from stack {stack_name}"));
                        }
                        ActionRequest::TemplateFromContainer { name, .. } => {
                            app.refresh_templates();
                            if let Some(idx) = app
                                .templates_state
                                .templates
                                .iter()
                                .position(|t| t.name == *name)
                            {
                                app.templates_state.templates_selected = idx;
                            }
                            app.set_info(format!("saved template {name} from container"));
                        }
                        ActionRequest::ImageUpdateCheck { image, .. } => {
                            app.image_updates_inflight.remove(image);
                            match serde_json::from_str::<ImageUpdateResult>(&out) {
                                Ok(result) => {
                                    let status = match result.entry.status {
                                        ImageUpdateKind::UpToDate => "up-to-date",
                                        ImageUpdateKind::UpdateAvailable => "update",
                                        ImageUpdateKind::Error => "error",
                                    };
                                    let local = result
                                        .entry
                                        .local_digest
                                        .as_deref()
                                        .unwrap_or("-");
                                    let remote = result
                                        .entry
                                        .remote_digest
                                        .as_deref()
                                        .unwrap_or("-");
                                    let mut msg = format!(
                                        "image update result: {} status={} local={} remote={}",
                                        result.image, status, local, remote
                                    );
                                    if let Some(err) = result.entry.error.as_deref() {
                                        msg.push_str(&format!(" error={err}"));
                                        if is_rate_limit_error(Some(err)) {
                                            app.note_rate_limit_error(&result.image);
                                        }
                                    }
                                    app.log_msg(MsgLevel::Info, msg);
                                    if let Some(debug) = result.debug.as_deref() {
                                        app.log_msg(
                                            MsgLevel::Info,
                                            format!("image update debug: {debug}"),
                                        );
                                    }
                                    app.image_updates
                                        .insert(result.image.clone(), result.entry);
                                    app.prune_image_updates();
                                    app.save_local_state();
                                }
                                Err(e) => {
                                    app.log_msg(
                                        MsgLevel::Warn,
                                        format!("image update parse failed: {:#}", e),
                                    );
                                }
                            }
                        }
                        ActionRequest::ImageUntag { marker_key, .. } => {
                            app.image_action_error.remove(marker_key);
                        }
                        ActionRequest::ImageForceRemove { marker_key, .. } => {
                            app.image_action_error.remove(marker_key);
                        }
                        ActionRequest::ImagePush { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                            app.image_action_error.remove(marker_key);
                            if out.trim().is_empty() {
                                app.set_info("image push finished");
                            } else {
                                app.set_info("image push finished (see log)");
                                for line in out.lines() {
                                    app.log_msg(MsgLevel::Info, line.to_string());
                                }
                            }
                        }
                        ActionRequest::VolumeRemove { name } => {
                            app.volume_action_error.remove(name);
                        }
                        ActionRequest::NetworkRemove { id } => {
                            app.network_action_error.remove(id);
                        }
                    }
                    // Keep container "in-flight" markers for a short time; the next refresh will
                    // replace the status. For other kinds we just refresh.
                    let _ = refresh_tx.send(());
                    if matches!(
                        req,
                        ActionRequest::TemplateFromStack { .. }
                            | ActionRequest::TemplateFromContainer { .. }
                            | ActionRequest::TemplateFromNetwork { .. }
                    ) && !out.trim().is_empty()
                    {
                        for line in out.lines() {
                            app.log_msg(MsgLevel::Warn, line.to_string());
                        }
                    }
                }
                Err(e) => {
                    match &req {
                        ActionRequest::Container { id, action } => {
                            app.action_inflight.remove(id);
                            app.container_action_error.insert(
                                id.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: format!("{action:?}"),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                        }
                        ActionRequest::RegistryTest { host, .. } => {
                            let key = host.to_ascii_lowercase();
                            app.registry_tests.insert(
                                key,
                                RegistryTestEntry {
                                    checked_at: now_unix(),
                                    ok: false,
                                    message: truncate_msg(&format!("{:#}", e), 200),
                                },
                            );
                            app.save_local_state();
                            app.log_msg(
                                MsgLevel::Warn,
                                format!("registry test failed for {host}: {:#}", e),
                            );
                            continue;
                        }
                        ActionRequest::TemplateDeploy { name, .. } => {
                            app.templates_state.template_deploy_inflight.remove(name);
                            app.template_action_error.insert(
                                name.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "deploy".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                            app.set_error(format!("deploy failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::StackUpdate { stack_name, .. } => {
                            app.stack_update_inflight.remove(stack_name);
                            app.stack_update_containers.remove(stack_name);
                            app.stack_update_error.insert(
                                stack_name.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "update".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                            app.set_error(format!("stack update failed for {stack_name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::NetTemplateDeploy { name, .. } => {
                            app.templates_state
                                .net_template_deploy_inflight
                                .remove(name);
                            app.net_template_action_error.insert(
                                name.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "deploy".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                            app.set_error(format!("deploy failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::TemplateFromStack { name, .. } => {
                            app.set_error(format!("template export failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::TemplateFromContainer { name, .. } => {
                            app.set_error(format!("template export failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::TemplateFromNetwork { name, .. } => {
                            app.set_error(format!(
                                "network template export failed for {name}: {:#}",
                                e
                            ));
                            continue;
                        }
                        ActionRequest::ImageUpdateCheck { image, .. } => {
                            app.image_updates_inflight.remove(image);
                            let entry = ImageUpdateEntry {
                                checked_at: now_unix(),
                                status: ImageUpdateKind::Error,
                                local_digest: None,
                                remote_digest: None,
                                error: Some(truncate_msg(&format!("{:#}", e), 240)),
                            };
                            if is_rate_limit_error(entry.error.as_deref()) {
                                app.note_rate_limit_error(image);
                            }
                            app.image_updates.insert(image.clone(), entry);
                            app.prune_image_updates();
                            app.prune_rate_limits();
                            app.save_local_state();
                            app.log_msg(
                                MsgLevel::Warn,
                                format!("image update failed for {image}: {:#}", e),
                            );
                            continue;
                        }
                        ActionRequest::ImageUntag { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                            app.image_action_error.insert(
                                marker_key.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "untag".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                        }
                        ActionRequest::ImageForceRemove { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                            app.image_action_error.insert(
                                marker_key.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "rm".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                        }
                        ActionRequest::ImagePush { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                            app.image_action_error.insert(
                                marker_key.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "push".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                        }
                        ActionRequest::VolumeRemove { name } => {
                            app.volume_action_inflight.remove(name);
                            app.volume_action_error.insert(
                                name.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "rm".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                        }
                        ActionRequest::NetworkRemove { id } => {
                            app.network_action_inflight.remove(id);
                            app.network_action_error.insert(
                                id.clone(),
                                LastActionError {
                                    at: now_local(),
                                    action: "rm".to_string(),
                                    kind: classify_action_error(&format!("{:#}", e)),
                                    message: truncate_msg(&format!("{:#}", e), 240),
                                },
                            );
                        }
                    }
                    app.set_error(format!("{:#}", e));
                }
            }
        }

        while let Ok((id, res)) = logs_res_rx.try_recv() {
            if app.logs.for_id.as_deref() != Some(&id) {
                continue;
            }
            app.logs.loading = false;
            match res {
                Ok(text) => {
                    app.logs.max_width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                    app.logs.text = Some(text);
                    app.logs.error = None;
                    if app.logs.cursor >= app.logs_total_lines() {
                        app.logs.cursor = app.logs_total_lines().saturating_sub(1);
                    }
                    app.logs_rebuild_matches();
                }
                Err(e) => {
                    app.logs.text = None;
                    let msg = format!("{:#}", e);
                    app.logs.error = Some(msg.clone());
                    app.log_msg(MsgLevel::Error, format!("logs failed: {msg}"));
                    app.logs.cursor = 0;
                    app.logs.hscroll = 0;
                    app.logs.max_width = 0;
                    app.logs_rebuild_matches();
                }
            }
        }

        if app.reset_screen {
            terminal.clear()?;
            app.reset_screen = false;
        }
        let refresh_display = Duration::from_secs(app.refresh_secs.max(1));
        terminal.draw(|f| draw(f, &mut app, refresh_display))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    input::handle_shell_key(
                        &mut app,
                        key,
                        &conn_tx,
                        &refresh_tx,
                        &dash_refresh_tx,
                        &dash_all_refresh_tx,
                        &dash_all_enabled_tx,
                        &refresh_interval_tx,
                        &refresh_pause_tx,
                        &image_update_limit_tx,
                        &inspect_req_tx,
                        &logs_req_tx,
                        &action_req_tx,
                    );
                    if let Some(req) = app.shell_pending_interactive.take() {
                        let runner = current_runner_from_app(&app);
                        restore_terminal(&mut terminal)?;
                        let res = match req {
                            ShellInteractive::RunCommand { cmd } => {
                                run_interactive_command(&runner, &cmd)
                            }
                            ShellInteractive::RunLocalCommand { cmd } => {
                                run_interactive_local_command(&cmd)
                            }
                        };
                        terminal = setup_terminal()?;
                        if let Some(name) = app.templates_state.templates_refresh_after_edit.take()
                        {
                            app.refresh_templates();
                            if let Some(idx) = app
                                .templates_state
                                .templates
                                .iter()
                                .position(|t| t.name == name)
                            {
                                app.templates_state.templates_selected = idx;
                            }
                            app.apply_template_ai_snapshot_if_kind(TemplatesKind::Stacks);
                            maybe_autocommit_templates(
                                &mut app,
                                TemplatesKind::Stacks,
                                "update",
                                &name,
                            );
                        }
                        if let Some(name) =
                            app.templates_state.net_templates_refresh_after_edit.take()
                        {
                            app.refresh_net_templates();
                            if let Some(idx) = app
                                .templates_state
                                .net_templates
                                .iter()
                                .position(|t| t.name == name)
                            {
                                app.templates_state.net_templates_selected = idx;
                            }
                            app.apply_template_ai_snapshot_if_kind(TemplatesKind::Networks);
                            maybe_autocommit_templates(
                                &mut app,
                                TemplatesKind::Networks,
                                "update",
                                &name,
                            );
                        }
                        if let Some(name) = app.theme_refresh_after_edit.take() {
                            commands::theme_cmd::reload_active_theme_after_edit(&mut app, &name);
                            app.reset_dashboard_image();
                        }
                        if let Err(e) = res {
                            app.set_error(format!("{:#}", e));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fetch_task.abort();
    dash_task.abort();
    inspect_task.abort();
    action_task.abort();
    image_update_task.abort();
    logs_task.abort();
    ip_task.abort();
    usage_task.abort();
    restore_terminal(&mut terminal).context("failed to restore terminal")?;
    Ok(())
}

fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}
fn run_interactive_command(runner: &Runner, cmd: &str) -> anyhow::Result<()> {
    match runner {
        Runner::Ssh(ssh) => {
            let mut c = StdCommand::new("ssh");
            // Allocate a tty for interactive docker exec.
            c.arg("-t");
            if let Some(port) = ssh.port {
                c.arg("-p").arg(port.to_string());
            }
            if let Some(identity) = &ssh.identity {
                c.arg("-i").arg(identity);
            }
            c.arg(&ssh.target).arg("--").arg(cmd);
            c.stdin(Stdio::inherit());
            c.stdout(Stdio::inherit());
            c.stderr(Stdio::inherit());
            let status = c.status().context("failed to run ssh")?;
            if !status.success() {
                anyhow::bail!("ssh exited with {}", status);
            }
            Ok(())
        }
        Runner::Local => {
            let status = StdCommand::new("sh")
                .arg("-lc")
                .arg(cmd)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .context("failed to run local command")?;
            if !status.success() {
                anyhow::bail!("local command exited with {}", status);
            }
            Ok(())
        }
    }
}

fn run_interactive_local_command(cmd: &str) -> anyhow::Result<()> {
    let status = StdCommand::new("sh")
        .arg("-lc")
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run local command")?;
    if !status.success() {
        anyhow::bail!("local command exited with {}", status);
    }
    Ok(())
}

// moved to ui/helpers.rs

fn shell_quote_with_home(s: &str) -> String {
    if s.starts_with("$HOME/") {
        format!("\"{s}\"")
    } else {
        shell_single_quote(s)
    }
}

fn parse_git_status_path(line: &str) -> Option<String> {
    let line = line.trim_end();
    if line.len() < 4 {
        return None;
    }
    let rest = line.get(3..)?.trim();
    let path = if let Some((_, new)) = rest.split_once(" -> ") {
        new.trim()
    } else {
        rest
    };
    let path = path.trim_matches('"');
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn git_remote_status_for_path(repo: &Path, rel_path: &str) -> GitRemoteStatus {
    let ahead = commands::git_cmd::run_git(
        repo,
        &["log", "--oneline", "@{u}..", "--", rel_path],
    )
    .ok()
    .map(|out| !out.trim().is_empty())
    .unwrap_or(false);
    let behind = commands::git_cmd::run_git(
        repo,
        &["log", "--oneline", "..@{u}", "--", rel_path],
    )
    .ok()
    .map(|out| !out.trim().is_empty())
    .unwrap_or(false);
    match (ahead, behind) {
        (false, false) => GitRemoteStatus::UpToDate,
        (true, false) => GitRemoteStatus::Ahead,
        (false, true) => GitRemoteStatus::Behind,
        (true, true) => GitRemoteStatus::Diverged,
    }
}

fn file_content_hash(path: &Path) -> Option<u64> {
    let data = fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    hasher.write(&data);
    Some(hasher.finish())
}

async fn perform_template_deploy(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    local_compose: &Path,
    pull: bool,
    force_recreate: bool,
    template_commit: Option<&str>,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let remote_dir = match runner {
        Runner::Local => {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            format!("{home}/.config/containr/apps/{name}")
        }
        Runner::Ssh(_) => deploy_remote_dir_for(name),
    };
    let template_id = ensure_template_id(&local_compose.to_path_buf())?;
    let rendered_path =
        render_compose_with_template_id(local_compose, &template_id, template_commit)?;
    let remote_compose = format!("{remote_dir}/compose.rendered.yaml");
    let remote_dir_q = shell_single_quote(&remote_dir);
    let compose_cmd = docker.docker_cmd.to_compose_shell();
    let mkdir_cmd = format!("mkdir -p {remote_dir_q}");
    let pull_cmd = format!("cd {remote_dir_q} && {compose_cmd} -f compose.rendered.yaml pull");
    let recreate_flag = if force_recreate { " --force-recreate" } else { "" };
    let up_cmd =
        format!("cd {remote_dir_q} && {compose_cmd} -f compose.rendered.yaml up -d{recreate_flag}");
    runner.run(&mkdir_cmd).await?;
    runner.copy_file_to(rendered_path.as_ref(), &remote_compose).await?;
    if pull {
        let _ = run_with_local_compose_fallback(runner, &pull_cmd).await?;
    }
    let out = run_with_local_compose_fallback(runner, &up_cmd).await?;
    Ok(out)
}

async fn run_with_local_compose_fallback(runner: &Runner, cmd: &str) -> anyhow::Result<String> {
    match runner.run(cmd).await {
        Ok(out) => Ok(out),
        Err(e) => {
            let msg = format!("{:#}", e);
            let is_missing_desktop_helper = msg.contains("docker-credential-desktop")
                && msg.contains("executable file not found");
            if !matches!(runner, Runner::Local) || !is_missing_desktop_helper {
                return Err(e);
            }

            let home = std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("HOME is not set for local compose fallback"))?;
            let cfg_dir = PathBuf::from(home).join(".config/containr/docker-no-creds");
            fs::create_dir_all(&cfg_dir)
                .map_err(|err| anyhow::anyhow!("failed to create local docker fallback dir: {err}"))?;
            let cfg_file = cfg_dir.join("config.json");
            fs::write(&cfg_file, "{\"auths\":{}}\n").map_err(|err| {
                anyhow::anyhow!("failed to write local docker fallback config: {err}")
            })?;
            let cfg_q = shell_single_quote(cfg_dir.to_string_lossy().as_ref());
            let wrapped = format!("export DOCKER_CONFIG={cfg_q}; {cmd}");
            runner.run(&wrapped).await
        }
    }
}

async fn perform_stack_update(
    runner: &Runner,
    docker: &DockerCfg,
    stack_name: &str,
    compose_dirs: &[String],
    pull: bool,
    dry: bool,
    force: bool,
    services: &[StackUpdateService],
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let mut selected_dir: Option<String> = None;
    for dir in compose_dirs {
        let dir = dir.trim();
        if dir.is_empty() {
            continue;
        }
        let ok = match runner {
            Runner::Local => Path::new(dir).is_dir(),
            Runner::Ssh(_) => {
                let dir_q = shell_quote_with_home(dir);
                runner.run(&format!("test -d {dir_q}")).await.is_ok()
            }
        };
        if ok {
            selected_dir = Some(dir.to_string());
            break;
        }
    }
    let dir = selected_dir.ok_or_else(|| {
        anyhow::anyhow!(
            "stack update: compose dir not found for {stack_name} (tried: {})",
            compose_dirs.join(", ")
        )
    })?;
    let dir_q = shell_quote_with_home(&dir);
    let file_q = shell_single_quote("compose.rendered.yaml");
    let docker_cmd = docker.docker_cmd.to_shell();
    let compose_cmd = docker.docker_cmd.to_compose_shell();
    let pull_cmd = format!("cd {dir_q} && {compose_cmd} -f {file_q} pull");
    let mut svc_args: Vec<String> = Vec::new();
    for svc in services {
        let name = svc.name.trim();
        if !name.is_empty() {
            svc_args.push(shell_single_quote(name));
        }
    }
    let svc_args_str = if svc_args.is_empty() {
        String::new()
    } else {
        format!(" {}", svc_args.join(" "))
    };
    let up_cmd =
        format!("cd {dir_q} && {compose_cmd} -f {file_q} up -d --force-recreate{svc_args_str}");
    if dry {
        let mut lines = Vec::new();
        lines.push(format!("stack update dry-run: {stack_name}"));
        lines.push(format!("compose dir: {}", dir));
        if pull {
            lines.push(pull_cmd);
        }
        lines.push(up_cmd);
        return Ok(lines.join("\n"));
    }
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("stack update: {stack_name}"));
    lines.push(format!("compose dir: {}", dir));
    if pull {
        let pull_out = run_with_local_compose_fallback(runner, &pull_cmd).await?;
        let pull_msg = pull_out.trim();
        if pull_msg.is_empty() {
            lines.push("pull: ok".to_string());
        } else {
            lines.push(format!("pull: {}", truncate_msg(pull_msg, 200)));
        }
    } else {
        lines.push("pull: skipped".to_string());
    }
    let mut to_recreate: Vec<String> = Vec::new();
    if force {
        for svc in services {
            if !svc.name.trim().is_empty() {
                to_recreate.push(svc.name.clone());
            }
        }
    } else {
        for svc in services {
            let container_id = svc.container_id.trim();
            if container_id.is_empty() {
                continue;
            }
            let image_ref = svc.image.trim();
            if image_ref.is_empty() {
                continue;
            }
            let container_id_q = shell_single_quote(container_id);
            let image_ref_q = shell_single_quote(image_ref);
            let container_cmd = format!(
                "{docker_cmd} inspect --format '{{{{.Image}}}}' {container_id_q}"
            );
            let image_cmd = format!(
                "{docker_cmd} image inspect --format '{{{{.Id}}}}' {image_ref_q}"
            );
            let current = runner.run(&container_cmd).await?.trim().to_string();
            let latest = runner.run(&image_cmd).await?.trim().to_string();
            let cur_short = if current.len() > 20 {
                truncate_msg(&current, 20)
            } else {
                current.clone()
            };
            let new_short = if latest.len() > 20 {
                truncate_msg(&latest, 20)
            } else {
                latest.clone()
            };
            if current.is_empty() || latest.is_empty() {
                lines.push(format!("svc {}: digest missing", svc.name));
                continue;
            }
            if current != latest {
                lines.push(format!(
                    "svc {}: {} -> {} (update)",
                    svc.name, cur_short, new_short
                ));
                to_recreate.push(svc.name.clone());
            } else {
                lines.push(format!("svc {}: {} (no change)", svc.name, cur_short));
            }
        }
    }
    if !force && to_recreate.is_empty() {
        lines.push("result: no changes".to_string());
        return Ok(lines.join("\n"));
    }
    if !force {
        let mut uniq: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for name in to_recreate {
            if seen.insert(name.clone()) {
                uniq.push(name);
            }
        }
        svc_args = uniq
            .iter()
            .map(|name| shell_single_quote(name))
            .collect();
    }
    let svc_args_str = if svc_args.is_empty() {
        String::new()
    } else {
        format!(" {}", svc_args.join(" "))
    };
    let up_cmd =
        format!("cd {dir_q} && {compose_cmd} -f {file_q} up -d --force-recreate{svc_args_str}");
    if !svc_args_str.is_empty() {
        let raw = svc_args
            .iter()
            .map(|s| s.trim_matches('\''))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("recreate: {raw}"));
    } else {
        lines.push("recreate: all".to_string());
    }
    let out = run_with_local_compose_fallback(runner, &up_cmd).await?;
    let out_msg = out.trim();
    if !out_msg.is_empty() {
        lines.push(format!("compose up: {}", truncate_msg(out_msg, 200)));
    } else {
        lines.push("compose up: ok".to_string());
    }
    Ok(lines.join("\n"))
}

async fn perform_image_push(
    runner: &Runner,
    docker: &DockerCfg,
    source_ref: &str,
    target_ref: &str,
    registry_host: &str,
    auth: Option<&RegistryAuthResolved>,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker_cmd = docker.docker_cmd.to_shell();
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("image push: {source_ref} -> {target_ref}"));
    if let Some(auth) = auth {
        if !matches!(auth.auth, config::RegistryAuth::Anonymous) {
            let secret = auth
                .secret
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("registry secret missing for {registry_host}"))?;
            let username = auth
                .username
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| match auth.auth {
                    config::RegistryAuth::BearerToken => Some("token".to_string()),
                    config::RegistryAuth::GithubPat => Some("token".to_string()),
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("registry username missing for {registry_host}"))?;
            let pass_q = shell_single_quote(secret);
            let user_q = shell_single_quote(&username);
            let host_q = shell_single_quote(registry_host);
            let login_cmd = format!(
                "printf %s {pass_q} | {docker_cmd} login -u {user_q} --password-stdin {host_q}"
            );
            let out = runner.run(&login_cmd).await?;
            if !out.trim().is_empty() {
                lines.push(format!("login: {}", truncate_msg(out.trim(), 200)));
            } else {
                lines.push("login: ok".to_string());
            }
        } else {
            lines.push("login: skipped".to_string());
        }
    } else {
        lines.push("login: skipped".to_string());
    }
    let src_q = shell_single_quote(source_ref);
    let dst_q = shell_single_quote(target_ref);
    let tag_cmd = format!("{docker_cmd} image tag {src_q} {dst_q}");
    runner.run(&tag_cmd).await?;
    lines.push("tag: ok".to_string());
    let push_cmd = format!("{docker_cmd} image push {dst_q}");
    let out = runner.run(&push_cmd).await?;
    if !out.trim().is_empty() {
        lines.push(format!("push: {}", truncate_msg(out.trim(), 200)));
    } else {
        lines.push("push: ok".to_string());
    }
    Ok(lines.join("\n"))
}

async fn perform_net_template_deploy(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    local_cfg: &Path,
    force: bool,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let raw = fs::read_to_string(local_cfg)
        .with_context(|| format!("failed to read {}", local_cfg.display()))?;
    let spec: NetworkTemplateSpec =
        serde_json::from_str(&raw).context("network.json was not valid JSON")?;
    let net_name = spec.name.trim();
    anyhow::ensure!(!net_name.is_empty(), "network template: name is empty");

    let remote_dir = match runner {
        Runner::Local => {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            format!("{home}/.config/containr/networks/{name}")
        }
        Runner::Ssh(_) => deploy_remote_net_dir_for(name),
    };
    let remote_cfg = format!("{remote_dir}/network.json");
    let remote_dir_q = shell_single_quote(&remote_dir);
    let mkdir_cmd = format!("mkdir -p {remote_dir_q}");
    runner.run(&mkdir_cmd).await?;
    runner.copy_file_to(local_cfg, &remote_cfg).await?;

    let docker_cmd = docker.docker_cmd.to_shell();
    let net_q = shell_single_quote(net_name);
    let exists_cmd = format!("{docker_cmd} network inspect {net_q} >/dev/null 2>&1");
    let exists = runner.run(&exists_cmd).await.is_ok();
    if exists && !force {
        return Ok("exists".to_string());
    }
    if exists && force {
        let rm_cmd = format!("{docker_cmd} network rm {net_q}");
        runner.run(&rm_cmd).await?;
    }

    let mut parts: Vec<String> = Vec::new();
    parts.push(docker_cmd.clone());
    parts.push("network".to_string());
    parts.push("create".to_string());

    let driver = spec
        .driver
        .as_deref()
        .unwrap_or("bridge")
        .trim()
        .to_string();
    parts.push("--driver".to_string());
    parts.push(shell_single_quote(&driver));

    if spec.internal.unwrap_or(false) {
        parts.push("--internal".to_string());
    }
    if spec.attachable.unwrap_or(false) {
        parts.push("--attachable".to_string());
    }

    if let Some(ipv4) = &spec.ipv4 {
        if let Some(subnet) = ipv4.subnet.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--subnet".to_string());
            parts.push(shell_single_quote(subnet.trim()));
        }
        if let Some(gw) = ipv4.gateway.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--gateway".to_string());
            parts.push(shell_single_quote(gw.trim()));
        }
        if let Some(r) = ipv4.ip_range.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--ip-range".to_string());
            parts.push(shell_single_quote(r.trim()));
        }
    }

    let mut effective_parent: Option<String> = None;
    if driver == "ipvlan" {
        let parent = spec.parent.as_deref().unwrap_or("").trim();
        anyhow::ensure!(!parent.is_empty(), "ipvlan requires 'parent'");
        effective_parent = Some(parent.to_string());
        parts.push("--opt".to_string());
        parts.push(shell_single_quote(&format!("parent={parent}")));
        if let Some(mode) = spec.ipvlan_mode.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--opt".to_string());
            parts.push(shell_single_quote(&format!("ipvlan_mode={}", mode.trim())));
        }
    }

    if let Some(opts) = &spec.options {
        for (k, v) in opts {
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            parts.push("--opt".to_string());
            parts.push(shell_single_quote(&format!("{k}={v}")));
        }
    }
    if let Some(labels) = &spec.labels {
        for (k, v) in labels {
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            parts.push("--label".to_string());
            parts.push(shell_single_quote(&format!("{k}={v}")));
        }
    }

    parts.push(net_q);
    let create_cmd = parts.join(" ");
    match runner.run(&create_cmd).await {
        Ok(out) => Ok(out),
        Err(primary_err) => {
            let parent = effective_parent.as_deref().unwrap_or("");
            let can_retry_macos_parent = cfg!(target_os = "macos")
                && matches!(runner, Runner::Local)
                && driver == "ipvlan"
                && parent.starts_with("en")
                && !parent.is_empty();
            if !can_retry_macos_parent {
                return Err(primary_err);
            }

            let vlan_suffix = parent
                .split_once('.')
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty());
            let detected_base = detect_local_ipvlan_parent_base(runner, docker).await;
            let mapped_parent = if let Some(base) = detected_base {
                if let Some(vlan) = &vlan_suffix {
                    format!("{base}.{vlan}")
                } else {
                    base
                }
            } else if let Some(vlan) = &vlan_suffix {
                format!("eth0.{vlan}")
            } else {
                "eth0".to_string()
            };

            let from_quoted = shell_single_quote(&format!("parent={parent}"));
            let to_quoted = shell_single_quote(&format!("parent={mapped_parent}"));
            let from_plain = format!("parent={parent}");
            let to_plain = format!("parent={mapped_parent}");
            let retry_cmd = create_cmd
                .replace(&from_quoted, &to_quoted)
                .replace(&from_plain, &to_plain);

            match runner.run(&retry_cmd).await {
                Ok(out) => Ok(format!(
                    "{}\n(macos local ipvlan parent remapped: {} -> {})",
                    out.trim(),
                    parent,
                    mapped_parent
                )
                .trim()
                .to_string()),
                Err(_) => Err(primary_err),
            }
        }
    }
}

async fn detect_local_ipvlan_parent_base(runner: &Runner, docker: &DockerCfg) -> Option<String> {
    if !matches!(runner, Runner::Local) {
        return None;
    }
    let docker_cmd = docker.docker_cmd.to_shell();
    if docker_cmd.trim().is_empty() {
        return None;
    }
    let cmd = format!(
        "{docker_cmd} network inspect $({docker_cmd} network ls -q) --format '{{{{.Driver}}}} {{{{index .Options \"parent\"}}}}' 2>/dev/null || true"
    );
    let out = runner.run(&cmd).await.ok()?;
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let drv = it.next().unwrap_or("");
        let parent = it.next().unwrap_or("");
        if drv != "ipvlan" || parent.is_empty() || parent == "<no" {
            continue;
        }
        let base = parent.split('.').next().unwrap_or(parent).trim();
        if !base.is_empty() {
            return Some(base.to_string());
        }
    }
    None
}

pub(in crate::ui) fn current_runner_from_app(app: &App) -> Runner {
    if let Some(name) = &app.active_server {
        if let Some(s) = app.servers.iter().find(|x| &x.name == name) {
            if s.target == "local" {
                return Runner::Local;
            }
            return Runner::Ssh(Ssh {
                target: s.target.clone(),
                identity: s.identity.clone(),
                port: s.port,
            });
        }
    }
    if app.current_target == "local" {
        Runner::Local
    } else {
        Runner::Ssh(Ssh {
            target: app.current_target.clone(),
            identity: None,
            port: None,
        })
    }
}

pub(in crate::ui) fn current_docker_cmd_from_app(app: &App) -> DockerCmd {
    if let Some(name) = &app.active_server {
        if let Some(s) = app.servers.iter().find(|x| &x.name == name) {
            return s.docker_cmd.clone();
        }
    }
    DockerCmd::default()
}

pub(in crate::ui) fn current_server_label(app: &App) -> String {
    if app.server_all_selected {
        return "All servers".to_string();
    }
    if let Some(name) = app.active_server.as_deref() {
        return name.to_string();
    }
    if !app.current_target.trim().is_empty() {
        return app.current_target.clone();
    }
    "no server".to_string()
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "integration"))]
mod integration_tests;
