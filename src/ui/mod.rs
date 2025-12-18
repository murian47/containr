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

pub mod theme;
mod commands;

use crate::config::{self, ContainrConfig, KeyBinding, ServerEntry};
use crate::docker::{
    self, ContainerAction, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow,
};
use crate::runner::Runner;
use crate::ssh::Ssh;
use anyhow::Context as _;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Cell, List, ListItem, ListState, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
};
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};
use std::{collections::HashMap, collections::HashSet, fmt::Write as _};
use tokio::sync::mpsc;
use tokio::sync::watch;

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

#[derive(Debug, Default, Clone)]
struct CmdHistory {
    entries: Vec<String>,
    pos: Option<usize>,
    saved_current: String,
}

impl CmdHistory {
    fn new() -> Self {
        Self::default()
    }

    fn reset_nav(&mut self) {
        self.pos = None;
        self.saved_current.clear();
    }

    fn on_edit(&mut self) {
        if self.pos.is_some() {
            self.reset_nav();
        }
    }

    fn push(&mut self, cmd: &str, max: usize) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if self.entries.last().is_some_and(|x| x == cmd) {
            return;
        }
        self.entries.push(cmd.to_string());
        let max = max.max(1);
        if self.entries.len() > max {
            let drain = self.entries.len() - max;
            self.entries.drain(0..drain);
        }
    }

    fn prev(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let len = self.entries.len();
        let pos = match self.pos {
            None => {
                self.saved_current = current.to_string();
                len - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.pos = Some(pos);
        self.entries.get(pos).cloned()
    }

    fn next(&mut self) -> Option<String> {
        let Some(pos) = self.pos else {
            return None;
        };
        let len = self.entries.len();
        if pos + 1 >= len {
            self.pos = None;
            return Some(std::mem::take(&mut self.saved_current));
        }
        let pos = pos + 1;
        self.pos = Some(pos);
        self.entries.get(pos).cloned()
    }
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

fn shell_begin_confirm(app: &mut App, label: impl Into<String>, cmdline: impl Into<String>) {
    app.shell_cmdline.mode = true;
    app.shell_cmdline.input.clear();
    app.shell_cmdline.cursor = 0;
    app.shell_cmdline.confirm = Some(ShellConfirm {
        label: label.into(),
        cmdline: cmdline.into(),
    });
}

fn clamp_cursor_to_text(text: &str, cursor: usize) -> usize {
    cursor.min(text.chars().count())
}

fn insert_char_at_cursor(text: &mut String, cursor: &mut usize, ch: char) {
    let mut out = String::new();
    let mut idx = 0usize;
    let cur = clamp_cursor_to_text(text, *cursor);
    for c in text.chars() {
        if idx == cur {
            out.push(ch);
        }
        out.push(c);
        idx += 1;
    }
    if idx == cur {
        out.push(ch);
    }
    *text = out;
    *cursor = cur.saturating_add(1);
}

fn backspace_at_cursor(text: &mut String, cursor: &mut usize) {
    let cur = clamp_cursor_to_text(text, *cursor);
    if cur == 0 {
        *cursor = 0;
        return;
    }
    let target = cur - 1;
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i != target {
            out.push(c);
        }
    }
    *text = out;
    *cursor = target;
}

fn delete_at_cursor(text: &mut String, cursor: &mut usize) {
    let cur = clamp_cursor_to_text(text, *cursor);
    let len = text.chars().count();
    if cur >= len {
        *cursor = len;
        return;
    }
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i != cur {
            out.push(c);
        }
    }
    *text = out;
    *cursor = cur.min(text.chars().count());
}

fn set_text_and_cursor(text: &mut String, cursor: &mut usize, new_text: String) {
    *text = new_text;
    *cursor = text.chars().count();
}

fn input_window_with_cursor(text: &str, cursor: usize, width: usize) -> (String, String, String) {
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
        "container" | "ctr" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "template" | "tpl" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "nettemplate" | "nettpl" | "ntpl" | "nt" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
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
enum ListMode {
    Flat,
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveView {
    Containers,
    Images,
    Volumes,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ShellView {
    Containers,
    Images,
    Volumes,
    Networks,
    Templates,
    Inspect,
    Logs,
    Help,
    Messages,
}

impl ShellView {
    fn slug(self) -> &'static str {
        match self {
            ShellView::Containers => "containers",
            ShellView::Images => "images",
            ShellView::Volumes => "volumes",
            ShellView::Networks => "networks",
            ShellView::Templates => "templates",
            ShellView::Inspect => "inspect",
            ShellView::Logs => "logs",
            ShellView::Help => "help",
            ShellView::Messages => "messages",
        }
    }

    fn title(self) -> &'static str {
        match self {
            ShellView::Containers => "Containers",
            ShellView::Images => "Images",
            ShellView::Volumes => "Volumes",
            ShellView::Networks => "Networks",
            ShellView::Templates => "Templates",
            ShellView::Inspect => "Inspect",
            ShellView::Logs => "Logs",
            ShellView::Help => "Help",
            ShellView::Messages => "Messages",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellFocus {
    Sidebar,
    List,
    Details,
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
    at: Duration,
    level: MsgLevel,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellAction {
    Start,
    Stop,
    Restart,
    Delete,
    Console,
    ImageUntag,
    ImageForceRemove,
    VolumeRemove,
    NetworkRemove,
    TemplateEdit,
    TemplateNew,
    TemplateDelete,
    TemplateDeploy,
}

impl ShellAction {
    fn label(self) -> &'static str {
        match self {
            ShellAction::Start => "Start",
            ShellAction::Stop => "Stop",
            ShellAction::Restart => "Restart",
            ShellAction::Delete => "Delete",
            ShellAction::Console => "Console",
            ShellAction::ImageUntag => "Untag",
            ShellAction::ImageForceRemove => "Remove",
            ShellAction::VolumeRemove => "Remove",
            ShellAction::NetworkRemove => "Remove",
            ShellAction::TemplateEdit => "Edit",
            ShellAction::TemplateNew => "New",
            ShellAction::TemplateDelete => "Delete",
            ShellAction::TemplateDeploy => "Deploy",
        }
    }

    fn ctrl_hint(self) -> &'static str {
        match self {
            ShellAction::Start => "^s",
            ShellAction::Stop => "^o",
            ShellAction::Restart => "^r",
            ShellAction::Delete => "^d",
            // Console: ^c = bash, ^C = sh (Ctrl+Shift+C)
            ShellAction::Console => "^c",
            // Non-container actions: keep a separate chord to avoid ambiguity
            ShellAction::ImageUntag => "^u",
            ShellAction::ImageForceRemove => "^d",
            ShellAction::VolumeRemove => "^d",
            ShellAction::NetworkRemove => "^d",
            ShellAction::TemplateEdit => "^e",
            ShellAction::TemplateNew => "^n",
            ShellAction::TemplateDelete => "^d",
            ShellAction::TemplateDeploy => "^y",
        }
    }
}

fn shell_module_shortcut(view: ShellView) -> char {
    match view {
        ShellView::Containers => 'c',
        ShellView::Images => 'm',
        ShellView::Volumes => 'v',
        ShellView::Networks => 'n',
        ShellView::Templates => 't',
        ShellView::Inspect => 'i',
        ShellView::Logs => 'l',
        ShellView::Help => '?',
        // Not a primary module; used only for internal navigation/help display.
        ShellView::Messages => 'g',
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
    for ch in ['C', 'M', 'I', 'V', 'N', 'L'] {
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
}

#[derive(Clone, Debug)]
struct NetTemplateEntry {
    name: String,
    dir: PathBuf,
    cfg_path: PathBuf,
    has_cfg: bool,
    desc: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct NetworkTemplateIpv4 {
    subnet: Option<String>,
    gateway: Option<String>,
    #[serde(rename = "ip_range")]
    ip_range: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
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

#[derive(Clone, Copy, Debug)]
struct DeployMarker {
    started: Instant,
}

#[derive(Clone, Copy, Debug)]
struct ActionMarker {
    action: ContainerAction,
    until: Instant,
}

#[derive(Clone, Copy, Debug)]
struct SimpleMarker {
    until: Instant,
}

#[derive(Debug, Clone)]
enum ActionRequest {
    Container {
        action: ContainerAction,
        id: String,
    },
    TemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_compose: PathBuf,
    },
    NetTemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_cfg: PathBuf,
        force: bool,
    },
    ImageUntag {
        marker_key: String,
        reference: String,
    },
    ImageForceRemove {
        marker_key: String,
        id: String,
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
    volume_referenced_by_name: HashMap<String, bool>,
    volume_referenced_count_by_name: HashMap<String, usize>,
    volume_running_count_by_name: HashMap<String, usize>,
    volume_containers_by_name: HashMap<String, Vec<String>>,
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
    conn_error: Option<String>,
    last_error: Option<String>,
    loading: bool,
    loading_since: Option<Instant>,
    action_inflight: HashMap<String, ActionMarker>,
    image_action_inflight: HashMap<String, SimpleMarker>,
    volume_action_inflight: HashMap<String, SimpleMarker>,
    network_action_inflight: HashMap<String, SimpleMarker>,
    inspect_loading: bool,
    inspect_error: Option<String>,
    inspect_value: Option<Value>,
    inspect_target: Option<InspectTarget>,
    inspect_for_id: Option<String>,
    inspect_lines: Vec<InspectLine>,
    inspect_selected: usize,
    inspect_scroll_top: usize,
    inspect_scroll: usize,
    inspect_query: String,
    inspect_expanded: HashSet<String>,
    inspect_match_paths: Vec<String>,
    inspect_path_rank: HashMap<String, usize>,
	inspect_mode: InspectMode,
	inspect_input: String,
	inspect_input_cursor: usize,
	inspect_cmd_history: CmdHistory,

    servers: Vec<ServerEntry>,
    active_server: Option<String>,
    server_selected: usize,
    config_path: std::path::PathBuf,
    current_target: String,

    logs_loading: bool,
    logs_error: Option<String>,
    logs_text: Option<String>,
    logs_for_id: Option<String>,
    logs_tail: usize,
    logs_cursor: usize,
    logs_scroll_top: usize,
    logs_select_anchor: Option<usize>,
    logs_hscroll: usize,
    logs_max_width: usize,
	logs_mode: LogsMode,
	logs_input: String,
	logs_query: String,
	logs_command: String,
	logs_input_cursor: usize,
	logs_command_cursor: usize,
	logs_cmd_history: CmdHistory,
    logs_use_regex: bool,
    logs_regex: Option<Regex>,
    logs_regex_error: Option<String>,
    logs_match_lines: Vec<usize>,
    logs_show_line_numbers: bool,

    mouse_enabled: bool,

    ip_cache: HashMap<String, (String, Instant)>,
    ip_refresh_needed: bool,
    should_quit: bool,
    ascii_only: bool,

    theme_name: String,
    theme: theme::ThemeSpec,

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
    shell_help_scroll: usize,
    shell_help_return: ShellView,
    refresh_secs: u64,
    cmd_history_max: usize,

    session_start: Instant,
    session_msgs: Vec<SessionMsg>,
    shell_msgs_scroll: usize, // cursor (absolute); usize::MAX = last
    shell_msgs_hscroll: usize,
    shell_msgs_return: ShellView,

    keymap: Vec<KeyBinding>,
    keymap_parsed: HashMap<(KeyScope, KeySpec), String>,
    keymap_defaults: HashMap<(KeyScope, KeySpec), String>,

    templates_dir: PathBuf,
    templates_kind: TemplatesKind,
    templates: Vec<TemplateEntry>,
    templates_selected: usize,
    templates_error: Option<String>,
    templates_details_scroll: usize,
    templates_refresh_after_edit: Option<String>,
    template_deploy_inflight: HashMap<String, DeployMarker>,

    net_templates: Vec<NetTemplateEntry>,
    net_templates_selected: usize,
    net_templates_error: Option<String>,
    net_templates_details_scroll: usize,
    net_templates_refresh_after_edit: Option<String>,
    net_template_deploy_inflight: HashMap<String, DeployMarker>,

    theme_refresh_after_edit: Option<String>,
}

#[derive(Clone, Debug)]
struct ShellConfirm {
    label: String,
    cmdline: String, // command line without leading ':'
}

impl App {
    fn log_msg(&mut self, level: MsgLevel, text: impl Into<String>) {
        let text = text.into();
        let at = self.session_start.elapsed();
        self.session_msgs.push(SessionMsg { at, level, text });
    }

    fn get_view_split_mode(&self, view: ShellView) -> Option<ShellSplitMode> {
        self.shell_split_by_view.get(view.slug()).copied()
    }

    fn set_view_split_mode(&mut self, view: ShellView, mode: ShellSplitMode) {
        self.shell_split_by_view
            .insert(view.slug().to_string(), mode);
    }

    fn cmd_history_max_effective(&self) -> usize {
        self.cmd_history_max.clamp(1, 5000)
    }

    fn set_cmd_history_entries(&mut self, mut entries: Vec<String>) {
        entries.retain(|s| !s.trim().is_empty());
        let max = self.cmd_history_max_effective();
        if entries.len() > max {
            let drain = entries.len() - max;
            entries.drain(0..drain);
        }
        self.shell_cmdline.history.entries = entries.clone();
        self.shell_cmdline.history.reset_nav();
        self.logs_cmd_history.entries = entries.clone();
        self.logs_cmd_history.reset_nav();
        self.inspect_cmd_history.entries = entries;
        self.inspect_cmd_history.reset_nav();
    }

    fn push_cmd_history(&mut self, cmd: &str) {
        let max = self.cmd_history_max_effective();
        self.shell_cmdline.history.push(cmd, max);
        // Keep all command modes in sync.
        let entries = self.shell_cmdline.history.entries.clone();
        self.logs_cmd_history.entries = entries.clone();
        self.inspect_cmd_history.entries = entries;
        self.shell_cmdline.history.reset_nav();
        self.logs_cmd_history.reset_nav();
        self.inspect_cmd_history.reset_nav();
        self.persist_config();
    }

    fn clear_last_error(&mut self) {
        self.last_error = None;
    }

    fn set_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.last_error = Some(t.clone());
        self.log_msg(MsgLevel::Error, t);
    }

    fn set_warn(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.last_error = Some(t.clone());
        self.log_msg(MsgLevel::Warn, t);
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.log_msg(MsgLevel::Info, text);
    }

    fn messages_copy_selected(&mut self) {
        if self.session_msgs.is_empty() {
            self.set_warn("no messages");
            return;
        }
        let idx = if self.shell_msgs_scroll == usize::MAX {
            self.session_msgs.len().saturating_sub(1)
        } else {
            self.shell_msgs_scroll
                .min(self.session_msgs.len().saturating_sub(1))
        };
        let m = &self.session_msgs[idx];
        let lvl = match m.level {
            MsgLevel::Info => "INFO",
            MsgLevel::Warn => "WARN",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let line = format!("{ts} {lvl} {}", m.text);
        if let Err(e) = copy_to_clipboard(&line) {
            self.set_error(format!("{e:#}"));
        } else {
            self.set_info("copied message to clipboard");
        }
    }

    fn clear_conn_error(&mut self) {
        self.conn_error = None;
    }

    fn set_conn_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.conn_error = Some(t.clone());
        self.set_error(t);
    }

    fn image_row_key(img: &ImageRow) -> String {
        if img.repository != "<none>" && img.tag != "<none>" && !img.tag.trim().is_empty() {
            format!("ref:{}:{}", img.repository, img.tag)
        } else {
            format!("id:{}", img.id)
        }
    }

    fn image_row_ref(img: &ImageRow) -> Option<String> {
        if img.repository != "<none>" && img.tag != "<none>" && !img.tag.trim().is_empty() {
            Some(format!("{}:{}", img.repository, img.tag))
        } else {
            None
        }
    }

    fn new(
        servers: Vec<ServerEntry>,
        keymap: Vec<KeyBinding>,
        active_server: Option<String>,
        config_path: std::path::PathBuf,
        view_layout: HashMap<String, String>,
        theme_name: String,
        theme: theme::ThemeSpec,
    ) -> Self {
        let mut server_selected = 0usize;
        if let Some(name) = &active_server {
            if let Some(idx) = servers.iter().position(|s| &s.name == name) {
                server_selected = idx;
            }
        }
        let mut app = Self {
            containers: Vec::new(),
            images: Vec::new(),
            volumes: Vec::new(),
            networks: Vec::new(),
            image_referenced_by_id: HashMap::new(),
            image_referenced_count_by_id: HashMap::new(),
            image_running_count_by_id: HashMap::new(),
            images_unused_only: false,
            volume_referenced_by_name: HashMap::new(),
            volume_referenced_count_by_name: HashMap::new(),
            volume_running_count_by_name: HashMap::new(),
            volume_containers_by_name: HashMap::new(),
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
            conn_error: None,
            last_error: None,
            loading: true,
            loading_since: Some(Instant::now()),
            action_inflight: HashMap::new(),
            image_action_inflight: HashMap::new(),
            volume_action_inflight: HashMap::new(),
            network_action_inflight: HashMap::new(),
            inspect_loading: false,
            inspect_error: None,
            inspect_value: None,
            inspect_target: None,
            inspect_for_id: None,
            inspect_lines: Vec::new(),
            inspect_selected: 0,
            inspect_scroll_top: 0,
            inspect_scroll: 0,
            inspect_query: String::new(),
            inspect_expanded: HashSet::new(),
            inspect_match_paths: Vec::new(),
            inspect_path_rank: HashMap::new(),
            inspect_mode: InspectMode::Normal,
            inspect_input: String::new(),
            inspect_input_cursor: 0,
            inspect_cmd_history: CmdHistory::new(),

            servers,
            active_server,
            server_selected,
            config_path,
            current_target: String::new(),

            logs_loading: false,
            logs_error: None,
            logs_text: None,
            logs_for_id: None,
            logs_tail: 500,
            logs_cursor: 0,
            logs_scroll_top: 0,
            logs_select_anchor: None,
            logs_hscroll: 0,
            logs_max_width: 0,
            logs_mode: LogsMode::Normal,
            logs_input: String::new(),
            logs_query: String::new(),
            logs_command: String::new(),
            logs_input_cursor: 0,
            logs_command_cursor: 0,
            logs_cmd_history: CmdHistory::new(),
            logs_use_regex: false,
            logs_regex: None,
            logs_regex_error: None,
            logs_match_lines: Vec::new(),
            logs_show_line_numbers: false,

            mouse_enabled: false,

            ip_cache: HashMap::new(),
            ip_refresh_needed: true,
            should_quit: false,
            ascii_only: false,
            theme_name,
            theme,
            shell_view: ShellView::Containers,
            shell_last_main_view: ShellView::Containers,
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
            shell_help_scroll: 0,
            shell_help_return: ShellView::Containers,
            refresh_secs: 5,
            cmd_history_max: 200,

            session_start: Instant::now(),
            session_msgs: Vec::new(),
            shell_msgs_scroll: 0,
            shell_msgs_hscroll: 0,
            shell_msgs_return: ShellView::Containers,

            keymap,
            keymap_parsed: HashMap::new(),
            keymap_defaults: HashMap::new(),

            templates_dir: PathBuf::from("templates"),
            templates_kind: TemplatesKind::Stacks,
            templates: Vec::new(),
            templates_selected: 0,
            templates_error: None,
            templates_details_scroll: 0,
            templates_refresh_after_edit: None,
            template_deploy_inflight: HashMap::new(),

            net_templates: Vec::new(),
            net_templates_selected: 0,
            net_templates_error: None,
            net_templates_details_scroll: 0,
            net_templates_refresh_after_edit: None,
            net_template_deploy_inflight: HashMap::new(),

            theme_refresh_after_edit: None,
        };
        app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
        app.rebuild_keymap();
        if let Some(mode) = app.get_view_split_mode(app.shell_view) {
            app.shell_split_mode = mode;
        }
        app
    }

    fn refresh_templates(&mut self) {
        self.templates_error = None;
        self.templates.clear();
        self.templates_details_scroll = 0;

        self.migrate_templates_layout_if_needed();

        let dir = self.stack_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.templates_error = Some(format!("failed to create templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.templates_error = Some(format!("failed to read templates dir: {e}"));
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
            out.push(TemplateEntry {
                name,
                dir: path,
                compose_path,
                has_compose,
                desc,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.templates = out;
        if self.templates_selected >= self.templates.len() {
            self.templates_selected = self.templates.len().saturating_sub(1);
        }
    }

    fn selected_template(&self) -> Option<&TemplateEntry> {
        self.templates.get(self.templates_selected)
    }

    fn net_templates_dir(&self) -> PathBuf {
        self.templates_dir.join("networks")
    }

    fn stack_templates_dir(&self) -> PathBuf {
        self.templates_dir.join("stacks")
    }

    fn migrate_templates_layout_if_needed(&mut self) {
        // Old layout: <templates_dir>/<name>/compose.yaml and <templates_dir>/networks/...
        // New layout: <templates_dir>/stacks/<name>/compose.yaml and <templates_dir>/networks/...
        let stacks = self.stack_templates_dir();
        if stacks.exists() {
            return;
        }
        let root = self.templates_dir.clone();
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
                format!("failed to create stacks templates dir '{}': {e}", stacks.display()),
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
                    format!(
                        "template migration failed for '{}': {}",
                        name,
                        e
                    ),
                );
            }
        }
    }

    fn refresh_net_templates(&mut self) {
        self.net_templates_error = None;
        self.net_templates.clear();
        self.net_templates_details_scroll = 0;

        self.migrate_templates_layout_if_needed();

        let dir = self.net_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.net_templates_error = Some(format!("failed to create net templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.net_templates_error = Some(format!("failed to read net templates dir: {e}"));
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
        self.net_templates = out;
        if self.net_templates_selected >= self.net_templates.len() {
            self.net_templates_selected = self.net_templates.len().saturating_sub(1);
        }
    }

    fn selected_net_template(&self) -> Option<&NetTemplateEntry> {
        self.net_templates.get(self.net_templates_selected)
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

    fn selected_container(&self) -> Option<&ContainerRow> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        match self.list_mode {
            ListMode::Flat => self.containers.get(self.selected),
            ListMode::Tree => {
                let Some(entry) = self.view.get(self.selected) else {
                    return None;
                };
                let ViewEntry::Container { id, .. } = entry else {
                    return None;
                };
                let idx = self.container_idx_by_id.get(id)?;
                self.containers.get(*idx)
            }
        }
    }

    fn selected_stack(&self) -> Option<(&str, usize, usize, bool)> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        if self.list_mode != ListMode::Tree {
            return None;
        }
        let Some(entry) = self.view.get(self.selected) else {
            return None;
        };
        match entry {
            ViewEntry::StackHeader {
                name,
                total,
                running,
                expanded,
            } => Some((name.as_str(), *total, *running, *expanded)),
            ViewEntry::UngroupedHeader { total, running } => {
                Some(("Ungrouped", *total, *running, true))
            }
            _ => None,
        }
    }

    fn selected_stack_container_ids(&mut self) -> Option<Vec<String>> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        if self.list_mode != ListMode::Tree {
            return None;
        }
        self.ensure_view();
        let Some(entry) = self.view.get(self.selected) else {
            return None;
        };
        let ViewEntry::StackHeader { name, .. } = entry else {
            return None;
        };
        let stack = name.clone();
        let mut ids: Vec<String> = self
            .containers
            .iter()
            .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack.as_str()))
            .map(|c| c.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        Some(ids)
    }

    fn view_len(&mut self) -> usize {
        if self.active_view != ActiveView::Containers {
            return 0;
        }
        self.ensure_view();
        match self.list_mode {
            ListMode::Flat => self.containers.len(),
            ListMode::Tree => self.view.len(),
        }
    }

    fn ensure_view(&mut self) {
        if self.active_view != ActiveView::Containers {
            return;
        }
        if self.list_mode != ListMode::Tree {
            self.view.clear();
            self.view_dirty = false;
            return;
        }
        if !self.view_dirty {
            return;
        }
        self.view_dirty = false;
        self.rebuild_tree_view();
    }

    fn current_anchor(&self) -> Option<(String, Option<String>)> {
        // (container_id, stack_name) where stack_name is Some only if selection is a stack header.
        match self.list_mode {
            ListMode::Flat => self.selected_container().map(|c| (c.id.clone(), None)),
            ListMode::Tree => match self.view.get(self.selected) {
                Some(ViewEntry::Container { id, .. }) => Some((id.clone(), None)),
                Some(ViewEntry::StackHeader { name, .. }) => {
                    Some(("".to_string(), Some(name.clone())))
                }
                Some(ViewEntry::UngroupedHeader { .. }) => {
                    Some(("".to_string(), Some("Ungrouped".to_string())))
                }
                None => None,
            },
        }
    }

    fn rebuild_tree_view(&mut self) {
        use std::collections::BTreeMap;

        let anchor = self.current_anchor();

        let mut stacks: BTreeMap<String, Vec<&ContainerRow>> = BTreeMap::new();
        let mut ungrouped: Vec<&ContainerRow> = Vec::new();
        for c in &self.containers {
            if let Some(stack) = stack_name_from_labels(&c.labels) {
                stacks.entry(stack).or_default().push(c);
            } else {
                ungrouped.push(c);
            }
        }

        let mut out: Vec<ViewEntry> = Vec::new();

        for (name, mut cs) in stacks {
            cs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            let total = cs.len();
            let running = cs
                .iter()
                .filter(|c| !is_container_stopped(&c.status))
                .count();
            let expanded = !self.stack_collapsed.contains(&name);
            out.push(ViewEntry::StackHeader {
                name: name.clone(),
                total,
                running,
                expanded,
            });
            if expanded {
                for c in cs {
                    out.push(ViewEntry::Container {
                        id: c.id.clone(),
                        indent: 2,
                    });
                }
            }
        }

        if !ungrouped.is_empty() {
            let total = ungrouped.len();
            let running = ungrouped
                .iter()
                .filter(|c| !is_container_stopped(&c.status))
                .count();
            out.push(ViewEntry::UngroupedHeader { total, running });
            ungrouped.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            for c in ungrouped {
                out.push(ViewEntry::Container {
                    id: c.id.clone(),
                    indent: 2,
                });
            }
        }

        self.view = out;

        // Restore selection when possible.
        if let Some((id, stack)) = anchor {
            if !id.is_empty() {
                if let Some(idx) = self
                    .view
                    .iter()
                    .position(|e| matches!(e, ViewEntry::Container { id: cid, .. } if cid == &id))
                {
                    self.selected = idx;
                    return;
                }
            }
            if let Some(stack) = stack {
                if let Some(idx) = self.view.iter().position(
                    |e| matches!(e, ViewEntry::StackHeader { name, .. } if name == &stack),
                ) {
                    self.selected = idx;
                    return;
                }
            }
        }
        if self.selected >= self.view.len() {
            self.selected = self.view.len().saturating_sub(1);
        }
    }

    fn toggle_tree_expanded_selected(&mut self) -> bool {
        if self.active_view != ActiveView::Containers || self.list_mode != ListMode::Tree {
            return false;
        }
        self.ensure_view();
        let Some(entry) = self.view.get(self.selected).cloned() else {
            return false;
        };
        match entry {
            ViewEntry::StackHeader { name, .. } => {
                if !self.stack_collapsed.insert(name.clone()) {
                    self.stack_collapsed.remove(&name);
                }
                self.view_dirty = true;
                self.ensure_view();
                true
            }
            _ => false,
        }
    }

    fn is_marked(&self, id: &str) -> bool {
        self.marked.contains(id)
    }

    fn is_image_marked(&self, key: &str) -> bool {
        self.marked_images.contains(key)
    }

    fn is_volume_marked(&self, name: &str) -> bool {
        self.marked_volumes.contains(name)
    }

    fn is_network_marked(&self, id: &str) -> bool {
        self.marked_networks.contains(id)
    }

    fn toggle_mark_selected(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                let Some(id) = self.selected_container().map(|c| c.id.clone()) else {
                    return;
                };
                if !self.marked.remove(&id) {
                    self.marked.insert(id);
                }
            }
            ActiveView::Images => {
                let Some(img) = self.selected_image() else {
                    return;
                };
                let key = App::image_row_key(img);
                if !self.marked_images.remove(&key) {
                    self.marked_images.insert(key);
                }
            }
            ActiveView::Volumes => {
                let Some(name) = self.selected_volume().map(|v| v.name.clone()) else {
                    return;
                };
                if !self.marked_volumes.remove(&name) {
                    self.marked_volumes.insert(name);
                }
            }
            ActiveView::Networks => {
                let Some(id) = self.selected_network().map(|n| n.id.clone()) else {
                    return;
                };
                if !self.marked_networks.remove(&id) {
                    self.marked_networks.insert(id);
                }
            }
        }
    }

    fn mark_all(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                for c in &self.containers {
                    self.marked.insert(c.id.clone());
                }
            }
            ActiveView::Images => {
                if self.images_unused_only {
                    for img in &self.images {
                        if !self.image_referenced(img) {
                            self.marked_images.insert(App::image_row_key(img));
                        }
                    }
                } else {
                    for img in &self.images {
                        self.marked_images.insert(App::image_row_key(img));
                    }
                }
            }
            ActiveView::Volumes => {
                if self.volumes_unused_only {
                    for v in &self.volumes {
                        if !self.volume_referenced(v) {
                            self.marked_volumes.insert(v.name.clone());
                        }
                    }
                } else {
                    for v in &self.volumes {
                        self.marked_volumes.insert(v.name.clone());
                    }
                }
            }
            ActiveView::Networks => {
                for n in &self.networks {
                    self.marked_networks.insert(n.id.clone());
                }
            }
        }
    }

    fn clear_marks(&mut self) {
        match self.active_view {
            ActiveView::Containers => self.marked.clear(),
            ActiveView::Images => self.marked_images.clear(),
            ActiveView::Volumes => self.marked_volumes.clear(),
            ActiveView::Networks => self.marked_networks.clear(),
        }
    }

    fn clear_all_marks(&mut self) {
        self.marked.clear();
        self.marked_images.clear();
        self.marked_volumes.clear();
        self.marked_networks.clear();
    }

    fn prune_marks(&mut self) {
        if self.marked.is_empty() || self.containers.is_empty() {
            if self.containers.is_empty() {
                // Keep marks during transient loading; they will be pruned after we have data again.
            }
            return;
        }
        let present: HashSet<&str> = self.containers.iter().map(|c| c.id.as_str()).collect();
        self.marked.retain(|id| present.contains(id.as_str()));
    }

    fn prune_image_marks(&mut self) {
        if self.marked_images.is_empty() || self.images.is_empty() {
            if self.images.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<String> = self.images.iter().map(App::image_row_key).collect();
        self.marked_images.retain(|k| present.contains(k));
    }

    fn prune_volume_marks(&mut self) {
        if self.marked_volumes.is_empty() || self.volumes.is_empty() {
            if self.volumes.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<&str> = self.volumes.iter().map(|v| v.name.as_str()).collect();
        self.marked_volumes
            .retain(|name| present.contains(name.as_str()));
    }

    fn prune_network_marks(&mut self) {
        if self.marked_networks.is_empty() || self.networks.is_empty() {
            if self.networks.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<&str> = self.networks.iter().map(|n| n.id.as_str()).collect();
        self.marked_networks
            .retain(|id| present.contains(id.as_str()));
    }

    fn move_up(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                if self.view_len() == 0 {
                    self.selected = 0;
                    return;
                }
                self.selected = self.selected.saturating_sub(1);
            }
            ActiveView::Images => self.images_selected = self.images_selected.saturating_sub(1),
            ActiveView::Volumes => self.volumes_selected = self.volumes_selected.saturating_sub(1),
            ActiveView::Networks => {
                self.networks_selected = self.networks_selected.saturating_sub(1)
            }
        }
    }

    fn move_down(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                if self.view_len() == 0 {
                    self.selected = 0;
                    return;
                }
                self.selected = (self.selected + 1).min(self.view_len().saturating_sub(1));
            }
            ActiveView::Images => {
                if self.images_visible_len() == 0 {
                    self.images_selected = 0;
                } else {
                    self.images_selected =
                        (self.images_selected + 1).min(self.images_visible_len() - 1);
                }
            }
            ActiveView::Volumes => {
                if self.volumes_visible_len() == 0 {
                    self.volumes_selected = 0;
                } else {
                    self.volumes_selected =
                        (self.volumes_selected + 1).min(self.volumes_visible_len() - 1);
                }
            }
            ActiveView::Networks => {
                if self.networks.is_empty() {
                    self.networks_selected = 0;
                } else {
                    self.networks_selected =
                        (self.networks_selected + 1).min(self.networks.len() - 1);
                }
            }
        }
    }

    fn set_containers(&mut self, containers: Vec<ContainerRow>) {
        self.containers = containers;
        self.container_idx_by_id.clear();
        for (i, c) in self.containers.iter().enumerate() {
            self.container_idx_by_id.insert(c.id.clone(), i);
        }
        self.loading = false;
        self.loading_since = None;
        self.ip_refresh_needed = true;
        self.prune_marks();
        self.view_dirty = true;
        self.reconcile_action_markers();
        self.ensure_view();
        let max = match self.list_mode {
            ListMode::Flat => self.containers.len(),
            ListMode::Tree => self.view.len(),
        };
        if self.selected >= max {
            self.selected = max.saturating_sub(1);
        }
    }

    fn reconcile_noncontainer_action_markers(&mut self) {
        let now = Instant::now();
        let present_image_ids: HashSet<&str> = self.images.iter().map(|i| i.id.as_str()).collect();
        let present_image_refs: HashSet<String> =
            self.images.iter().map(App::image_row_key).collect();
        self.image_action_inflight.retain(|k, m| {
            if now >= m.until {
                return false;
            }
            if k.starts_with("ref:") {
                return present_image_refs.contains(k);
            }
            // Fallback: allow raw image IDs to keep markers across tag changes.
            present_image_ids.contains(k.as_str()) || present_image_refs.contains(k)
        });
        let present_vols: HashSet<&str> = self.volumes.iter().map(|v| v.name.as_str()).collect();
        self.volume_action_inflight
            .retain(|name, m| now < m.until && present_vols.contains(name.as_str()));
        let present_nets: HashSet<&str> = self.networks.iter().map(|n| n.id.as_str()).collect();
        self.network_action_inflight
            .retain(|id, m| now < m.until && present_nets.contains(id.as_str()));
    }

    fn image_referenced(&self, img: &ImageRow) -> bool {
        self.image_referenced_by_id
            .get(&img.id)
            .copied()
            .unwrap_or(false)
    }

    fn volume_referenced(&self, v: &VolumeRow) -> bool {
        self.volume_referenced_by_name
            .get(&v.name)
            .copied()
            .unwrap_or(false)
    }

    fn reconcile_action_markers(&mut self) {
        // The docker start/stop/restart command may return before docker ps reflects the new state.
        // Keep showing the marker until we observe a matching state, or until the marker expires.
        let now = Instant::now();
        self.action_inflight.retain(|id, marker| {
            if now >= marker.until {
                return false;
            }
            let Some(c) = self.containers.iter().find(|c| &c.id == id) else {
                // If it's gone, we consider the action done (or the container removed).
                return false;
            };
            let running =
                c.status.trim().starts_with("Up") || c.status.trim().starts_with("Restarting");
            let stopped = is_container_stopped(&c.status);
            match marker.action {
                ContainerAction::Start => !running,
                ContainerAction::Stop => !stopped,
                ContainerAction::Restart => !running,
                ContainerAction::Remove => true,
            }
        });
    }

    fn start_loading(&mut self, clear_list: bool) {
        self.loading = true;
        self.loading_since = Some(Instant::now());
        self.clear_last_error();
        if clear_list {
            self.containers.clear();
            self.selected = 0;
            self.images.clear();
            self.volumes.clear();
            self.networks.clear();
            self.image_referenced_by_id.clear();
            self.image_referenced_count_by_id.clear();
            self.image_running_count_by_id.clear();
            self.volume_referenced_by_name.clear();
            self.volume_referenced_count_by_name.clear();
            self.volume_running_count_by_name.clear();
            self.volume_containers_by_name.clear();
            self.images_selected = 0;
            self.volumes_selected = 0;
            self.networks_selected = 0;
        }
    }

    fn open_inspect_state(&mut self, target: InspectTarget) {
        self.inspect_loading = true;
        self.inspect_error = None;
        self.inspect_value = None;
        self.inspect_target = Some(target.clone());
        self.inspect_for_id = Some(target.key);
        self.inspect_lines.clear();
        self.inspect_selected = 0;
        self.inspect_scroll_top = 0;
        self.inspect_scroll = 0;
        self.inspect_query.clear();
        self.inspect_expanded.clear();
        self.inspect_expanded.insert("".to_string()); // root expanded by default
        self.inspect_match_paths.clear();
        self.inspect_path_rank.clear();
        self.inspect_mode = InspectMode::Normal;
        self.inspect_input.clear();
    }

    fn rebuild_inspect_lines(&mut self) {
        self.inspect_path_rank = collect_path_rank(self.inspect_value.as_ref());
        let effective_query = self.inspect_effective_query().to_string();
        self.inspect_match_paths =
            collect_match_paths(self.inspect_value.as_ref(), &effective_query);
        let match_set: HashSet<String> = self.inspect_match_paths.iter().cloned().collect();
        self.inspect_lines = build_inspect_lines(
            self.inspect_value.as_ref(),
            &self.inspect_expanded,
            &match_set,
            &effective_query,
        );
        if self.inspect_selected >= self.inspect_lines.len() {
            self.inspect_selected = self.inspect_lines.len().saturating_sub(1);
        }
        if self.inspect_scroll > self.inspect_selected {
            self.inspect_scroll = self.inspect_selected;
        }
    }

    fn inspect_move_up(&mut self, by: usize) {
        if self.inspect_lines.is_empty() {
            self.inspect_selected = 0;
            self.inspect_scroll = 0;
            return;
        }
        self.inspect_selected = self.inspect_selected.saturating_sub(by);
        if self.inspect_selected < self.inspect_scroll {
            self.inspect_scroll = self.inspect_selected;
        }
    }

    fn inspect_move_down(&mut self, by: usize) {
        if self.inspect_lines.is_empty() {
            self.inspect_selected = 0;
            self.inspect_scroll = 0;
            return;
        }
        self.inspect_selected = self
            .inspect_selected
            .saturating_add(by)
            .min(self.inspect_lines.len() - 1);
    }

    fn inspect_toggle_selected(&mut self) {
        let Some(line) = self.inspect_lines.get(self.inspect_selected) else {
            return;
        };
        if !line.expandable {
            return;
        }
        if self.inspect_expanded.contains(&line.path) {
            self.inspect_expanded.remove(&line.path);
        } else {
            self.inspect_expanded.insert(line.path.clone());
        }
        self.rebuild_inspect_lines();
    }

    fn inspect_expand_all(&mut self) {
        let Some(root) = self.inspect_value.as_ref() else {
            return;
        };
        self.inspect_expanded = collect_expandable_paths(root);
        self.inspect_expanded.insert("".to_string());
        self.rebuild_inspect_lines();
    }

    fn inspect_collapse_all(&mut self) {
        self.inspect_expanded.clear();
        self.inspect_expanded.insert("".to_string());
        self.rebuild_inspect_lines();
    }

    fn inspect_jump_next_match(&mut self) {
        if self.inspect_mode != InspectMode::Normal {
            return;
        }
        if self.inspect_match_paths.is_empty() {
            return;
        }
        let current_path = self
            .inspect_lines
            .get(self.inspect_selected)
            .map(|l| l.path.as_str())
            .unwrap_or("");

        let current_rank = self
            .inspect_path_rank
            .get(current_path)
            .copied()
            .unwrap_or(0);

        let mut best: Option<(usize, String)> = None;
        for p in &self.inspect_match_paths {
            let r = self.inspect_path_rank.get(p).copied().unwrap_or(usize::MAX);
            if r > current_rank && best.as_ref().map(|(br, _)| r < *br).unwrap_or(true) {
                best = Some((r, p.clone()));
            }
        }
        let target = best
            .map(|(_, p)| p)
            .or_else(|| self.inspect_match_paths.first().cloned());
        if let Some(target) = target {
            self.inspect_focus_path(&target);
        }
    }

    fn inspect_jump_prev_match(&mut self) {
        if self.inspect_mode != InspectMode::Normal {
            return;
        }
        if self.inspect_match_paths.is_empty() {
            return;
        }
        let current_path = self
            .inspect_lines
            .get(self.inspect_selected)
            .map(|l| l.path.as_str())
            .unwrap_or("");

        let current_rank = self
            .inspect_path_rank
            .get(current_path)
            .copied()
            .unwrap_or(0);

        let mut best: Option<(usize, String)> = None;
        for p in &self.inspect_match_paths {
            let r = self.inspect_path_rank.get(p).copied().unwrap_or(0);
            if r < current_rank && best.as_ref().map(|(br, _)| r > *br).unwrap_or(true) {
                best = Some((r, p.clone()));
            }
        }
        let target = best
            .map(|(_, p)| p)
            .or_else(|| self.inspect_match_paths.last().cloned());
        if let Some(target) = target {
            self.inspect_focus_path(&target);
        }
    }

    fn inspect_focus_path(&mut self, path: &str) {
        for parent in ancestors_of_pointer(path) {
            self.inspect_expanded.insert(parent);
        }
        self.rebuild_inspect_lines();
        if let Some(idx) = self.inspect_lines.iter().position(|l| l.path == path) {
            self.inspect_selected = idx;
        }
    }

    fn inspect_effective_query(&self) -> &str {
        match self.inspect_mode {
            InspectMode::Search => &self.inspect_input,
            _ => &self.inspect_query,
        }
    }

    fn inspect_enter_search(&mut self) {
        self.inspect_mode = InspectMode::Search;
        self.inspect_input = self.inspect_query.clone();
        self.inspect_input_cursor = self.inspect_input.chars().count();
        self.rebuild_inspect_lines();
    }

    fn inspect_enter_command(&mut self) {
        self.inspect_mode = InspectMode::Command;
        self.inspect_input.clear();
        self.inspect_input_cursor = 0;
    }

    fn inspect_exit_input(&mut self) {
        self.inspect_mode = InspectMode::Normal;
        self.inspect_input.clear();
        self.inspect_input_cursor = 0;
        self.rebuild_inspect_lines();
    }

    fn inspect_commit_search(&mut self) {
        self.inspect_query = self.inspect_input.clone();
        self.inspect_mode = InspectMode::Normal;
        self.inspect_input.clear();
        self.inspect_input_cursor = 0;
        self.rebuild_inspect_lines();
        if let Some(first) = self.inspect_match_paths.first().cloned() {
            self.inspect_focus_path(&first);
        }
    }

    fn inspect_copy_selected_value(&mut self, pretty: bool) {
        let Some(root) = self.inspect_value.as_ref() else {
            return;
        };
        let Some(line) = self.inspect_lines.get(self.inspect_selected) else {
            return;
        };
        let Some(value) = root.pointer(&line.path) else {
            self.inspect_error = Some("failed to locate selected value".to_string());
            return;
        };

        let text = if pretty {
            match serde_json::to_string_pretty(value) {
                Ok(s) => s,
                Err(e) => {
                    self.inspect_error = Some(format!("failed to serialize value: {:#}", e));
                    return;
                }
            }
        } else {
            value.to_string()
        };

        if let Err(e) = copy_to_clipboard(&text) {
            self.inspect_error = Some(format!("{:#}", e));
        }
    }

    fn inspect_copy_selected_path(&mut self) {
        let Some(line) = self.inspect_lines.get(self.inspect_selected) else {
            return;
        };
        if let Err(e) = copy_to_clipboard(&line.path) {
            self.inspect_error = Some(format!("{:#}", e));
        }
    }

    fn open_logs_state(&mut self, id: String) {
        self.logs_loading = true;
        self.logs_error = None;
        self.logs_text = None;
        self.logs_for_id = Some(id);
        self.logs_cursor = 0;
        self.logs_scroll_top = 0;
        self.logs_select_anchor = None;
        self.logs_hscroll = 0;
        self.logs_max_width = 0;
        self.logs_mode = LogsMode::Normal;
        self.logs_input.clear();
        self.logs_query.clear();
        self.logs_command.clear();
        self.logs_regex = None;
        self.logs_regex_error = None;
        self.logs_match_lines.clear();
        self.logs_show_line_numbers = false;
    }

    fn logs_move_up(&mut self, by: usize) {
        self.logs_cursor = self.logs_cursor.saturating_sub(by);
    }

    fn logs_move_down(&mut self, by: usize) {
        let total = self.logs_total_lines();
        if total == 0 {
            self.logs_cursor = 0;
            return;
        }
        self.logs_cursor = self.logs_cursor.saturating_add(by).min(total - 1);
    }

    fn logs_total_lines(&self) -> usize {
        self.logs_text
            .as_ref()
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    fn logs_toggle_selection(&mut self) {
        if self.logs_select_anchor.take().is_none() {
            self.logs_select_anchor = Some(self.logs_cursor);
        }
    }

    fn logs_clear_selection(&mut self) {
        self.logs_select_anchor = None;
    }

    fn logs_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.logs_select_anchor?;
        let a = anchor.min(self.logs_cursor);
        let b = anchor.max(self.logs_cursor);
        Some((a, b))
    }

    fn logs_copy_selection(&mut self) {
        let Some(text) = self.logs_text.as_deref() else {
            self.set_warn("no logs loaded");
            return;
        };

        let total = self.logs_total_lines();
        if total == 0 {
            self.set_warn("no logs loaded");
            return;
        }

        let (start, end) = self
            .logs_selection_range()
            .unwrap_or((self.logs_cursor, self.logs_cursor));
        let start = start.min(total.saturating_sub(1));
        let end = end.min(total.saturating_sub(1));

        let mut out = String::new();
        for (i, line) in text.lines().enumerate() {
            if i < start {
                continue;
            }
            if i > end {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }

        if out.is_empty() {
            self.set_warn("nothing to copy");
            return;
        }

        if let Err(e) = copy_to_clipboard(&out) {
            self.set_error(format!("{e:#}"));
        } else {
            let count = end.saturating_sub(start) + 1;
            self.set_info(format!("copied {count} line(s) to clipboard"));
            self.logs_clear_selection();
        }
    }

    fn logs_rebuild_matches(&mut self) {
        let q = match self.logs_mode {
            LogsMode::Search => self.logs_input.trim(),
            LogsMode::Normal | LogsMode::Command => self.logs_query.trim(),
        };
        if q.is_empty() {
            self.logs_match_lines.clear();
            self.logs_regex = None;
            self.logs_regex_error = None;
            return;
        }

        let Some(text) = &self.logs_text else {
            self.logs_match_lines.clear();
            return;
        };

        if self.logs_use_regex {
            match RegexBuilder::new(q).case_insensitive(true).build() {
                Ok(re) => {
                    self.logs_regex = Some(re);
                    self.logs_regex_error = None;
                }
                Err(e) => {
                    self.logs_regex = None;
                    self.logs_regex_error = Some(format!("{e}"));
                    self.logs_match_lines.clear();
                    return;
                }
            }

            let Some(re) = self.logs_regex.as_ref() else {
                self.logs_match_lines.clear();
                return;
            };
            self.logs_match_lines = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| if re.is_match(line) { Some(i) } else { None })
                .collect();
        } else {
            self.logs_regex = None;
            self.logs_regex_error = None;
            let q_lc = q.to_ascii_lowercase();
            self.logs_match_lines = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    if line.to_ascii_lowercase().contains(&q_lc) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
        }
    }

    fn logs_commit_search(&mut self) {
        self.logs_query = self.logs_input.clone();
        self.logs_mode = LogsMode::Normal;
        self.logs_input.clear();
        self.logs_input_cursor = 0;
        self.logs_rebuild_matches();
        if let Some(first) = self.logs_match_lines.first().copied() {
            self.logs_cursor = first;
        }
    }

    fn logs_cancel_search(&mut self) {
        self.logs_mode = LogsMode::Normal;
        self.logs_input.clear();
        self.logs_input_cursor = 0;
        self.logs_rebuild_matches();
    }

    fn logs_next_match(&mut self) {
        if self.logs_mode != LogsMode::Normal {
            return;
        }
        if self.logs_match_lines.is_empty() {
            return;
        }
        let cur = self.logs_cursor;
        let next = self
            .logs_match_lines
            .iter()
            .copied()
            .find(|&i| i > cur)
            .or_else(|| self.logs_match_lines.first().copied())
            .unwrap();
        self.logs_cursor = next;
    }

    fn logs_prev_match(&mut self) {
        if self.logs_mode != LogsMode::Normal {
            return;
        }
        if self.logs_match_lines.is_empty() {
            return;
        }
        let cur = self.logs_cursor;
        let prev = self
            .logs_match_lines
            .iter()
            .copied()
            .rfind(|&i| i < cur)
            .or_else(|| self.logs_match_lines.last().copied())
            .unwrap();
        self.logs_cursor = prev;
    }

    fn selected_image(&self) -> Option<&ImageRow> {
        let idx = self.images_visible_index_at(self.images_selected)?;
        self.images.get(idx)
    }

    fn selected_volume(&self) -> Option<&VolumeRow> {
        let idx = self.volumes_visible_index_at(self.volumes_selected)?;
        self.volumes.get(idx)
    }

    fn selected_network(&self) -> Option<&NetworkRow> {
        self.networks.get(self.networks_selected)
    }

    fn is_system_network(n: &NetworkRow) -> bool {
        // Docker/system-managed networks that should not be modified from the UI.
        // - Default networks: bridge/host/none
        // - Swarm: ingress, docker_gwbridge
        matches!(
            n.name.as_str(),
            "bridge" | "host" | "none" | "ingress" | "docker_gwbridge"
        )
    }

    fn is_system_network_id(&self, id: &str) -> bool {
        self.networks
            .iter()
            .find(|n| n.id == id)
            .map(App::is_system_network)
            .unwrap_or(false)
    }

    fn images_visible_index_at(&self, pos: usize) -> Option<usize> {
        if !self.images_unused_only {
            if pos < self.images.len() {
                return Some(pos);
            }
            return None;
        }
        self.images
            .iter()
            .enumerate()
            .filter(|(_, img)| !self.image_referenced(img))
            .nth(pos)
            .map(|(i, _)| i)
    }

    fn images_visible_len(&self) -> usize {
        if !self.images_unused_only {
            self.images.len()
        } else {
            self.images
                .iter()
                .filter(|img| !self.image_referenced(img))
                .count()
        }
    }

    fn volumes_visible_index_at(&self, pos: usize) -> Option<usize> {
        if !self.volumes_unused_only {
            if pos < self.volumes.len() {
                return Some(pos);
            }
            return None;
        }
        self.volumes
            .iter()
            .enumerate()
            .filter(|(_, v)| !self.volume_referenced(v))
            .nth(pos)
            .map(|(i, _)| i)
    }

    fn volumes_visible_len(&self) -> usize {
        if !self.volumes_unused_only {
            self.volumes.len()
        } else {
            self.volumes
                .iter()
                .filter(|v| !self.volume_referenced(v))
                .count()
        }
    }

    fn selected_inspect_target(&self) -> Option<InspectTarget> {
        match self.active_view {
            ActiveView::Containers => {
                let c = self.selected_container()?;
                Some(InspectTarget {
                    kind: InspectKind::Container,
                    key: c.id.clone(),
                    arg: c.id.clone(),
                    label: c.name.clone(),
                })
            }
            ActiveView::Images => {
                let img = self.selected_image()?;
                Some(InspectTarget {
                    kind: InspectKind::Image,
                    key: img.id.clone(),
                    arg: img.id.clone(),
                    label: img.name(),
                })
            }
            ActiveView::Volumes => {
                let v = self.selected_volume()?;
                Some(InspectTarget {
                    kind: InspectKind::Volume,
                    key: v.name.clone(),
                    arg: v.name.clone(),
                    label: v.name.clone(),
                })
            }
            ActiveView::Networks => {
                let n = self.selected_network()?;
                Some(InspectTarget {
                    kind: InspectKind::Network,
                    key: n.id.clone(),
                    arg: n.id.clone(),
                    label: n.name.clone(),
                })
            }
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
    volume_ref_count_by_name: HashMap<String, usize>,
    volume_run_count_by_name: HashMap<String, usize>,
    volume_containers_by_name: HashMap<String, Vec<String>>,
    ip_by_container_id: HashMap<String, String>,
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
    mouse_enabled: bool,
    ascii_only: bool,
) -> anyhow::Result<()> {
    let mut terminal = setup_terminal(mouse_enabled).context("failed to setup terminal")?;
    let (theme_spec, theme_err) = match theme::load_theme(&config_path, &active_theme) {
        Ok(t) => (t, None),
        Err(e) => (theme::default_theme_spec(), Some(e)),
    };
    let mut app = App::new(
        servers,
        keymap,
        active_server,
        config_path,
        view_layout,
        active_theme,
        theme_spec,
    );
    if let Some(e) = theme_err {
        app.log_msg(MsgLevel::Warn, format!("failed to load theme: {:#}", e));
    }
    app.current_target = runner.key();
    app.mouse_enabled = mouse_enabled;
    app.ascii_only = ascii_only;
    app.refresh_secs = refresh.as_secs().max(1);
    app.logs_tail = logs_tail.max(1);
    app.cmd_history_max = cmd_history_max.clamp(1, 5000);
    app.set_cmd_history_entries(cmd_history);
    app.templates_dir = expand_user_path(&templates_dir);
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
    let (action_res_tx, mut action_res_rx) =
        mpsc::unbounded_channel::<(ActionRequest, anyhow::Result<String>)>();

    let (logs_req_tx, mut logs_req_rx) = mpsc::unbounded_channel::<(String, usize)>();
    let (logs_res_tx, mut logs_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<String>)>();

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

    let (refresh_interval_tx, refresh_interval_rx) =
        watch::channel(Duration::from_secs(app.refresh_secs.max(1)));
    let fetch_task = tokio::spawn(async move {
        let mut refresh_interval_rx = refresh_interval_rx;
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
              changed = conn_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
            }

            let conn = conn_rx.borrow().clone();
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
                let child = child_opt.take().expect("child already taken");
                child.wait_with_output().await
              } => out,
              changed = conn_rx.changed() => {
                // Server switch: kill the in-flight SSH command to avoid waiting
                // for slow "docker stats" on the old server.
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
    let action_task = tokio::spawn(async move {
        let action_conn_rx = action_conn_rx;
        while let Some(req) = action_req_rx.recv().await {
            let conn = action_conn_rx.borrow().clone();
            let res = match &req {
                ActionRequest::Container { action, id } => {
                    docker::container_action(&conn.runner, &conn.docker, *action, id).await
                }
                ActionRequest::TemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_compose,
                } => {
                    async {
                        let remote_dir = match runner {
                            Runner::Local => {
                                let home = std::env::var("HOME")
                                    .map_err(|_| anyhow::anyhow!("HOME is not set"))?;
                                format!("{home}/.config/containr/apps/{name}")
                            }
                            Runner::Ssh(_) => deploy_remote_dir_for(name),
                        };
                        let remote_compose = format!("{remote_dir}/compose.yaml");
                        let mkdir_cmd = format!("mkdir -p {remote_dir}");
                        let up_cmd =
                            format!("cd {remote_dir} && {} compose up -d", docker.docker_cmd);
                        runner.run(&mkdir_cmd).await?;
                        runner.copy_file_to(local_compose, &remote_compose).await?;
                        let out = runner.run(&up_cmd).await?;
                        Ok::<_, anyhow::Error>(out)
                    }
                    .await
                }
                ActionRequest::NetTemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_cfg,
                    force,
                } => {
                    async {
                        let raw = fs::read_to_string(local_cfg)
                            .with_context(|| format!("failed to read {}", local_cfg.display()))?;
                        let spec: NetworkTemplateSpec =
                            serde_json::from_str(&raw).context("network.json was not valid JSON")?;
                        let net_name = spec.name.trim();
                        anyhow::ensure!(!net_name.is_empty(), "network template: name is empty");

                        let remote_dir = match runner {
                            Runner::Local => {
                                let home = std::env::var("HOME")
                                    .map_err(|_| anyhow::anyhow!("HOME is not set"))?;
                                format!("{home}/.config/containr/networks/{name}")
                            }
                            Runner::Ssh(_) => deploy_remote_net_dir_for(name),
                        };
                        let remote_cfg = format!("{remote_dir}/network.json");
                        let mkdir_cmd = format!("mkdir -p {remote_dir}");
                        runner.run(&mkdir_cmd).await?;
                        runner.copy_file_to(local_cfg, &remote_cfg).await?;

                        let docker_cmd = &docker.docker_cmd;
                        let net_q = shell_single_quote(net_name);
                        let exists_cmd = format!("{docker_cmd} network inspect {net_q} >/dev/null 2>&1");
                        let exists = runner.run(&exists_cmd).await.is_ok();
                        if exists && !*force {
                            return Ok::<_, anyhow::Error>("exists".to_string());
                        }
                        if exists && *force {
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

                        // Driver-specific helpers.
                        if driver == "ipvlan" {
                            let parent = spec.parent.as_deref().unwrap_or("").trim();
                            anyhow::ensure!(!parent.is_empty(), "ipvlan requires 'parent'");
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
                        let out = runner.run(&create_cmd).await?;
                        Ok::<_, anyhow::Error>(out)
                    }
                    .await
                }
                ActionRequest::ImageUntag { reference, .. } => {
                    docker::image_remove(&conn.runner, &conn.docker, reference).await
                }
                ActionRequest::ImageForceRemove { id, .. } => {
                    docker::image_remove_force(&conn.runner, &conn.docker, id).await
                }
                ActionRequest::VolumeRemove { name } => {
                    docker::volume_remove(&conn.runner, &conn.docker, name).await
                }
                ActionRequest::NetworkRemove { id } => {
                    docker::network_remove(&conn.runner, &conn.docker, id).await
                }
            };
            let _ = action_res_tx.send((req, res));
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

                for v in snapshot.volume_containers_by_name.values_mut() {
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

    loop {
        if app.should_quit {
            break;
        }
        // Avoid stale "in-progress" markers if the background action result gets lost.
        let now = Instant::now();
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
                    app.clear_conn_error();
                    app.clear_last_error();
                }
                Err(e) => {
                    app.loading = false;
                    app.loading_since = None;
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
                    for img in &app.images {
                        let id = normalize_image_id(&img.id);
                        let refs = snap.image_ref_count_by_id.get(&id).copied().unwrap_or(0);
                        let runs = snap.image_run_count_by_id.get(&id).copied().unwrap_or(0);
                        app.image_referenced_by_id.insert(img.id.clone(), refs > 0);
                        app.image_referenced_count_by_id
                            .insert(img.id.clone(), refs);
                        app.image_running_count_by_id.insert(img.id.clone(), runs);
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

                    // Clamp selections in case the unused-only toggles depend on usage.
                    app.images_selected = app
                        .images_selected
                        .min(app.images_visible_len().saturating_sub(1));
                    app.volumes_selected = app
                        .volumes_selected
                        .min(app.volumes_visible_len().saturating_sub(1));
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
            if app.inspect_for_id.as_deref() != Some(&id) {
                continue;
            }
            app.inspect_loading = false;
            match res {
                Ok(value) => {
                    app.inspect_value = Some(value);
                    app.inspect_error = None;
                    app.rebuild_inspect_lines();
                }
                Err(e) => {
                    app.inspect_value = None;
                    let msg = format!("{:#}", e);
                    app.inspect_error = Some(msg.clone());
                    app.log_msg(MsgLevel::Error, format!("inspect failed: {msg}"));
                    app.rebuild_inspect_lines();
                }
            }
        }

        while let Ok((req, res)) = action_res_rx.try_recv() {
            match res {
                Ok(out) => {
                    app.clear_last_error();
                    if let ActionRequest::TemplateDeploy { name, .. } = &req {
                        app.template_deploy_inflight.remove(name);
                        app.set_info(format!("deployed template {name}"));
                    }
                    if let ActionRequest::NetTemplateDeploy { name, .. } = &req {
                        app.net_template_deploy_inflight.remove(name);
                        if out.trim() == "exists" {
                            app.set_warn(format!(
                                "network '{name}' already exists (use :nettemplate deploy! to recreate)"
                            ));
                        } else {
                            app.set_info(format!("deployed network template {name}"));
                        }
                    }
                    // Keep container "in-flight" markers for a short time; the next refresh will
                    // replace the status. For other kinds we just refresh.
                    let _ = refresh_tx.send(());
                }
                Err(e) => {
                    match &req {
                        ActionRequest::Container { id, .. } => {
                            app.action_inflight.remove(id);
                        }
                        ActionRequest::TemplateDeploy { name, .. } => {
                            app.template_deploy_inflight.remove(name);
                            app.set_error(format!("deploy failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::NetTemplateDeploy { name, .. } => {
                            app.net_template_deploy_inflight.remove(name);
                            app.set_error(format!("deploy failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::ImageUntag { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                        }
                        ActionRequest::ImageForceRemove { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                        }
                        ActionRequest::VolumeRemove { name } => {
                            app.volume_action_inflight.remove(name);
                        }
                        ActionRequest::NetworkRemove { id } => {
                            app.network_action_inflight.remove(id);
                        }
                    }
                    app.set_error(format!("{:#}", e));
                }
            }
        }

        while let Ok((id, res)) = logs_res_rx.try_recv() {
            if app.logs_for_id.as_deref() != Some(&id) {
                continue;
            }
            app.logs_loading = false;
            match res {
                Ok(text) => {
                    app.logs_max_width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                    app.logs_text = Some(text);
                    app.logs_error = None;
                    if app.logs_cursor >= app.logs_total_lines() {
                        app.logs_cursor = app.logs_total_lines().saturating_sub(1);
                    }
                    app.logs_rebuild_matches();
                }
                Err(e) => {
                    app.logs_text = None;
                    let msg = format!("{:#}", e);
                    app.logs_error = Some(msg.clone());
                    app.log_msg(MsgLevel::Error, format!("logs failed: {msg}"));
                    app.logs_cursor = 0;
                    app.logs_hscroll = 0;
                    app.logs_max_width = 0;
                    app.logs_rebuild_matches();
                }
            }
        }

        let refresh_display = Duration::from_secs(app.refresh_secs.max(1));
        terminal.draw(|f| draw(f, &mut app, refresh_display))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    handle_shell_key(
                        &mut app,
                        key,
                        &conn_tx,
                        &refresh_tx,
                        &refresh_interval_tx,
                        &inspect_req_tx,
                        &logs_req_tx,
                        &action_req_tx,
                    );
                    if let Some(req) = app.shell_pending_interactive.take() {
                        let runner = current_runner_from_app(&app);
                        restore_terminal(&mut terminal, mouse_enabled)?;
                        let res = match req {
                            ShellInteractive::RunCommand { cmd } => {
                                run_interactive_command(&runner, &cmd)
                            }
                            ShellInteractive::RunLocalCommand { cmd } => {
                                run_interactive_local_command(&cmd)
                            }
                        };
                        terminal = setup_terminal(mouse_enabled)?;
                        if let Some(name) = app.templates_refresh_after_edit.take() {
                            app.refresh_templates();
                            if let Some(idx) = app.templates.iter().position(|t| t.name == name) {
                                app.templates_selected = idx;
                            }
                        }
                        if let Some(name) = app.net_templates_refresh_after_edit.take() {
                            app.refresh_net_templates();
                            if let Some(idx) = app.net_templates.iter().position(|t| t.name == name)
                            {
                                app.net_templates_selected = idx;
                            }
                        }
                        if let Some(name) = app.theme_refresh_after_edit.take() {
                            commands::theme_cmd::reload_active_theme_after_edit(&mut app, &name);
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
    inspect_task.abort();
    action_task.abort();
    logs_task.abort();
    ip_task.abort();
    usage_task.abort();
    restore_terminal(&mut terminal, mouse_enabled).context("failed to restore terminal")?;
    Ok(())
}

fn setup_terminal(mouse_enabled: bool) -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse_enabled {
        execute!(stdout, EnableMouseCapture)?;
    }
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

fn shell_single_quote(s: &str) -> String {
    // Produce a POSIX-shell-safe single-quoted string literal.
    // Example: abc'd -> 'abc'"'"'d'
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn current_runner_from_app(app: &App) -> Runner {
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

fn current_docker_cmd_from_app(app: &App) -> String {
    if let Some(name) = &app.active_server {
        if let Some(s) = app.servers.iter().find(|x| &x.name == name) {
            return s.docker_cmd.clone();
        }
    }
    "docker".to_string()
}

fn current_server_label(app: &App) -> String {
    app.active_server
        .as_deref()
        .unwrap_or_else(|| app.current_target.as_str())
        .to_string()
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mouse_enabled: bool,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    if mouse_enabled {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

include!("render.inc.rs");
