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
pub(in crate::ui) use state::shell_types::{
    input_window_with_cursor, shell_begin_confirm, ActiveView, GitRemoteStatus, InspectState,
    ListMode, LogsState, MsgLevel, SessionMsg, ShellAction, ShellCmdlineState, ShellFocus,
    ShellHelpState, ShellInteractive, ShellMessagesState, ShellSidebarItem, ShellSplitMode,
    ShellView, TemplateEditSnapshot, TemplatesKind, TemplatesState, ThemeSelectorState,
};
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
pub(in crate::ui) use core::key_types::{
    BindingHit, KeyCodeNorm, KeyScope, KeySpec, build_default_keymap, key_spec_from_event,
    lookup_binding, lookup_scoped_binding, parse_key_spec, parse_scope, scope_to_string,
};
pub(in crate::ui) use features::templates::{
    render_compose_with_template_id, template_commit_from_labels, template_id_from_labels,
};
pub(in crate::ui) use core::keymap::{cmdline_is_destructive, is_single_letter_without_modifiers};
pub(in crate::ui) use text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};
pub(in crate::ui) use cmd_history::CmdHistory;

use crate::config::{self, KeyBinding, ServerEntry};
use crate::docker::{
    ContainerAction, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow,
};
use crate::runner::Runner;
use ratatui_image::picker::Picker;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
};
use std::path::PathBuf;

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
#[path = "../tests/ui_tests.rs"]
mod tests;
#[cfg(all(test, feature = "integration"))]
#[path = "../tests/ui_integration_tests.rs"]
mod integration_tests;
