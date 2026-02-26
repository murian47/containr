use anyhow::Context as _;
use crossterm::event::{self, Event, KeyEventKind};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::JoinSet;

use crate::config::{KeyBinding, ServerEntry};
use crate::docker::{self, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow};
use crate::runner::Runner;
use crate::services::image_update::ImageUpdateService;
use crate::ssh::Ssh;
use crate::ui::*;
use crate::ui::core::tasks::BackgroundTasks;

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
    let dash_all_task = tokio::spawn(async move {
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

    let tasks = BackgroundTasks {
        fetch_task,
        dash_task,
        dash_all_task,
        inspect_task,
        action_task,
        image_update_task,
        logs_task,
        ip_task,
        usage_task,
    };
    tasks.abort_all();
    restore_terminal(&mut terminal).context("failed to restore terminal")?;
    Ok(())
}
