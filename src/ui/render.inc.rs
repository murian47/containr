fn draw(f: &mut ratatui::Frame, app: &mut App, refresh: Duration) {
    draw_shell(f, app, refresh);
}

fn draw_shell(f: &mut ratatui::Frame, app: &mut App, refresh: Duration) {
    // Shell UI: header + sidebar + main + footer + command line. No overlays/dialogs.
    let area = f.area();
    let bg = app.theme.background.to_style();
    f.render_widget(Block::default().style(bg), area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
            Constraint::Length(1), // cmdline
        ])
        .split(area);

    draw_shell_header(f, app, refresh, rows[0]);
    draw_shell_body(f, app, rows[1]);
    draw_shell_footer(f, app, rows[2]);
    draw_shell_cmdline(f, app, rows[3]);
}


fn shell_cycle_focus(app: &mut App) {
    let mut order: Vec<ShellFocus> = Vec::new();
    if !app.shell_sidebar_hidden {
        order.push(ShellFocus::Sidebar);
    }
    order.push(ShellFocus::List);
    let has_details = matches!(
        app.shell_view,
        ShellView::Stacks
            | ShellView::Containers
            | ShellView::Images
            | ShellView::Volumes
            | ShellView::Networks
            | ShellView::Templates
            | ShellView::Registries
    );
    if has_details {
        order.push(ShellFocus::Details);
    }
    let dock_allowed = app.log_dock_enabled
        && !matches!(
            app.shell_view,
            ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages | ShellView::ThemeSelector
        );
    if dock_allowed {
        order.push(ShellFocus::Dock);
    }
    if order.is_empty() {
        app.shell_focus = ShellFocus::List;
        return;
    }
    let idx = order
        .iter()
        .position(|f| *f == app.shell_focus)
        .unwrap_or(0);
    let next = (idx + 1) % order.len();
    app.shell_focus = order[next];
}




impl App {
    fn persist_config(&mut self) {
        let cfg = ContainrConfig {
            version: 10,
            last_server: self.active_server.clone(),
            refresh_secs: self.refresh_secs.max(1),
            logs_tail: self.logs.tail.max(1),
            cmd_history_max: self.cmd_history_max_effective(),
            cmd_history: self.shell_cmdline.history.entries.clone(),
            active_theme: self.theme_name.clone(),
            templates_dir: self.templates_state.dir.to_string_lossy().to_string(),
            editor_cmd: self.editor_cmd.clone(),
            view_layout: self
                .shell_split_by_view
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            ShellSplitMode::Horizontal => "horizontal".to_string(),
                            ShellSplitMode::Vertical => "vertical".to_string(),
                        },
                    )
                })
                .collect(),
            keymap: self.keymap.clone(),
            servers: self.servers.clone(),
            git_autocommit: self.git_autocommit,
            git_autocommit_confirm: self.git_autocommit_confirm,
            image_update_concurrency: self.image_update_concurrency,
            image_update_debug: self.image_update_debug,
            image_update_autocheck: self.image_update_autocheck,
            kitty_graphics: self.kitty_graphics,
            log_dock_enabled: self.log_dock_enabled,
            log_dock_height: self.log_dock_height,
        };
        if let Err(e) = config::save(&self.config_path, &cfg) {
            self.set_error(format!("failed to save config: {:#}", e));
        }
    }

    fn persist_registries(&mut self) {
        let path = config::registries_path(&self.config_path);
        if let Err(e) = config::save_registries(&path, &self.registries_cfg) {
            self.set_error(format!("failed to save registries: {:#}", e));
            return;
        }
        self.resolve_registry_auths();
    }

    fn prune_image_updates(&mut self) {
        let now = now_unix();
        self.image_updates
            .retain(|_, v| now.saturating_sub(v.checked_at) <= IMAGE_UPDATE_TTL_SECS);
    }

    fn prune_rate_limits(&mut self) {
        let now = now_unix();
        self.rate_limits.retain(|_, v| {
            v.hits.retain(|ts| now.saturating_sub(*ts) <= RATE_LIMIT_WINDOW_SECS);
            if let Some(until) = v.limited_until {
                if now >= until {
                    v.limited_until = None;
                }
            }
            !v.hits.is_empty() || v.limited_until.is_some()
        });
    }

    fn note_rate_limit_request(&mut self, image_ref: &str) {
        let now = now_unix();
        let registry = image_registry_for_ref(image_ref);
        let entry = self.rate_limits.entry(registry).or_default();
        entry.hits.push(now);
        entry
            .hits
            .retain(|ts| now.saturating_sub(*ts) <= RATE_LIMIT_WINDOW_SECS);
    }

    fn note_rate_limit_error(&mut self, image_ref: &str) {
        let now = now_unix();
        let registry = image_registry_for_ref(image_ref);
        let entry = self.rate_limits.entry(registry).or_default();
        entry.limited_until = Some(now + RATE_LIMIT_WINDOW_SECS);
    }

    fn status_banner(&mut self) -> Option<String> {
        if self.refresh_paused {
            let reason = self
                .refresh_pause_reason
                .as_deref()
                .unwrap_or("paused")
                .to_string();
            return Some(format!("Refresh paused ({reason}). Press r to retry."));
        }
        self.rate_limit_banner()
    }

    fn rate_limit_banner(&mut self) -> Option<String> {
        self.prune_rate_limits();
        let now = now_unix();
        let mut limited: Option<(String, i64)> = None;
        let mut warn: Option<(String, usize)> = None;
        for (reg, entry) in &self.rate_limits {
            if let Some(until) = entry.limited_until {
                if until > now {
                    let remaining = until.saturating_sub(now);
                    limited = Some((reg.clone(), remaining));
                    break;
                }
            }
            let count = entry.hits.len();
            if count >= RATE_LIMIT_WARN {
                if warn.as_ref().map(|(_, c)| count > *c).unwrap_or(true) {
                    warn = Some((reg.clone(), count));
                }
            }
        }
        if let Some((reg, remaining)) = limited {
            let mins = (remaining / 60).max(1);
            return Some(format!(
                "Rate limit reached for {reg}. Try again in ~{mins}m."
            ));
        }
        if let Some((reg, count)) = warn {
            return Some(format!(
                "Rate limit nearing for {reg}: {count}/{RATE_LIMIT_MAX} in 6h window."
            ));
        }
        None
    }

    fn save_local_state(&mut self) {
        let dir = self
            .image_updates_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        if let Err(e) = fs::create_dir_all(&dir) {
            self.log_msg(MsgLevel::Warn, format!("state dir create failed: {:#}", e));
            return;
        }
        self.prune_rate_limits();
        let state = LocalState {
            version: 6,
            image_updates: self.image_updates.clone(),
            rate_limits: self.rate_limits.clone(),
            template_deploys: self.template_deploys.clone(),
            net_template_deploys: self.net_template_deploys.clone(),
            registry_tests: self.registry_tests.clone(),
        };
        match serde_json::to_string_pretty(&state) {
            Ok(raw) => {
                if let Err(e) = fs::write(&self.image_updates_path, raw) {
                    self.log_msg(MsgLevel::Warn, format!("state save failed: {:#}", e));
                }
            }
            Err(e) => {
                self.log_msg(MsgLevel::Warn, format!("state serialize failed: {:#}", e));
            }
        }
    }

    fn remove_template_deploys_for_server(&mut self, template_id: &str, server: &str) -> bool {
        if template_id.trim().is_empty() || server.trim().is_empty() {
            return false;
        }
        let mut changed = false;
        let mut empty = false;
        if let Some(list) = self.template_deploys.get_mut(template_id) {
            let before = list.len();
            list.retain(|entry| entry.server_name != server);
            if list.len() != before {
                changed = true;
            }
            if list.is_empty() {
                empty = true;
            }
        }
        if empty {
            self.template_deploys.remove(template_id);
        }
        changed || empty
    }

    fn prune_template_deploys_for_active_server(&mut self) {
        let Some(server) = self.active_server.clone() else {
            return;
        };
        if server.trim().is_empty() {
            return;
        }
        let mut present: HashMap<String, (Option<String>, Vec<String>)> = HashMap::new();
        for c in &self.containers {
            let Some(id) = template_id_from_labels(&c.labels) else {
                continue;
            };
            let commit = template_commit_from_labels(&c.labels);
            present
                .entry(id)
                .and_modify(|slot| {
                    if slot.0.is_none() && commit.is_some() {
                        slot.0 = commit.clone();
                    }
                    slot.1.push(c.name.clone());
                })
                .or_insert_with(|| (commit, vec![c.name.clone()]));
        }
        let present_ids: HashSet<String> = present.keys().cloned().collect();
        let known_ids: HashSet<String> = self
            .templates_state
            .templates
            .iter()
            .filter_map(|t| t.template_id.clone())
            .collect();
        for id in present_ids.iter() {
            if known_ids.contains(id) {
                continue;
            }
            if self.unknown_template_ids_warned.insert(id.clone()) {
                let names = present
                    .get(id)
                    .map(|(_, names)| names.clone())
                    .unwrap_or_default();
                let mut names = names;
                names.sort();
                names.dedup();
                let names_text = if names.is_empty() {
                    "-".to_string()
                } else {
                    names.join(", ")
                };
                self.log_msg(
                    MsgLevel::Info,
                    format!(
                        "template id found on server but missing locally: {id} (containers: {names_text})"
                    ),
                );
            }
        }
        let mut next: HashMap<String, Vec<TemplateDeployEntry>> = HashMap::new();
        let mut changed = false;
        for (template_id, list) in &self.template_deploys {
            let mut out: Vec<TemplateDeployEntry> = Vec::new();
            for entry in list {
                if entry.server_name == server && !present_ids.contains(template_id) {
                    changed = true;
                    continue;
                }
                out.push(entry.clone());
            }
            if out.is_empty() {
                changed = true;
                continue;
            }
            next.insert(template_id.clone(), out);
        }
        for id in present_ids.iter() {
            if !known_ids.contains(id) {
                continue;
            }
            let entry = next.entry(id.clone()).or_default();
            if let Some(existing) = entry.iter_mut().find(|e| e.server_name == server) {
                let commit = present.get(id).and_then(|c| c.0.clone());
                if existing.commit != commit {
                    existing.commit = commit;
                    changed = true;
                }
                continue;
            }
            if !entry.iter().any(|e| e.server_name == server) {
                entry.push(TemplateDeployEntry {
                    server_name: server.clone(),
                    timestamp: now_unix(),
                    commit: present.get(id).and_then(|c| c.0.clone()),
                });
                self.log_msg(
                    MsgLevel::Info,
                    format!("template id matched on server {server}: {id}"),
                );
                changed = true;
            }
        }
        if changed {
            self.template_deploys = next;
            self.save_local_state();
        }
    }

    fn image_update_entry(&self, key: &str) -> Option<&ImageUpdateEntry> {
        let entry = self.image_updates.get(key)?;
        let now = now_unix();
        if now.saturating_sub(entry.checked_at) > IMAGE_UPDATE_TTL_SECS {
            return None;
        }
        Some(entry)
    }

    fn messages_save(&mut self, path: &str, force: bool) {
        if self.session_msgs.is_empty() {
            self.set_warn("no messages");
            return;
        }
        let mut out = String::new();
        for m in &self.session_msgs {
            let lvl = match m.level {
                MsgLevel::Info => "INFO",
                MsgLevel::Warn => "WARN",
                MsgLevel::Error => "ERROR",
            };
            let ts = format_session_ts(m.at);
            out.push_str(&format!("{ts} {lvl} {}\n", m.text));
        }
        match write_text_file(path, &out, force) {
            Ok(p) => self.set_info(format!("saved messages to {}", p.display())),
            Err(e) => self.set_error(format!("{e:#}")),
        }
    }
}

fn find_server_by_name(servers: &[ServerEntry], name: &str) -> Option<usize> {
    servers.iter().position(|s| s.name == name)
}

fn ensure_unique_server_name(servers: &[ServerEntry], desired: &str) -> Option<String> {
    let desired = desired.trim();
    if desired.is_empty() {
        return None;
    }
    if !servers.iter().any(|s| s.name == desired) {
        return Some(desired.to_string());
    }
    None
}

























#[derive(Debug, Serialize, Deserialize)]
struct ImageUpdateResult {
    image: String,
    entry: ImageUpdateEntry,
    debug: Option<String>,
}



























pub(in crate::ui) fn image_update_indicator(app: &App, view: ImageUpdateView, bg: Style) -> (String, Style) {
    let (text, style) = match view {
        ImageUpdateView::UpToDate => (
            if app.ascii_only { "Y" } else { "●" },
            bg.patch(app.theme.text_ok.to_style()),
        ),
        ImageUpdateView::UpdateAvailable => (
            if app.ascii_only { "U" } else { "●" },
            bg.patch(app.theme.text_warn.to_style()),
        ),
        ImageUpdateView::Error => (
            if app.ascii_only { "!" } else { "●" },
            bg.patch(app.theme.text_error.to_style()),
        ),
        ImageUpdateView::RateLimited => (
            if app.ascii_only { "i" } else { "●" },
            bg.patch(app.theme.text_info.to_style()),
        ),
        ImageUpdateView::Checking => (
            if app.ascii_only { "*" } else { "⟳" },
            bg.patch(app.theme.text_warn.to_style()),
        ),
        ImageUpdateView::Unknown => (
            if app.ascii_only { "?" } else { "·" },
            bg.patch(app.theme.text_dim.to_style()),
        ),
    };
    (text.to_string(), style)
}






























































pub(in crate::ui) fn format_session_ts(at: OffsetDateTime) -> String {
    use std::sync::OnceLock;
    static FMT: OnceLock<Vec<time::format_description::FormatItem<'static>>> = OnceLock::new();
    let fmt = FMT.get_or_init(|| {
        time::format_description::parse("[hour]:[minute]:[second]")
            .unwrap_or_else(|_| Vec::new())
    });
    at.format(fmt)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

