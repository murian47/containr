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
mod core;
mod features;
mod commands;
mod helpers;
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
pub(in crate::ui) use core::view::{shell_cycle_focus, shell_module_shortcut};
pub(in crate::ui) use helpers::{
    deploy_remote_dir_for, deploy_remote_net_dir_for, ensure_template_id, extract_container_ip,
    extract_template_id, parse_kv_args, shell_quote_with_home, shell_single_quote,
    build_server_shortcuts, normalize_image_id, truncate_msg,
};
pub(in crate::ui) use state::persistence::{ensure_unique_server_name, find_server_by_name};
pub(in crate::ui) use features::templates::{
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
use core::ops::{perform_image_push, perform_net_template_deploy, perform_stack_update, perform_template_deploy};
use core::runtime::{current_docker_cmd_from_app, current_runner_from_app, current_server_label, restore_terminal, run_interactive_command, run_interactive_local_command, setup_terminal};
pub use core::run::run_tui;
pub(in crate::ui) use core::clock::{now_local, now_unix};
pub(in crate::ui) use core::requests::{ActionRequest, Connection, ShellConfirm};
pub(in crate::ui) use core::types::{
    ActionErrorKind, ActionMarker, DashboardAllState, DashboardHostState, DashboardImageState,
    DashboardSnapshot, DashboardState, DeployMarker, DiskEntry, IMAGE_UPDATE_TTL_SECS,
    ImageUpdateEntry, ImageUpdateKind, InspectKind, InspectLine, InspectMode, InspectTarget,
    LastActionError, LocalState, LogsMode, NetTemplateEntry, NetworkTemplateIpv4,
    NetworkTemplateSpec, NicEntry, RATE_LIMIT_MAX, RATE_LIMIT_WARN, RATE_LIMIT_WINDOW_SECS,
    RateLimitEntry, RegistryAuthResolved, RegistryTestEntry, SimpleMarker, StackDetailsFocus,
    StackEntry, StackUpdateService, TemplateDeployEntry, TemplateEntry, UsageSnapshot, ViewEntry,
    classify_action_error,
};
use features::dashboard::{dashboard_command, parse_dashboard_output};
use features::registry::registry_test;
pub(in crate::ui) use core::secrets::{
    decrypt_age_secret, encrypt_age_secret, ensure_age_identity, load_age_identities,
};
pub(in crate::ui) use features::templates::{
    render_compose_with_template_id, template_commit_from_labels, template_id_from_labels,
};
pub(in crate::ui) use core::keymap::{cmdline_is_destructive, is_single_letter_without_modifiers};
pub(in crate::ui) use text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};

use crate::config::{self, KeyBinding, ServerEntry};
use crate::docker::{
    ContainerAction, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow,
};
use crate::runner::Runner;
use crossterm::{
    event::{KeyCode, KeyModifiers},
};
use ratatui_image::picker::Picker;
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
};
use time::OffsetDateTime;

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

#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "integration"))]
mod integration_tests;
