use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui_image::picker::Picker;

use crate::config::{self, KeyBinding, ServerEntry};
use crate::ui::cmd_history::CmdHistory;
use crate::ui::core::types::{
    DashboardAllState, DashboardHostState, DashboardState, IMAGE_UPDATE_TTL_SECS, InspectMode,
    LogsMode, RATE_LIMIT_WINDOW_SECS, StackDetailsFocus,
};
use crate::ui::helpers::build_server_shortcuts;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{
    ActiveView, InspectState, ListMode, LogsState, ShellCmdlineState, ShellFocus, ShellHelpState,
    ShellMessagesState, ShellSplitMode, ShellView, TemplatesKind, TemplatesState,
    ThemeSelectorState,
};
use crate::ui::theme;

use crate::ui::core::clock::now_unix;
use crate::ui::core::local_state::load_local_state;

impl App {
    pub(in crate::ui) fn new(
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
        if let Some(name) = &active_server
            && let Some(idx) = servers.iter().position(|s| &s.name == name)
        {
            server_selected = idx;
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
            v.hits
                .retain(|ts| now.saturating_sub(*ts) <= RATE_LIMIT_WINDOW_SECS);
            if let Some(until) = v.limited_until
                && now >= until
            {
                v.limited_until = None;
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
                dashboard_picker
                    .map(|p| crate::ui::features::dashboard::init_dashboard_image(p, &theme))
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
}
