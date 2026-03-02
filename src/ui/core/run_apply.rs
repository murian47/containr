use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

use crate::docker::{ContainerRow, ImageRow, NetworkRow, VolumeRow};
use crate::ui::core::clock::{now_local, now_unix};
use crate::ui::core::requests::ActionRequest;
use crate::ui::core::types::{
    DashboardSnapshot, ImageUpdateEntry, ImageUpdateKind, LastActionError, RegistryTestEntry,
    TemplateDeployEntry, UsageSnapshot, classify_action_error,
};
use crate::ui::features::templates::images_from_compose;
use crate::ui::render::utils::is_container_stopped;
use crate::ui::shell_utils::{normalize_image_id, truncate_msg};
use crate::ui::state::app::App;
use crate::ui::state::image_updates::{ImageUpdateResult, is_rate_limit_error};
use crate::ui::state::shell_types::MsgLevel;
use crate::ui::ui_actions;

type OverviewResult = anyhow::Result<(
    Vec<ContainerRow>,
    Vec<ImageRow>,
    Vec<VolumeRow>,
    Vec<NetworkRow>,
)>;

fn is_transient_missing_object_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}").to_ascii_lowercase();
    msg.contains("no such object:")
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn process_background_updates(
    app: &mut App,
    result_rx: &mut mpsc::UnboundedReceiver<(String, OverviewResult)>,
    ip_res_rx: &mut mpsc::UnboundedReceiver<(String, anyhow::Result<HashMap<String, String>>)>,
    dash_res_rx: &mut mpsc::UnboundedReceiver<(String, anyhow::Result<DashboardSnapshot>)>,
    dash_all_res_rx: &mut mpsc::UnboundedReceiver<(
        String,
        anyhow::Result<DashboardSnapshot>,
        u128,
    )>,
    usage_res_rx: &mut mpsc::UnboundedReceiver<(String, anyhow::Result<UsageSnapshot>)>,
    inspect_res_rx: &mut mpsc::UnboundedReceiver<(String, anyhow::Result<Value>)>,
    action_res_rx: &mut mpsc::UnboundedReceiver<(ActionRequest, anyhow::Result<String>)>,
    logs_res_rx: &mut mpsc::UnboundedReceiver<(String, anyhow::Result<String>)>,
    ip_req_tx: &mpsc::UnboundedSender<Vec<String>>,
    usage_req_tx: &mpsc::UnboundedSender<Vec<String>>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    refresh_pause_tx: &watch::Sender<bool>,
    error_pause_threshold: u32,
) {
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
                if app.refresh_error_streak >= error_pause_threshold {
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
                if !is_transient_missing_object_error(&e) {
                    app.set_warn(format!("ip lookup failed: {:#}", e));
                }
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
        let host = app.dashboard_all.hosts.iter_mut().find(|h| h.name == name);
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
                let now = Instant::now();
                for (id, ip) in snap.ip_by_container_id {
                    app.ip_cache.insert(id, (ip, now));
                }

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
                if !is_transient_missing_object_error(&e) {
                    app.set_warn(format!("usage lookup failed: {:#}", e));
                }
            }
        }
    }

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
                                ui_actions::check_image_updates(app, images, action_req_tx);
                            }
                        }
                    }
                    ActionRequest::StackUpdate {
                        stack_name, dry, ..
                    } => {
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
                            app.log_msg(MsgLevel::Info, format!("{label} for {stack_name}:"));
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
                    ActionRequest::NetTemplateDeploy {
                        name, server_name, ..
                    } => {
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
                        if let Some(server_name) = app.active_server.clone()
                            && !server_name.trim().is_empty()
                        {
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
                    ActionRequest::TemplateFromStack {
                        name, stack_name, ..
                    } => {
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
                                let local = result.entry.local_digest.as_deref().unwrap_or("-");
                                let remote = result.entry.remote_digest.as_deref().unwrap_or("-");
                                let mut msg = format!(
                                    "image update result: {} status={} local={} remote={}",
                                    result.image, status, local, remote
                                );
                                if let Some(note) = result.entry.note.as_deref() {
                                    msg.push_str(&format!(" note={note}"));
                                }
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
                                app.image_updates.insert(result.image.clone(), result.entry);
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
                            note: None,
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
}
