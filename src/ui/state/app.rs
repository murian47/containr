//! Central in-memory UI state.
//!
//! `App` is the single mutable state object for the TUI. Rendering, input handling, background
//! task result application, and persistence all operate on this structure. Keep it as a state
//! container: orchestration belongs here, but heavy IO and rendering details belong in their
//! dedicated modules.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use crate::config::{self, KeyBinding, ServerEntry};
use crate::docker::{ContainerRow, ImageRow, NetworkRow, VolumeRow};
use crate::ui::core::key_types::{KeyScope, KeySpec};
use crate::ui::core::types::{
    ActionMarker, DashboardAllState, DashboardImageState, DashboardState, DeployMarker,
    ImageUpdateEntry, LastActionError, RateLimitEntry, RegistryAuthResolved, RegistryTestEntry,
    SimpleMarker, StackDetailsFocus, StackEntry, TemplateDeployEntry, ViewEntry,
};
use crate::ui::state::shell_types::{
    ActiveView, InspectState, ListMode, LogsState, SessionMsg, ShellCmdlineState, ShellFocus,
    ShellHelpState, ShellInteractive, ShellMessagesState, ShellSplitMode, ShellView,
    TemplatesState, ThemeSelectorState,
};
use crate::ui::theme;

pub(in crate::ui) struct App {
    // Core Docker inventory and derived usage/reference caches.
    pub(in crate::ui) containers: Vec<ContainerRow>,
    pub(in crate::ui) images: Vec<ImageRow>,
    pub(in crate::ui) volumes: Vec<VolumeRow>,
    pub(in crate::ui) networks: Vec<NetworkRow>,
    pub(in crate::ui) image_referenced_by_id: HashMap<String, bool>,
    pub(in crate::ui) image_referenced_count_by_id: HashMap<String, usize>,
    pub(in crate::ui) image_running_count_by_id: HashMap<String, usize>,
    pub(in crate::ui) image_containers_by_id: HashMap<String, Vec<String>>,
    pub(in crate::ui) volume_referenced_by_name: HashMap<String, bool>,
    pub(in crate::ui) volume_referenced_count_by_name: HashMap<String, usize>,
    pub(in crate::ui) volume_running_count_by_name: HashMap<String, usize>,
    pub(in crate::ui) volume_containers_by_name: HashMap<String, Vec<String>>,
    pub(in crate::ui) network_referenced_count_by_id: HashMap<String, usize>,
    pub(in crate::ui) network_containers_by_id: HashMap<String, Vec<String>>,
    pub(in crate::ui) images_unused_only: bool,
    pub(in crate::ui) volumes_unused_only: bool,
    pub(in crate::ui) usage_refresh_needed: bool,
    pub(in crate::ui) usage_loading: bool,
    pub(in crate::ui) selected: usize,
    pub(in crate::ui) active_view: ActiveView,
    pub(in crate::ui) list_mode: ListMode,
    pub(in crate::ui) view: Vec<ViewEntry>,
    pub(in crate::ui) view_dirty: bool,
    pub(in crate::ui) stack_collapsed: HashSet<String>,
    pub(in crate::ui) container_idx_by_id: HashMap<String, usize>,
    pub(in crate::ui) marked: HashSet<String>, // container ids
    pub(in crate::ui) marked_images: HashSet<String>, // image row keys (ref:repo:tag or id:<sha256..>)
    pub(in crate::ui) marked_volumes: HashSet<String>, // volume names
    pub(in crate::ui) marked_networks: HashSet<String>, // network ids
    pub(in crate::ui) images_selected: usize,
    pub(in crate::ui) volumes_selected: usize,
    pub(in crate::ui) networks_selected: usize,
    pub(in crate::ui) last_refresh: Option<Instant>,
    pub(in crate::ui) last_loop_at: Option<Instant>,
    pub(in crate::ui) reset_screen: bool,
    pub(in crate::ui) conn_error: Option<String>,
    pub(in crate::ui) last_error: Option<String>,
    pub(in crate::ui) loading: bool,
    pub(in crate::ui) loading_since: Option<Instant>,
    pub(in crate::ui) action_inflight: HashMap<String, ActionMarker>,
    pub(in crate::ui) image_action_inflight: HashMap<String, SimpleMarker>,
    pub(in crate::ui) volume_action_inflight: HashMap<String, SimpleMarker>,
    pub(in crate::ui) network_action_inflight: HashMap<String, SimpleMarker>,
    pub(in crate::ui) stack_update_inflight: HashMap<String, DeployMarker>,
    pub(in crate::ui) stack_update_containers: HashMap<String, Vec<String>>,
    pub(in crate::ui) container_action_error: HashMap<String, LastActionError>,
    pub(in crate::ui) image_action_error: HashMap<String, LastActionError>,
    pub(in crate::ui) volume_action_error: HashMap<String, LastActionError>,
    pub(in crate::ui) network_action_error: HashMap<String, LastActionError>,
    pub(in crate::ui) stack_update_error: HashMap<String, LastActionError>,
    pub(in crate::ui) template_action_error: HashMap<String, LastActionError>,
    pub(in crate::ui) net_template_action_error: HashMap<String, LastActionError>,
    pub(in crate::ui) inspect: InspectState,

    // Server/connection state. current_target is the effective runner target string and may differ
    // from active_server when the app is started without a named server entry.
    pub(in crate::ui) servers: Vec<ServerEntry>,
    pub(in crate::ui) active_server: Option<String>,
    pub(in crate::ui) server_selected: usize,
    pub(in crate::ui) server_all_selected: bool,
    pub(in crate::ui) config_path: std::path::PathBuf,
    pub(in crate::ui) current_target: String,

    pub(in crate::ui) logs: LogsState,
    pub(in crate::ui) dashboard: DashboardState,
    pub(in crate::ui) dashboard_all: DashboardAllState,
    pub(in crate::ui) dashboard_image: Option<DashboardImageState>,

    pub(in crate::ui) ip_cache: HashMap<String, (String, Instant)>,
    pub(in crate::ui) ip_refresh_needed: bool,
    pub(in crate::ui) should_quit: bool,
    pub(in crate::ui) ascii_only: bool,
    pub(in crate::ui) kitty_graphics: bool,

    pub(in crate::ui) theme_name: String,
    pub(in crate::ui) theme: theme::ThemeSpec,
    pub(in crate::ui) header_logo_seed: u64,

    // Shell-* fields drive the outer TUI chrome and focus model. active_view remains the semantic
    // list selection, while shell_view decides which top-level module or overlay is rendered.
    pub(in crate::ui) shell_view: ShellView,
    pub(in crate::ui) shell_last_main_view: ShellView,
    pub(in crate::ui) shell_focus: ShellFocus,
    pub(in crate::ui) shell_sidebar_collapsed: bool,
    pub(in crate::ui) shell_sidebar_hidden: bool,
    pub(in crate::ui) shell_sidebar_selected: usize,
    pub(in crate::ui) shell_split_mode: ShellSplitMode,
    pub(in crate::ui) shell_split_by_view: HashMap<String, ShellSplitMode>,
    pub(in crate::ui) shell_server_shortcuts: Vec<char>,
    pub(in crate::ui) shell_pending_interactive: Option<ShellInteractive>,
    pub(in crate::ui) shell_cmdline: ShellCmdlineState,
    pub(in crate::ui) shell_help: ShellHelpState,
    pub(in crate::ui) theme_selector: ThemeSelectorState,
    pub(in crate::ui) refresh_secs: u64,
    pub(in crate::ui) refresh_paused: bool,
    pub(in crate::ui) refresh_pause_reason: Option<String>,
    pub(in crate::ui) refresh_error_streak: u32,
    pub(in crate::ui) cmd_history_max: usize,
    pub(in crate::ui) git_autocommit: bool,
    pub(in crate::ui) git_autocommit_confirm: bool,
    pub(in crate::ui) editor_cmd: String,
    pub(in crate::ui) image_update_concurrency: usize,
    pub(in crate::ui) image_update_debug: bool,
    pub(in crate::ui) image_update_autocheck: bool,

    // Session messages and the optional dock are append-only UI history for the current process.
    pub(in crate::ui) session_msgs: Vec<SessionMsg>,
    pub(in crate::ui) messages_seen_len: usize,
    pub(in crate::ui) shell_msgs: ShellMessagesState,
    pub(in crate::ui) log_dock_enabled: bool,
    pub(in crate::ui) log_dock_height: u16,

    pub(in crate::ui) keymap: Vec<KeyBinding>,
    pub(in crate::ui) keymap_parsed: HashMap<(KeyScope, KeySpec), String>,
    pub(in crate::ui) keymap_defaults: HashMap<(KeyScope, KeySpec), String>,

    // Template/Git/registry state is persisted separately and mirrored here for quick rendering.
    pub(in crate::ui) templates_state: TemplatesState,
    pub(in crate::ui) image_updates: HashMap<String, ImageUpdateEntry>,
    pub(in crate::ui) image_updates_inflight: HashSet<String>,
    pub(in crate::ui) image_updates_path: PathBuf,
    pub(in crate::ui) rate_limits: HashMap<String, RateLimitEntry>,
    pub(in crate::ui) template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    pub(in crate::ui) net_template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    pub(in crate::ui) unknown_template_ids_warned: HashSet<String>,
    pub(in crate::ui) registries_cfg: config::RegistriesConfig,
    pub(in crate::ui) registry_auths: HashMap<String, RegistryAuthResolved>,
    pub(in crate::ui) registry_tests: HashMap<String, RegistryTestEntry>,

    pub(in crate::ui) theme_refresh_after_edit: Option<String>,

    pub(in crate::ui) stacks: Vec<StackEntry>,
    pub(in crate::ui) stacks_selected: usize,
    pub(in crate::ui) stacks_details_scroll: usize,
    pub(in crate::ui) stacks_networks_scroll: usize,
    pub(in crate::ui) stack_details_focus: StackDetailsFocus,
    pub(in crate::ui) stacks_only_running: bool,

    pub(in crate::ui) registries_selected: usize,
    pub(in crate::ui) registries_details_scroll: usize,

    pub(in crate::ui) container_details_scroll: usize,
    pub(in crate::ui) image_details_scroll: usize,
    pub(in crate::ui) volume_details_scroll: usize,
    pub(in crate::ui) network_details_scroll: usize,
    pub(in crate::ui) container_details_id: Option<String>,
    pub(in crate::ui) image_details_id: Option<String>,
    pub(in crate::ui) volume_details_id: Option<String>,
    pub(in crate::ui) network_details_id: Option<String>,
}
