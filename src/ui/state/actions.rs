use crate::docker::ContainerAction;
use crate::ui::{
    App, ActionMarker, ActionRequest, SimpleMarker, template_id_from_labels,
};
use std::collections::HashSet;
use tokio::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) fn exec_stack_action(
    app: &mut App,
    action: ContainerAction,
    name: Option<&str>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let stack_name = if let Some(name) = name {
        name.trim().to_string()
    } else {
        app.selected_stack_entry()
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };
    if stack_name.trim().is_empty() {
        app.set_warn("no stack selected");
        return;
    }
    if !app.stacks.iter().any(|s| s.name == stack_name) {
        app.set_warn(format!("unknown stack: {stack_name}"));
        return;
    }
    let ids = app.stack_container_ids(&stack_name);
    if ids.is_empty() {
        app.set_warn("no containers in stack");
        return;
    }
    let remove_networks = matches!(action, ContainerAction::Remove);
    if remove_networks {
        let server = app.active_server.clone().unwrap_or_default();
    let ids: HashSet<String> = app
        .containers
        .iter()
        .filter(|c| crate::ui::render::stacks::stack_name_from_labels(&c.labels).as_deref() == Some(stack_name.as_str()))
        .filter_map(|c| template_id_from_labels(&c.labels))
        .collect();
        let mut changed = false;
        for id in ids {
            if app.remove_template_deploys_for_server(&id, &server) {
                changed = true;
            }
        }
        if changed {
            app.save_local_state();
        }
    }
    let network_ids = if remove_networks {
        app.stack_network_ids(&stack_name)
    } else {
        Vec::new()
    };
    let info = if network_ids.is_empty() {
        format!("stack {stack_name}: {} containers", ids.len())
    } else {
        format!(
            "stack {stack_name}: {} containers, {} networks",
            ids.len(),
            network_ids.len()
        )
    };
    app.set_info(info);
    let now = Instant::now();
    for id in ids {
        app.action_inflight.insert(
            id.clone(),
            ActionMarker {
                action,
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::Container { action, id });
    }
    if remove_networks {
        for id in network_ids {
            app.network_action_inflight.insert(
                id.clone(),
                SimpleMarker {
                    until: now + Duration::from_secs(120),
                },
            );
            let _ = action_req_tx.send(ActionRequest::NetworkRemove { id });
        }
    }
}

pub(crate) fn exec_image_action(
    app: &mut App,
    untag: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let keys: Vec<String> = if !app.marked_images.is_empty() {
        app.marked_images.iter().cloned().collect()
    } else {
        app.selected_image()
            .map(|img| vec![App::image_row_key(img)])
            .unwrap_or_default()
    };
    if keys.is_empty() {
        app.set_warn("no images selected");
        return;
    }

    let now = Instant::now();
    for key in keys {
        let (id, reference) = if let Some(ref_str) = key.strip_prefix("ref:") {
            let ref_str = ref_str.to_string();
            let id = app
                .images
                .iter()
                .find(|i| App::image_row_ref(i).as_deref() == Some(ref_str.as_str()))
                .map(|i| i.id.clone())
                .unwrap_or_default();
            (id, Some(ref_str))
        } else if let Some(id) = key.strip_prefix("id:") {
            (id.to_string(), None)
        } else {
            (key.clone(), None)
        };
        if id.is_empty() {
            app.set_error("failed to resolve image id");
            continue;
        }

        let marker_key = if untag {
            reference
                .as_ref()
                .map(|r| format!("ref:{}", r))
                .unwrap_or_else(|| format!("id:{}", id))
        } else {
            format!("id:{}", id)
        };
        app.image_action_inflight.insert(
            marker_key.clone(),
            SimpleMarker {
                until: now + Duration::from_secs(120),
            },
        );
        if untag {
            let Some(reference) = reference else {
                app.image_action_inflight.remove(&marker_key);
                app.set_warn("cannot untag by ID; select a repo:tag row or use Remove");
                continue;
            };
            let _ = action_req_tx.send(ActionRequest::ImageUntag {
                marker_key,
                reference,
            });
        } else {
            let _ = action_req_tx.send(ActionRequest::ImageForceRemove { marker_key, id });
        }
    }
}

pub(crate) fn exec_volume_remove(
    app: &mut App,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let names: Vec<String> = if !app.marked_volumes.is_empty() {
        app.marked_volumes.iter().cloned().collect()
    } else {
        app.selected_volume()
            .map(|v| vec![v.name.clone()])
            .unwrap_or_default()
    };
    if names.is_empty() {
        app.set_warn("no volumes selected");
        return;
    }
    let now = Instant::now();
    for name in names {
        app.volume_action_inflight.insert(
            name.clone(),
            SimpleMarker {
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::VolumeRemove { name });
    }
}

pub(crate) fn exec_network_remove(
    app: &mut App,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let ids: Vec<String> = if !app.marked_networks.is_empty() {
        app.marked_networks.iter().cloned().collect()
    } else {
        app.selected_network()
            .map(|n| vec![n.id.clone()])
            .unwrap_or_default()
    };
    let ids: Vec<String> = ids
        .into_iter()
        .filter(|id| !app.is_system_network_id(id))
        .collect();
    if ids.is_empty() {
        app.set_warn("no networks selected");
        return;
    }
    let now = Instant::now();
    for id in ids {
        app.network_action_inflight.insert(
            id.clone(),
            SimpleMarker {
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::NetworkRemove { id });
    }
}

pub(crate) fn exec_container_action(
    app: &mut App,
    action: ContainerAction,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let ids: Vec<String> = if let Some(ids) = app.selected_stack_container_ids() {
        ids
    } else if !app.marked.is_empty() {
        app.marked.iter().cloned().collect()
    } else {
        app.selected_container()
            .map(|c| vec![c.id.clone()])
            .unwrap_or_default()
    };
    if ids.is_empty() {
        app.set_warn("no containers selected");
        return;
    }
    let now = Instant::now();
    for id in ids {
        app.action_inflight.insert(
            id.clone(),
            ActionMarker {
                action,
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::Container { action, id });
    }
}
