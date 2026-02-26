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
mod app_registry_http;
mod app_secrets;
mod app_clock;
mod app_dashboard;
mod app_dashboard_data;
mod app_dashboard_image;
mod app_local_state;
mod app_template_labels;
mod app_server_switch;
mod app_keymap;
mod app_init;
mod app_ops;
mod app_runtime;
mod app_selection;
mod app_state;
mod app_stacks;
mod app_theme_selector;
mod app_templates;
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
pub(in crate::ui) use app_view::{shell_cycle_focus, shell_module_shortcut};
pub(in crate::ui) use helpers::{
    deploy_remote_dir_for, deploy_remote_net_dir_for, ensure_template_id, extract_container_ip,
    extract_template_id, parse_kv_args, shell_quote_with_home, shell_single_quote,
    build_server_shortcuts, normalize_image_id, truncate_msg,
};
pub(in crate::ui) use state::persistence::{ensure_unique_server_name, find_server_by_name};
pub(in crate::ui) use templates_ops::{
    create_net_template, create_template, delete_net_template, delete_template,
    export_net_template, export_stack_template, images_from_compose, maybe_autocommit_templates,
};
#[cfg(test)]
pub(crate) use crate::ui::commands::cmdline_cmd::parse_cmdline_tokens;
use render::highlight::{json_highlight_line, yaml_highlight_line};
pub(in crate::ui) use render::status::image_update_indicator;
use render::utils::{
    expand_user_path, is_container_stopped, shell_escape_sh_arg, shell_row_highlight,
};
use render::stacks::stack_name_from_labels;
use cmd_history::CmdHistory;
use app_ops::{perform_image_push, perform_net_template_deploy, perform_stack_update, perform_template_deploy};
use app_runtime::{current_docker_cmd_from_app, current_runner_from_app, current_server_label, restore_terminal, run_interactive_command, run_interactive_local_command, setup_terminal};
pub(in crate::ui) use app_clock::{now_local, now_unix};
use app_dashboard_data::{dashboard_command, parse_dashboard_output};
use app_registry_http::registry_test;
pub(in crate::ui) use app_dashboard_image::{
    apply_dashboard_theme, build_dashboard_image, init_dashboard_image,
};
pub(in crate::ui) use app_local_state::load_local_state;
pub(in crate::ui) use app_secrets::{
    decrypt_age_secret, encrypt_age_secret, ensure_age_identity, load_age_identities,
};
pub(in crate::ui) use app_template_labels::{
    render_compose_with_template_id, template_commit_from_labels, template_id_from_labels,
};
pub(in crate::ui) use app_keymap::{cmdline_is_destructive, is_single_letter_without_modifiers};
pub(in crate::ui) use text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};

use crate::config::{self, KeyBinding, ServerEntry};
use crate::docker::{
    self, ContainerAction, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow,
};
use crate::runner::Runner;
use crate::services::image_update::ImageUpdateService;
use crate::ssh::Ssh;
use anyhow::Context as _;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{
    collections::{HashMap, HashSet},
};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::task::JoinSet;

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

#[derive(Clone, Debug)]
struct Connection {
    runner: Runner,
    docker: DockerCfg,
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

#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "integration"))]
mod integration_tests;
