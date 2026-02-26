use std::collections::HashSet;
use std::fs;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

use super::{
    App, GitRemoteStatus, MsgLevel, NetTemplateEntry, TemplateEditSnapshot, TemplateEntry,
    TemplatesKind,
};
use crate::ui::templates_ops::{extract_net_template_description, extract_template_description};
use crate::ui::{commands, extract_template_id};

impl App {
    pub(super) fn refresh_templates(&mut self) {
        self.templates_state.templates_error = None;
        self.templates_state.templates.clear();
        self.templates_state.templates_details_scroll = 0;
        self.templates_state.git_head = commands::git_cmd::git_head_short(&self.templates_state.dir);
        self.refresh_template_git_status();

        self.migrate_templates_layout_if_needed();

        let dir = self.stack_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.templates_state.templates_error = Some(format!("failed to create templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.templates_state.templates_error =
                    Some(format!("failed to read templates dir: {e}"));
                return;
            }
        };

        let mut out: Vec<TemplateEntry> = Vec::new();
        for ent in entries.flatten() {
            let path = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let compose_path = path.join("compose.yaml");
            let has_compose = compose_path.exists();
            let desc = if has_compose {
                extract_template_description(&compose_path).unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            let template_id = if has_compose {
                extract_template_id(&compose_path)
            } else {
                None
            };
            out.push(TemplateEntry {
                name,
                dir: path,
                compose_path,
                has_compose,
                desc,
                template_id,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.templates_state.templates = out;
        if self.templates_state.templates_selected >= self.templates_state.templates.len() {
            self.templates_state.templates_selected =
                self.templates_state.templates.len().saturating_sub(1);
        }
        for t in &self.templates_state.templates {
            let Some(id) = t.template_id.as_ref() else {
                continue;
            };
            if self.template_deploys.contains_key(id) {
                continue;
            }
            if let Some(list) = self.template_deploys.remove(&t.name) {
                self.template_deploys.insert(id.clone(), list);
            }
        }
        let known: HashSet<String> = self
            .templates_state
            .templates
            .iter()
            .filter_map(|t| t.template_id.clone())
            .collect();
        self.template_deploys.retain(|id, _| known.contains(id));
    }

    pub(super) fn selected_template(&self) -> Option<&TemplateEntry> {
        self.templates_state
            .templates
            .get(self.templates_state.templates_selected)
    }

    pub(super) fn net_templates_dir(&self) -> PathBuf {
        self.templates_state.dir.join("networks")
    }

    pub(super) fn stack_templates_dir(&self) -> PathBuf {
        self.templates_state.dir.join("stacks")
    }

    pub(super) fn migrate_templates_layout_if_needed(&mut self) {
        // Old layout: <templates_dir>/<name>/compose.yaml and <templates_dir>/networks/...
        // New layout: <templates_dir>/stacks/<name>/compose.yaml and <templates_dir>/networks/...
        let stacks = self.stack_templates_dir();
        if stacks.exists() {
            return;
        }
        let root = self.templates_state.dir.clone();
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut to_move: Vec<(String, PathBuf)> = Vec::new();
        for ent in entries.flatten() {
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "networks" || name == "stacks" {
                continue;
            }
            to_move.push((name, ent.path()));
        }
        if to_move.is_empty() {
            return;
        }
        if let Err(e) = fs::create_dir_all(&stacks) {
            self.log_msg(
                MsgLevel::Warn,
                format!(
                    "failed to create stacks templates dir '{}': {e}",
                    stacks.display()
                ),
            );
            return;
        }
        for (name, from) in to_move {
            let to = stacks.join(&name);
            if to.exists() {
                self.log_msg(
                    MsgLevel::Warn,
                    format!(
                        "template migration skipped: '{}' already exists in stacks/",
                        name
                    ),
                );
                continue;
            }
            if let Err(e) = fs::rename(&from, &to) {
                self.log_msg(
                    MsgLevel::Warn,
                    format!("template migration failed for '{}': {}", name, e),
                );
            }
        }
    }

    pub(super) fn refresh_net_templates(&mut self) {
        self.templates_state.net_templates_error = None;
        self.templates_state.net_templates.clear();
        self.templates_state.net_templates_details_scroll = 0;
        self.refresh_template_git_status();

        self.migrate_templates_layout_if_needed();

        let dir = self.net_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.templates_state.net_templates_error =
                Some(format!("failed to create net templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.templates_state.net_templates_error =
                    Some(format!("failed to read net templates dir: {e}"));
                return;
            }
        };

        let mut out: Vec<NetTemplateEntry> = Vec::new();
        for ent in entries.flatten() {
            let path = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let cfg_path = path.join("network.json");
            let has_cfg = cfg_path.exists();
            let desc = if has_cfg {
                extract_net_template_description(&cfg_path).unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            out.push(NetTemplateEntry {
                name,
                dir: path,
                cfg_path,
                has_cfg,
                desc,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.templates_state.net_templates = out;
        if self.templates_state.net_templates_selected >= self.templates_state.net_templates.len() {
            self.templates_state.net_templates_selected =
                self.templates_state.net_templates.len().saturating_sub(1);
        }
    }

    pub(super) fn refresh_template_git_status(&mut self) {
        self.templates_state.dirty_templates.clear();
        self.templates_state.dirty_net_templates.clear();
        self.templates_state.untracked_templates.clear();
        self.templates_state.untracked_net_templates.clear();
        self.templates_state.git_remote_templates.clear();
        self.templates_state.git_remote_net_templates.clear();
        let dir = self.templates_state.dir.clone();
        if !commands::git_cmd::is_git_repo(&dir) {
            return;
        }
        let out = match commands::git_cmd::run_git(&dir, &["status", "--porcelain", "-uall"]) {
            Ok(out) => out,
            Err(e) => {
                self.log_msg(MsgLevel::Warn, format!("git status failed: {:#}", e));
                return;
            }
        };
        for line in out.lines() {
            let untracked = line.starts_with("??");
            let path = parse_git_status_path(line);
            let Some(path) = path else { continue };
            if let Some(rest) = path.strip_prefix("stacks/") {
                if let Some(name) = rest.split('/').next() && !name.trim().is_empty() {
                    self.templates_state.dirty_templates.insert(name.to_string());
                    if untracked {
                        self.templates_state.untracked_templates.insert(name.to_string());
                    }
                }
            } else if let Some(rest) = path.strip_prefix("networks/")
                && let Some(name) = rest.split('/').next()
                && !name.trim().is_empty()
            {
                self.templates_state
                    .dirty_net_templates
                    .insert(name.to_string());
                if untracked {
                    self.templates_state
                        .untracked_net_templates
                        .insert(name.to_string());
                }
            }
        }

        if commands::git_cmd::run_git(
            &dir,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .is_err()
        {
            return;
        }

        let stacks_dir = dir.join("stacks");
        if let Ok(entries) = fs::read_dir(&stacks_dir) {
            for ent in entries.flatten() {
                let Ok(ft) = ent.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let rel = format!("stacks/{name}");
                let status = git_remote_status_for_path(&dir, &rel);
                self.templates_state.git_remote_templates.insert(name, status);
            }
        }

        let nets_dir = dir.join("networks");
        if let Ok(entries) = fs::read_dir(&nets_dir) {
            for ent in entries.flatten() {
                let Ok(ft) = ent.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let rel = format!("networks/{name}");
                let status = git_remote_status_for_path(&dir, &rel);
                self.templates_state
                    .git_remote_net_templates
                    .insert(name, status);
            }
        }
    }

    pub(super) fn selected_net_template(&self) -> Option<&NetTemplateEntry> {
        self.templates_state
            .net_templates
            .get(self.templates_state.net_templates_selected)
    }

    pub(super) fn capture_template_ai_snapshot(
        &mut self,
        kind: TemplatesKind,
        name: String,
        path: PathBuf,
    ) {
        let hash = file_content_hash(&path);
        self.templates_state.ai_edit_snapshot = Some(TemplateEditSnapshot {
            kind,
            name,
            path,
            hash,
        });
    }

    pub(super) fn apply_template_ai_snapshot_if_kind(&mut self, kind: TemplatesKind) {
        let Some(snapshot) = self.templates_state.ai_edit_snapshot.as_ref() else {
            return;
        };
        if snapshot.kind != kind {
            return;
        }
        let snapshot = self.templates_state.ai_edit_snapshot.take().unwrap();
        if commands::git_cmd::is_git_repo(&self.templates_state.dir) {
            return;
        }
        let next_hash = file_content_hash(&snapshot.path);
        if next_hash != snapshot.hash {
            match snapshot.kind {
                TemplatesKind::Stacks => {
                    self.templates_state.dirty_templates.insert(snapshot.name);
                }
                TemplatesKind::Networks => {
                    self.templates_state
                        .dirty_net_templates
                        .insert(snapshot.name);
                }
            }
        }
    }
}

fn parse_git_status_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let mut p = line[3..].trim();
    if let Some(idx) = p.rfind(" -> ") {
        p = p[idx + 4..].trim();
    }
    if (p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\'')) {
        p = &p[1..p.len().saturating_sub(1)];
    }
    if p.is_empty() {
        return None;
    }
    Some(p.replace('\\', "/"))
}

fn git_remote_status_for_path(repo: &Path, rel_path: &str) -> GitRemoteStatus {
    let ahead = commands::git_cmd::run_git(
        repo,
        &["log", "--oneline", "@{u}..", "--", rel_path],
    )
    .ok()
    .map(|out| !out.trim().is_empty())
    .unwrap_or(false);
    let behind = commands::git_cmd::run_git(
        repo,
        &["log", "--oneline", "..@{u}", "--", rel_path],
    )
    .ok()
    .map(|out| !out.trim().is_empty())
    .unwrap_or(false);
    match (ahead, behind) {
        (false, false) => GitRemoteStatus::UpToDate,
        (true, false) => GitRemoteStatus::Ahead,
        (false, true) => GitRemoteStatus::Behind,
        (true, true) => GitRemoteStatus::Diverged,
    }
}

fn file_content_hash(path: &Path) -> Option<u64> {
    let data = fs::read(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write(&data);
    Some(h.finish())
}
