//! Stack-centric App helpers.

use crate::docker::ContainerRow;
use crate::ui::{App, StackEntry};
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::render::utils::is_container_stopped;

impl App {
    pub(in crate::ui) fn rebuild_stacks(&mut self) {
        use std::collections::BTreeMap;

        let mut stacks: BTreeMap<String, Vec<&ContainerRow>> = BTreeMap::new();
        for c in &self.containers {
            if let Some(stack) = stack_name_from_labels(&c.labels) {
                stacks.entry(stack).or_default().push(c);
            }
        }

        let mut out: Vec<StackEntry> = Vec::new();
        for (name, cs) in stacks {
            let total = cs.len();
            let running = cs.iter().filter(|c| !is_container_stopped(&c.status)).count();
            if self.stacks_only_running && running == 0 {
                continue;
            }
            out.push(StackEntry {
                name,
                total,
                running,
            });
        }

        self.stacks = out;
        if self.stacks_selected >= self.stacks.len() {
            self.stacks_selected = self.stacks.len().saturating_sub(1);
        }
    }

    pub(in crate::ui) fn selected_stack_entry(&self) -> Option<&StackEntry> {
        self.stacks.get(self.stacks_selected)
    }

    pub(in crate::ui) fn stack_container_ids(&self, name: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .containers
            .iter()
            .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(name))
            .map(|c| c.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub(in crate::ui) fn stack_container_count(&self, name: &str) -> usize {
        self.containers
            .iter()
            .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(name))
            .count()
    }

    pub(in crate::ui) fn stack_network_count(&self, name: &str) -> usize {
        self.networks
            .iter()
            .filter(|n| stack_name_from_labels(&n.labels).as_deref() == Some(name))
            .count()
    }

    pub(in crate::ui) fn stack_network_ids(&self, name: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .networks
            .iter()
            .filter(|n| stack_name_from_labels(&n.labels).as_deref() == Some(name))
            .filter(|n| !App::is_system_network(n))
            .map(|n| n.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}
