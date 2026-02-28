use anyhow::Context as _;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::JoinSet;

use crate::docker::{self, ContainerRow, ImageRow, NetworkRow, VolumeRow};
use crate::runner::Runner;
use crate::services::image_update::ImageUpdateService;
use crate::ssh::Ssh;
use crate::ui::core::background_ops::{
    perform_image_push, perform_net_template_deploy, perform_stack_update, perform_template_deploy,
};
use crate::ui::core::requests::{ActionRequest, Connection};
use crate::ui::core::tasks::BackgroundTasks;
use crate::ui::core::types::{DashboardSnapshot, InspectKind, InspectTarget, UsageSnapshot};
use crate::ui::features::dashboard::{dashboard_command, parse_dashboard_output};
use crate::ui::features::registry::registry_test;
use crate::ui::features::templates::{export_net_template, export_stack_template};
use crate::ui::shell_utils::{extract_container_ip, normalize_image_id};

type OverviewResult = anyhow::Result<(
    Vec<ContainerRow>,
    Vec<ImageRow>,
    Vec<VolumeRow>,
    Vec<NetworkRow>,
)>;

pub(in crate::ui) struct SpawnInputs {
    pub(in crate::ui) result_tx: mpsc::UnboundedSender<(String, OverviewResult)>,
    pub(in crate::ui) refresh_rx: mpsc::UnboundedReceiver<()>,
    pub(in crate::ui) inspect_req_rx: mpsc::UnboundedReceiver<InspectTarget>,
    pub(in crate::ui) inspect_res_tx: mpsc::UnboundedSender<(String, anyhow::Result<Value>)>,
    pub(in crate::ui) action_req_rx: mpsc::UnboundedReceiver<ActionRequest>,
    pub(in crate::ui) image_update_req_tx: mpsc::UnboundedSender<(String, bool)>,
    pub(in crate::ui) image_update_req_rx: mpsc::UnboundedReceiver<(String, bool)>,
    pub(in crate::ui) action_res_tx: mpsc::UnboundedSender<(ActionRequest, anyhow::Result<String>)>,
    pub(in crate::ui) logs_req_rx: mpsc::UnboundedReceiver<(String, usize)>,
    pub(in crate::ui) logs_res_tx: mpsc::UnboundedSender<(String, anyhow::Result<String>)>,
    pub(in crate::ui) dash_refresh_rx: mpsc::UnboundedReceiver<()>,
    pub(in crate::ui) dash_res_tx:
        mpsc::UnboundedSender<(String, anyhow::Result<DashboardSnapshot>)>,
    pub(in crate::ui) dash_all_refresh_rx: mpsc::UnboundedReceiver<()>,
    pub(in crate::ui) dash_all_res_tx:
        mpsc::UnboundedSender<(String, anyhow::Result<DashboardSnapshot>, u128)>,
    pub(in crate::ui) ip_req_rx: mpsc::UnboundedReceiver<Vec<String>>,
    pub(in crate::ui) ip_res_tx:
        mpsc::UnboundedSender<(String, anyhow::Result<HashMap<String, String>>)>,
    pub(in crate::ui) usage_req_rx: mpsc::UnboundedReceiver<Vec<String>>,
    pub(in crate::ui) usage_res_tx: mpsc::UnboundedSender<(String, anyhow::Result<UsageSnapshot>)>,
    pub(in crate::ui) conn_rx: watch::Receiver<Connection>,
    pub(in crate::ui) dash_all_enabled_rx: watch::Receiver<bool>,
    pub(in crate::ui) dash_all_servers_rx: watch::Receiver<Vec<crate::config::ServerEntry>>,
    pub(in crate::ui) refresh_interval_rx: watch::Receiver<Duration>,
    pub(in crate::ui) refresh_pause_rx: watch::Receiver<bool>,
    pub(in crate::ui) image_update_limit_rx: watch::Receiver<usize>,
}

pub(in crate::ui) fn spawn_background_tasks(inputs: SpawnInputs) -> BackgroundTasks {
    let SpawnInputs {
        result_tx,
        mut refresh_rx,
        mut inspect_req_rx,
        inspect_res_tx,
        mut action_req_rx,
        image_update_req_tx,
        mut image_update_req_rx,
        action_res_tx,
        mut logs_req_rx,
        logs_res_tx,
        mut dash_refresh_rx,
        dash_res_tx,
        mut dash_all_refresh_rx,
        dash_all_res_tx,
        mut ip_req_rx,
        ip_res_tx,
        mut usage_req_rx,
        usage_res_tx,
        conn_rx,
        dash_all_enabled_rx,
        dash_all_servers_rx,
        refresh_interval_rx,
        refresh_pause_rx,
        image_update_limit_rx,
    } = inputs;

    let fetch_conn_rx = conn_rx.clone();
    let fetch_refresh_interval_rx = refresh_interval_rx.clone();
    let fetch_pause_rx = refresh_pause_rx.clone();
    let fetch_task = tokio::spawn(async move {
        let mut refresh_interval_rx = fetch_refresh_interval_rx;
        let mut pause_rx = fetch_pause_rx;
        let mut interval = tokio::time::interval(*refresh_interval_rx.borrow());
        let mut conn_rx = fetch_conn_rx;
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
                    Err(std::io::Error::other("overview child already consumed"))
                }
              } => out,
              changed_res = conn_rx.changed() => {
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

    let dash_conn_rx = conn_rx.clone();
    let dash_refresh_interval_rx = refresh_interval_rx.clone();
    let dash_pause_rx = refresh_pause_rx.clone();
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

    let dash_all_refresh_interval_rx = refresh_interval_rx.clone();
    let dash_all_pause_rx = refresh_pause_rx.clone();
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

            if *pause_rx.borrow() || !*enabled_rx.borrow() {
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
                                let stderr =
                                    String::from_utf8_lossy(&out.stderr).trim().to_string();
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
                    let _ = tx.send((s.name.clone(), res, latency_ms));
                });
            }
            while set.join_next().await.is_some() {}
        }
    });

    let inspect_conn_rx = conn_rx.clone();
    let inspect_task = tokio::spawn(async move {
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

    let action_conn_rx = conn_rx.clone();
    let action_res_tx_action = action_res_tx.clone();
    let action_task = tokio::spawn(async move {
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
                } => registry_test(host, auth, test_repo.as_deref()).await,
                ActionRequest::TemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_compose,
                    pull,
                    force_recreate,
                    template_commit,
                    ..
                } => {
                    perform_template_deploy(
                        runner,
                        docker,
                        name,
                        local_compose,
                        *pull,
                        *force_recreate,
                        template_commit.as_deref(),
                    )
                    .await
                }
                ActionRequest::StackUpdate {
                    stack_name,
                    runner,
                    docker,
                    compose_dirs,
                    pull,
                    dry,
                    force,
                    services,
                } => {
                    perform_stack_update(
                        runner,
                        docker,
                        stack_name,
                        compose_dirs,
                        *pull,
                        *dry,
                        *force,
                        services,
                    )
                    .await
                }
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
                } => {
                    export_stack_template(
                        &conn.runner,
                        &conn.docker,
                        name,
                        source,
                        Some(stack_name),
                        container_ids,
                        templates_dir,
                    )
                    .await
                }
                ActionRequest::TemplateFromContainer {
                    name,
                    source,
                    container_id,
                    templates_dir,
                } => {
                    export_stack_template(
                        &conn.runner,
                        &conn.docker,
                        name,
                        source,
                        None,
                        std::slice::from_ref(container_id),
                        templates_dir,
                    )
                    .await
                }
                ActionRequest::TemplateFromNetwork {
                    name,
                    source,
                    network_id,
                    templates_dir,
                } => {
                    export_net_template(
                        &conn.runner,
                        &conn.docker,
                        name,
                        source,
                        network_id,
                        templates_dir,
                    )
                    .await
                }
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
                } => {
                    perform_image_push(
                        &conn.runner,
                        &conn.docker,
                        source_ref,
                        target_ref,
                        registry_host,
                        auth.as_ref(),
                    )
                    .await
                }
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

    let image_update_conn_rx = conn_rx.clone();
    let image_update_res_tx = action_res_tx.clone();
    let image_update_task = tokio::spawn(async move {
        let mut image_update_limit_rx = image_update_limit_rx;
        let mut semaphore = Arc::new(Semaphore::new((*image_update_limit_rx.borrow()).max(1)));
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
                        let _ = image_update_res_tx.send((ActionRequest::ImageUpdateCheck { image, debug }, res));
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

    let logs_conn_rx = conn_rx.clone();
    let logs_task = tokio::spawn(async move {
        while let Some((id, tail)) = logs_req_rx.recv().await {
            let conn = logs_conn_rx.borrow().clone();
            let res = docker::fetch_logs(&conn.runner, &conn.docker, &id, tail.max(1)).await;
            let _ = logs_res_tx.send((id, res));
        }
    });

    let ip_conn_rx = conn_rx.clone();
    let ip_task = tokio::spawn(async move {
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

    let usage_conn_rx = conn_rx;
    let usage_task = tokio::spawn(async move {
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

    BackgroundTasks {
        fetch_task,
        dash_task,
        dash_all_task,
        inspect_task,
        action_task,
        image_update_task,
        logs_task,
        ip_task,
        usage_task,
    }
}
