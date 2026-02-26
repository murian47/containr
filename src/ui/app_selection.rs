use std::collections::HashSet;
use std::time::Instant;

use crate::docker::{ContainerAction, ContainerRow, ImageRow, NetworkRow, VolumeRow};
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::render::utils::is_container_stopped;

use super::{
    ActiveView, App, InspectKind, InspectTarget, ListMode, ViewEntry,
};

impl App {
    pub(super) fn selected_container(&self) -> Option<&ContainerRow> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        match self.list_mode {
            ListMode::Flat => self.containers.get(self.selected),
            ListMode::Tree => {
                let Some(entry) = self.view.get(self.selected) else {
                    return None;
                };
                let ViewEntry::Container { id, .. } = entry else {
                    return None;
                };
                let idx = self.container_idx_by_id.get(id)?;
                self.containers.get(*idx)
            }
        }
    }

    pub(super) fn selected_stack(&self) -> Option<(&str, usize, usize, bool)> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        if self.list_mode != ListMode::Tree {
            return None;
        }
        let Some(entry) = self.view.get(self.selected) else {
            return None;
        };
        match entry {
            ViewEntry::StackHeader {
                name,
                total,
                running,
                expanded,
            } => Some((name.as_str(), *total, *running, *expanded)),
            ViewEntry::UngroupedHeader { total, running } => {
                Some(("Ungrouped", *total, *running, true))
            }
            _ => None,
        }
    }

    pub(super) fn selected_stack_container_ids(&mut self) -> Option<Vec<String>> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        if self.list_mode != ListMode::Tree {
            return None;
        }
        self.ensure_view();
        let Some(entry) = self.view.get(self.selected) else {
            return None;
        };
        let ViewEntry::StackHeader { name, .. } = entry else {
            return None;
        };
        let stack = name.clone();
        let mut ids: Vec<String> = self
            .containers
            .iter()
            .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack.as_str()))
            .map(|c| c.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        Some(ids)
    }

    pub(super) fn container_ids_for_selection(&mut self) -> Vec<String> {
        if let Some(ids) = self.selected_stack_container_ids() {
            return ids;
        }
        if !self.marked.is_empty() {
            return self.marked.iter().cloned().collect();
        }
        self.selected_container()
            .map(|c| vec![c.id.clone()])
            .unwrap_or_default()
    }

    pub(super) fn view_len(&mut self) -> usize {
        if self.active_view != ActiveView::Containers {
            return 0;
        }
        self.ensure_view();
        match self.list_mode {
            ListMode::Flat => self.containers.len(),
            ListMode::Tree => self.view.len(),
        }
    }

    pub(super) fn ensure_view(&mut self) {
        if self.active_view != ActiveView::Containers {
            return;
        }
        if self.list_mode != ListMode::Tree {
            self.view.clear();
            self.view_dirty = false;
            return;
        }
        if !self.view_dirty {
            return;
        }
        self.view_dirty = false;
        self.rebuild_tree_view();
    }

    pub(super) fn current_anchor(&self) -> Option<(String, Option<String>)> {
        // (container_id, stack_name) where stack_name is Some only if selection is a stack header.
        match self.list_mode {
            ListMode::Flat => self.selected_container().map(|c| (c.id.clone(), None)),
            ListMode::Tree => match self.view.get(self.selected) {
                Some(ViewEntry::Container { id, .. }) => Some((id.clone(), None)),
                Some(ViewEntry::StackHeader { name, .. }) => {
                    Some(("".to_string(), Some(name.clone())))
                }
                Some(ViewEntry::UngroupedHeader { .. }) => {
                    Some(("".to_string(), Some("Ungrouped".to_string())))
                }
                None => None,
            },
        }
    }

    pub(super) fn rebuild_tree_view(&mut self) {
        use std::collections::BTreeMap;

        let anchor = self.current_anchor();

        let mut stacks: BTreeMap<String, Vec<&ContainerRow>> = BTreeMap::new();
        let mut ungrouped: Vec<&ContainerRow> = Vec::new();
        for c in &self.containers {
            if let Some(stack) = stack_name_from_labels(&c.labels) {
                stacks.entry(stack).or_default().push(c);
            } else {
                ungrouped.push(c);
            }
        }

        let mut out: Vec<ViewEntry> = Vec::new();

        for (name, mut cs) in stacks {
            cs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            let total = cs.len();
            let running = cs.iter().filter(|c| !is_container_stopped(&c.status)).count();
            let expanded = !self.stack_collapsed.contains(&name);
            out.push(ViewEntry::StackHeader {
                name: name.clone(),
                total,
                running,
                expanded,
            });
            if expanded {
                for c in cs {
                    out.push(ViewEntry::Container {
                        id: c.id.clone(),
                        indent: 2,
                    });
                }
            }
        }

        if !ungrouped.is_empty() {
            let total = ungrouped.len();
            let running = ungrouped
                .iter()
                .filter(|c| !is_container_stopped(&c.status))
                .count();
            out.push(ViewEntry::UngroupedHeader { total, running });
            ungrouped.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            for c in ungrouped {
                out.push(ViewEntry::Container {
                    id: c.id.clone(),
                    indent: 2,
                });
            }
        }

        self.view = out;

        // Restore selection when possible.
        if let Some((id, stack)) = anchor {
            if !id.is_empty() {
                if let Some(idx) = self
                    .view
                    .iter()
                    .position(|e| matches!(e, ViewEntry::Container { id: cid, .. } if cid == &id))
                {
                    self.selected = idx;
                    return;
                }
            }
            if let Some(stack) = stack {
                if let Some(idx) = self.view.iter().position(
                    |e| matches!(e, ViewEntry::StackHeader { name, .. } if name == &stack),
                ) {
                    self.selected = idx;
                    return;
                }
            }
        }
        if self.selected >= self.view.len() {
            self.selected = self.view.len().saturating_sub(1);
        }
    }

    pub(super) fn toggle_tree_expanded_selected(&mut self) -> bool {
        if self.active_view != ActiveView::Containers || self.list_mode != ListMode::Tree {
            return false;
        }
        self.ensure_view();
        let Some(entry) = self.view.get(self.selected).cloned() else {
            return false;
        };
        match entry {
            ViewEntry::StackHeader { name, .. } => {
                if !self.stack_collapsed.insert(name.clone()) {
                    self.stack_collapsed.remove(&name);
                }
                self.view_dirty = true;
                self.ensure_view();
                true
            }
            _ => false,
        }
    }

    pub(super) fn is_marked(&self, id: &str) -> bool {
        self.marked.contains(id)
    }

    pub(super) fn is_image_marked(&self, key: &str) -> bool {
        self.marked_images.contains(key)
    }

    pub(super) fn is_volume_marked(&self, name: &str) -> bool {
        self.marked_volumes.contains(name)
    }

    pub(super) fn is_network_marked(&self, id: &str) -> bool {
        self.marked_networks.contains(id)
    }

    pub(super) fn toggle_mark_selected(&mut self) {
        match self.active_view {
            ActiveView::Stacks => {}
            ActiveView::Containers => {
                let Some(id) = self.selected_container().map(|c| c.id.clone()) else {
                    return;
                };
                if !self.marked.remove(&id) {
                    self.marked.insert(id);
                }
            }
            ActiveView::Images => {
                let Some(img) = self.selected_image() else {
                    return;
                };
                let key = App::image_row_key(img);
                if !self.marked_images.remove(&key) {
                    self.marked_images.insert(key);
                }
            }
            ActiveView::Volumes => {
                let Some(name) = self.selected_volume().map(|v| v.name.clone()) else {
                    return;
                };
                if !self.marked_volumes.remove(&name) {
                    self.marked_volumes.insert(name);
                }
            }
            ActiveView::Networks => {
                let Some(id) = self.selected_network().map(|n| n.id.clone()) else {
                    return;
                };
                if !self.marked_networks.remove(&id) {
                    self.marked_networks.insert(id);
                }
            }
        }
    }

    pub(super) fn mark_all(&mut self) {
        match self.active_view {
            ActiveView::Stacks => {}
            ActiveView::Containers => {
                for c in &self.containers {
                    self.marked.insert(c.id.clone());
                }
            }
            ActiveView::Images => {
                if self.images_unused_only {
                    for img in &self.images {
                        if !self.image_referenced(img) {
                            self.marked_images.insert(App::image_row_key(img));
                        }
                    }
                } else {
                    for img in &self.images {
                        self.marked_images.insert(App::image_row_key(img));
                    }
                }
            }
            ActiveView::Volumes => {
                if self.volumes_unused_only {
                    for v in &self.volumes {
                        if !self.volume_referenced(v) {
                            self.marked_volumes.insert(v.name.clone());
                        }
                    }
                } else {
                    for v in &self.volumes {
                        self.marked_volumes.insert(v.name.clone());
                    }
                }
            }
            ActiveView::Networks => {
                for n in &self.networks {
                    self.marked_networks.insert(n.id.clone());
                }
            }
        }
    }

    pub(super) fn clear_marks(&mut self) {
        match self.active_view {
            ActiveView::Stacks => {}
            ActiveView::Containers => self.marked.clear(),
            ActiveView::Images => self.marked_images.clear(),
            ActiveView::Volumes => self.marked_volumes.clear(),
            ActiveView::Networks => self.marked_networks.clear(),
        }
    }

    pub(super) fn clear_all_marks(&mut self) {
        self.marked.clear();
        self.marked_images.clear();
        self.marked_volumes.clear();
        self.marked_networks.clear();
    }

    pub(super) fn prune_marks(&mut self) {
        if self.marked.is_empty() || self.containers.is_empty() {
            if self.containers.is_empty() {
                // Keep marks during transient loading; they will be pruned after we have data again.
            }
            return;
        }
        let present: HashSet<&str> = self.containers.iter().map(|c| c.id.as_str()).collect();
        self.marked.retain(|id| present.contains(id.as_str()));
    }

    pub(super) fn prune_image_marks(&mut self) {
        if self.marked_images.is_empty() || self.images.is_empty() {
            if self.images.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<String> = self.images.iter().map(App::image_row_key).collect();
        self.marked_images.retain(|k| present.contains(k));
    }

    pub(super) fn prune_volume_marks(&mut self) {
        if self.marked_volumes.is_empty() || self.volumes.is_empty() {
            if self.volumes.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<&str> = self.volumes.iter().map(|v| v.name.as_str()).collect();
        self.marked_volumes
            .retain(|name| present.contains(name.as_str()));
    }

    pub(super) fn prune_network_marks(&mut self) {
        if self.marked_networks.is_empty() || self.networks.is_empty() {
            if self.networks.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<&str> = self.networks.iter().map(|n| n.id.as_str()).collect();
        self.marked_networks
            .retain(|id| present.contains(id.as_str()));
    }

    pub(super) fn move_up(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                if self.view_len() == 0 {
                    self.selected = 0;
                    return;
                }
                self.selected = self.selected.saturating_sub(1);
            }
            ActiveView::Stacks => {
                if self.stacks.is_empty() {
                    self.stacks_selected = 0;
                } else {
                    self.stacks_selected = self.stacks_selected.saturating_sub(1);
                }
            }
            ActiveView::Images => self.images_selected = self.images_selected.saturating_sub(1),
            ActiveView::Volumes => self.volumes_selected = self.volumes_selected.saturating_sub(1),
            ActiveView::Networks => {
                self.networks_selected = self.networks_selected.saturating_sub(1)
            }
        }
    }

    pub(super) fn move_down(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                if self.view_len() == 0 {
                    self.selected = 0;
                    return;
                }
                self.selected = (self.selected + 1).min(self.view_len().saturating_sub(1));
            }
            ActiveView::Stacks => {
                if self.stacks.is_empty() {
                    self.stacks_selected = 0;
                } else {
                    self.stacks_selected =
                        (self.stacks_selected + 1).min(self.stacks.len().saturating_sub(1));
                }
            }
            ActiveView::Images => {
                if self.images_visible_len() == 0 {
                    self.images_selected = 0;
                } else {
                    self.images_selected = (self.images_selected + 1).min(self.images_visible_len() - 1);
                }
            }
            ActiveView::Volumes => {
                if self.volumes_visible_len() == 0 {
                    self.volumes_selected = 0;
                } else {
                    self.volumes_selected =
                        (self.volumes_selected + 1).min(self.volumes_visible_len() - 1);
                }
            }
            ActiveView::Networks => {
                if self.networks.is_empty() {
                    self.networks_selected = 0;
                } else {
                    self.networks_selected = (self.networks_selected + 1).min(self.networks.len() - 1);
                }
            }
        }
    }

    pub(super) fn set_containers(&mut self, containers: Vec<ContainerRow>) {
        self.containers = containers;
        self.container_idx_by_id.clear();
        for (i, c) in self.containers.iter().enumerate() {
            self.container_idx_by_id.insert(c.id.clone(), i);
        }
        self.rebuild_stacks();
        self.prune_template_deploys_for_active_server();
        self.loading = false;
        self.loading_since = None;
        self.ip_refresh_needed = true;
        self.prune_marks();
        self.view_dirty = true;
        self.reconcile_action_markers();
        self.ensure_view();
        let max = match self.list_mode {
            ListMode::Flat => self.containers.len(),
            ListMode::Tree => self.view.len(),
        };
        if self.selected >= max {
            self.selected = max.saturating_sub(1);
        }
    }

    pub(super) fn reconcile_noncontainer_action_markers(&mut self) {
        let now = Instant::now();
        let present_image_ids: HashSet<&str> = self.images.iter().map(|i| i.id.as_str()).collect();
        let present_image_refs: HashSet<String> = self.images.iter().map(App::image_row_key).collect();
        self.image_action_inflight.retain(|k, m| {
            if now >= m.until {
                return false;
            }
            if k.starts_with("ref:") {
                return present_image_refs.contains(k);
            }
            // Fallback: allow raw image IDs to keep markers across tag changes.
            present_image_ids.contains(k.as_str()) || present_image_refs.contains(k)
        });
        self.image_action_error.retain(|k, _| {
            if k.starts_with("ref:") {
                present_image_refs.contains(k)
            } else {
                present_image_ids.contains(k.as_str()) || present_image_refs.contains(k)
            }
        });
        let present_vols: HashSet<&str> = self.volumes.iter().map(|v| v.name.as_str()).collect();
        self.volume_action_inflight
            .retain(|name, m| now < m.until && present_vols.contains(name.as_str()));
        self.volume_action_error
            .retain(|name, _| present_vols.contains(name.as_str()));
        let present_nets: HashSet<&str> = self.networks.iter().map(|n| n.id.as_str()).collect();
        self.network_action_inflight
            .retain(|id, m| now < m.until && present_nets.contains(id.as_str()));
        self.network_action_error
            .retain(|id, _| present_nets.contains(id.as_str()));
    }

    pub(super) fn image_referenced(&self, img: &ImageRow) -> bool {
        self.image_referenced_by_id
            .get(&img.id)
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn volume_referenced(&self, v: &VolumeRow) -> bool {
        self.volume_referenced_by_name
            .get(&v.name)
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn reconcile_action_markers(&mut self) {
        // The docker start/stop/restart command may return before docker ps reflects the new state.
        // Keep showing the marker until we observe a matching state, or until the marker expires.
        let now = Instant::now();
        let present: HashSet<&str> = self.containers.iter().map(|c| c.id.as_str()).collect();
        self.container_action_error
            .retain(|id, _| present.contains(id.as_str()));
        self.action_inflight.retain(|id, marker| {
            if now >= marker.until {
                return false;
            }
            let Some(c) = self.containers.iter().find(|c| &c.id == id) else {
                // If it's gone, we consider the action done (or the container removed).
                return false;
            };
            let running = c.status.trim().starts_with("Up") || c.status.trim().starts_with("Restarting");
            let stopped = is_container_stopped(&c.status);
            match marker.action {
                ContainerAction::Start => !running,
                ContainerAction::Stop => !stopped,
                ContainerAction::Restart => !running,
                ContainerAction::Remove => true,
            }
        });
    }

    pub(super) fn start_loading(&mut self, clear_list: bool) {
        self.loading = true;
        self.loading_since = Some(Instant::now());
        self.clear_last_error();
        if clear_list {
            self.containers.clear();
            self.selected = 0;
            self.images.clear();
            self.volumes.clear();
            self.networks.clear();
            self.image_referenced_by_id.clear();
            self.image_referenced_count_by_id.clear();
            self.image_running_count_by_id.clear();
            self.volume_referenced_by_name.clear();
            self.volume_referenced_count_by_name.clear();
            self.volume_running_count_by_name.clear();
            self.volume_containers_by_name.clear();
            self.images_selected = 0;
            self.volumes_selected = 0;
            self.networks_selected = 0;
        }
    }

    pub(super) fn selected_image(&self) -> Option<&ImageRow> {
        let idx = self.images_visible_index_at(self.images_selected)?;
        self.images.get(idx)
    }

    pub(super) fn selected_volume(&self) -> Option<&VolumeRow> {
        let idx = self.volumes_visible_index_at(self.volumes_selected)?;
        self.volumes.get(idx)
    }

    pub(super) fn selected_network(&self) -> Option<&NetworkRow> {
        self.networks.get(self.networks_selected)
    }

    pub(super) fn is_system_network(n: &NetworkRow) -> bool {
        // Docker/system-managed networks that should not be modified from the UI.
        // - Default networks: bridge/host/none
        // - Swarm: ingress, docker_gwbridge
        matches!(
            n.name.as_str(),
            "bridge" | "host" | "none" | "ingress" | "docker_gwbridge"
        )
    }

    pub(super) fn is_system_network_id(&self, id: &str) -> bool {
        self.networks
            .iter()
            .find(|n| n.id == id)
            .map(App::is_system_network)
            .unwrap_or(false)
    }

    pub(super) fn images_visible_index_at(&self, pos: usize) -> Option<usize> {
        if !self.images_unused_only {
            if pos < self.images.len() {
                return Some(pos);
            }
            return None;
        }
        self.images
            .iter()
            .enumerate()
            .filter(|(_, img)| !self.image_referenced(img))
            .nth(pos)
            .map(|(i, _)| i)
    }

    pub(super) fn images_visible_len(&self) -> usize {
        if !self.images_unused_only {
            self.images.len()
        } else {
            self.images.iter().filter(|img| !self.image_referenced(img)).count()
        }
    }

    pub(super) fn volumes_visible_index_at(&self, pos: usize) -> Option<usize> {
        if !self.volumes_unused_only {
            if pos < self.volumes.len() {
                return Some(pos);
            }
            return None;
        }
        self.volumes
            .iter()
            .enumerate()
            .filter(|(_, v)| !self.volume_referenced(v))
            .nth(pos)
            .map(|(i, _)| i)
    }

    pub(super) fn volumes_visible_len(&self) -> usize {
        if !self.volumes_unused_only {
            self.volumes.len()
        } else {
            self.volumes.iter().filter(|v| !self.volume_referenced(v)).count()
        }
    }

    pub(super) fn selected_inspect_target(&self) -> Option<InspectTarget> {
        match self.active_view {
            ActiveView::Stacks => None,
            ActiveView::Containers => {
                let c = self.selected_container()?;
                Some(InspectTarget {
                    kind: InspectKind::Container,
                    key: c.id.clone(),
                    arg: c.id.clone(),
                    label: c.name.clone(),
                })
            }
            ActiveView::Images => {
                let img = self.selected_image()?;
                Some(InspectTarget {
                    kind: InspectKind::Image,
                    key: img.id.clone(),
                    arg: img.id.clone(),
                    label: img.name(),
                })
            }
            ActiveView::Volumes => {
                let v = self.selected_volume()?;
                Some(InspectTarget {
                    kind: InspectKind::Volume,
                    key: v.name.clone(),
                    arg: v.name.clone(),
                    label: v.name.clone(),
                })
            }
            ActiveView::Networks => {
                let n = self.selected_network()?;
                Some(InspectTarget {
                    kind: InspectKind::Network,
                    key: n.id.clone(),
                    arg: n.id.clone(),
                    label: n.name.clone(),
                })
            }
        }
    }
}
