use anyhow::Context as _;
use crossterm::event::{self, Event, KeyEventKind};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

use crate::config::{KeyBinding, ServerEntry};
use crate::docker::DockerCfg;
use crate::runner::Runner;
use crate::ui::commands::theme_cmd;
use crate::ui::core::run_apply::process_background_updates;
use crate::ui::core::run_spawn::{SpawnInputs, spawn_background_tasks};
use crate::ui::input;
use crate::ui::theme;
use crate::{config, ui};
use ui::{
    ActionRequest, App, Connection, ContainerRow, DashboardSnapshot, ImageRow, InspectTarget,
    MsgLevel, NetworkRow, Picker, ShellInteractive, TemplatesKind, UsageSnapshot, VolumeRow,
    current_runner_from_app, draw, expand_user_path, maybe_autocommit_templates, restore_terminal,
    run_interactive_command, run_interactive_local_command, setup_terminal,
};

#[allow(
    clippy::collapsible_if,
    clippy::single_match,
    clippy::too_many_arguments
)]
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
        app.log_msg(
            MsgLevel::Warn,
            format!("failed to load registries: {:#}", e),
        );
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
    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel::<()>();

    let (inspect_req_tx, inspect_req_rx) = mpsc::unbounded_channel::<InspectTarget>();
    let (inspect_res_tx, mut inspect_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<Value>)>();

    let (action_req_tx, action_req_rx) = mpsc::unbounded_channel::<ActionRequest>();
    let (image_update_req_tx, image_update_req_rx) = mpsc::unbounded_channel::<(String, bool)>();
    let (action_res_tx, mut action_res_rx) =
        mpsc::unbounded_channel::<(ActionRequest, anyhow::Result<String>)>();

    let (logs_req_tx, logs_req_rx) = mpsc::unbounded_channel::<(String, usize)>();
    let (logs_res_tx, mut logs_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<String>)>();

    let (dash_refresh_tx, dash_refresh_rx) = mpsc::unbounded_channel::<()>();
    let (dash_res_tx, mut dash_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<DashboardSnapshot>)>();
    let (dash_all_refresh_tx, dash_all_refresh_rx) = mpsc::unbounded_channel::<()>();
    let (dash_all_res_tx, mut dash_all_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<DashboardSnapshot>, u128)>();

    let (ip_req_tx, ip_req_rx) = mpsc::unbounded_channel::<Vec<String>>();
    let (ip_res_tx, mut ip_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<HashMap<String, String>>)>();

    let (usage_req_tx, usage_req_rx) = mpsc::unbounded_channel::<Vec<String>>();
    let (usage_res_tx, mut usage_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<UsageSnapshot>)>();

    let (conn_tx, _conn_rx) = watch::channel(Connection {
        runner: runner.clone(),
        docker: cfg.clone(),
    });
    let (dash_all_enabled_tx, dash_all_enabled_rx) = watch::channel(false);
    let (_dash_all_servers_tx, dash_all_servers_rx) = watch::channel(app.servers.clone());

    let (refresh_interval_tx, _refresh_interval_rx) =
        watch::channel(Duration::from_secs(app.refresh_secs.max(1)));
    let (refresh_pause_tx, _refresh_pause_rx) = watch::channel(false);
    let (image_update_limit_tx, image_update_limit_rx) =
        watch::channel(app.image_update_concurrency.max(1));
    let tasks = spawn_background_tasks(SpawnInputs {
        result_tx,
        refresh_rx,
        inspect_req_rx,
        inspect_res_tx,
        action_req_rx,
        image_update_req_tx,
        image_update_req_rx,
        action_res_tx: action_res_tx.clone(),
        logs_req_rx,
        logs_res_tx,
        dash_refresh_rx,
        dash_res_tx,
        dash_all_refresh_rx,
        dash_all_res_tx,
        ip_req_rx,
        ip_res_tx,
        usage_req_rx,
        usage_res_tx,
        conn_rx: conn_tx.subscribe(),
        dash_all_enabled_rx: dash_all_enabled_rx.clone(),
        dash_all_servers_rx,
        refresh_interval_rx: refresh_interval_tx.subscribe(),
        refresh_pause_rx: refresh_pause_tx.subscribe(),
        image_update_limit_rx,
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

        process_background_updates(
            &mut app,
            &mut result_rx,
            &mut ip_res_rx,
            &mut dash_res_rx,
            &mut dash_all_res_rx,
            &mut usage_res_rx,
            &mut inspect_res_rx,
            &mut action_res_rx,
            &mut logs_res_rx,
            &ip_req_tx,
            &usage_req_tx,
            &action_req_tx,
            &refresh_tx,
            &refresh_pause_tx,
            ERROR_PAUSE_THRESHOLD,
        );

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
                            theme_cmd::reload_active_theme_after_edit(&mut app, &name);
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

    tasks.abort_all();
    restore_terminal(&mut terminal).context("failed to restore terminal")?;
    Ok(())
}
