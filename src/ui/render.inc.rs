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

fn shell_sidebar_items(app: &App) -> Vec<ShellSidebarItem> {
    let mut items: Vec<ShellSidebarItem> = Vec::new();
    for i in 0..app.servers.len() {
        items.push(ShellSidebarItem::Server(i));
    }
    items.push(ShellSidebarItem::Separator);
    items.push(ShellSidebarItem::Module(ShellView::Dashboard));
    items.push(ShellSidebarItem::Module(ShellView::Stacks));
    items.push(ShellSidebarItem::Module(ShellView::Containers));
    items.push(ShellSidebarItem::Module(ShellView::Images));
    items.push(ShellSidebarItem::Module(ShellView::Volumes));
    items.push(ShellSidebarItem::Module(ShellView::Networks));
    items.push(ShellSidebarItem::Module(ShellView::Inspect));
    items.push(ShellSidebarItem::Module(ShellView::Logs));
    items.push(ShellSidebarItem::Gap);
    items.push(ShellSidebarItem::Module(ShellView::Templates));
    items.push(ShellSidebarItem::Module(ShellView::Registries));
    // Help is accessible via :? / :help (not a module entry).

    let actions: Vec<ShellAction> = match app.shell_view {
        ShellView::Dashboard => vec![],
        ShellView::Stacks => vec![
            ShellAction::Start,
            ShellAction::Stop,
            ShellAction::Restart,
            ShellAction::Delete,
        ],
        ShellView::Containers => vec![
            ShellAction::Start,
            ShellAction::Stop,
            ShellAction::Restart,
            ShellAction::Delete,
            ShellAction::Console,
        ],
        ShellView::Images => vec![ShellAction::ImageUntag, ShellAction::ImageForceRemove],
        ShellView::Volumes => vec![ShellAction::VolumeRemove],
        ShellView::Networks => vec![ShellAction::NetworkRemove],
        ShellView::Templates => vec![
            ShellAction::TemplateEdit,
            ShellAction::TemplateNew,
            ShellAction::TemplateDelete,
            ShellAction::TemplateDeploy,
        ],
        ShellView::Registries => vec![ShellAction::RegistryTest],
        ShellView::Inspect | ShellView::Logs | ShellView::Help => vec![],
        ShellView::Messages => vec![],
    };
    if !actions.is_empty() {
        items.push(ShellSidebarItem::Separator);
        for a in actions {
            items.push(ShellSidebarItem::Action(a));
        }
    }
    items
}

fn shell_is_selectable(item: ShellSidebarItem) -> bool {
    !matches!(item, ShellSidebarItem::Separator | ShellSidebarItem::Gap)
}

fn shell_move_sidebar(app: &mut App, dir: i32) {
    let items = shell_sidebar_items(app);
    if items.is_empty() {
        app.shell_sidebar_selected = 0;
        return;
    }
    let mut idx = app.shell_sidebar_selected.min(items.len() - 1);
    for _ in 0..items.len() {
        if dir < 0 {
            idx = idx.saturating_sub(1);
        } else {
            idx = (idx + 1).min(items.len() - 1);
        }
        if shell_is_selectable(items[idx]) {
            app.shell_sidebar_selected = idx;
            return;
        }
        if idx == 0 || idx == items.len() - 1 {
            break;
        }
    }
    app.shell_sidebar_selected = idx;
}

fn shell_cycle_focus(app: &mut App) {
    app.shell_focus = match app.shell_focus {
        ShellFocus::Sidebar => ShellFocus::List,
        ShellFocus::List => ShellFocus::Details,
        ShellFocus::Details => ShellFocus::Sidebar,
    };
}

fn shell_sidebar_select_item(app: &mut App, target: ShellSidebarItem) {
    let items = shell_sidebar_items(app);
    if let Some((idx, _)) = items
        .iter()
        .enumerate()
        .find(|(_, it)| **it == target && shell_is_selectable(**it))
    {
        app.shell_sidebar_selected = idx;
    }
}

fn shell_set_main_view(app: &mut App, view: ShellView) {
    app.shell_view = view;
    if !matches!(
        view,
        ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages
    ) {
        app.shell_last_main_view = view;
    }
    if view == ShellView::Messages {
        app.mark_messages_seen();
    }
    app.shell_focus = ShellFocus::List;
    app.active_view = match view {
        ShellView::Dashboard => app.active_view,
        ShellView::Stacks => ActiveView::Stacks,
        ShellView::Containers => ActiveView::Containers,
        ShellView::Images => ActiveView::Images,
        ShellView::Volumes => ActiveView::Volumes,
        ShellView::Networks => ActiveView::Networks,
        ShellView::Templates => app.active_view,
        ShellView::Registries => app.active_view,
        ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages => {
            app.active_view
        }
    };
    if view == ShellView::Templates {
        app.refresh_templates();
        app.refresh_net_templates();
    }
    if let Some(mode) = app.get_view_split_mode(view) {
        app.shell_split_mode = mode;
    }
}

fn shell_first_container_id(app: &mut App) -> Option<String> {
    if let Some(c) = app.selected_container() {
        return Some(c.id.clone());
    }
    if app.active_view != ActiveView::Containers {
        app.active_view = ActiveView::Containers;
    }
    if app.containers.is_empty() {
        return None;
    }
    if app.list_mode == ListMode::Tree {
        app.ensure_view();
        if let Some((idx, ViewEntry::Container { id, .. })) = app
            .view
            .iter()
            .enumerate()
            .find(|(_, e)| matches!(e, ViewEntry::Container { .. }))
        {
            app.selected = idx;
            return Some(id.clone());
        }
    }
    app.selected = app.selected.min(app.containers.len().saturating_sub(1));
    Some(app.containers.get(app.selected)?.id.clone())
}

fn shell_enter_logs(app: &mut App, logs_req_tx: &mpsc::UnboundedSender<(String, usize)>) {
    // Logs are container-only; always use the containers selection.
    shell_set_main_view(app, ShellView::Containers);
    app.shell_view = ShellView::Logs;
    app.shell_focus = ShellFocus::List;
    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Logs));

    let Some(id) = shell_first_container_id(app) else {
        app.logs.loading = false;
        app.logs.error = Some("no container selected".to_string());
        app.logs.text = None;
        return;
    };
    app.open_logs_state(id.clone());
    let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
}

fn shell_enter_inspect(app: &mut App, inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>) {
    // Inspect follows the current main view selection.
    if matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
        app.shell_view = app.shell_last_main_view;
    }
    app.shell_view = ShellView::Inspect;
    app.shell_focus = ShellFocus::List;
    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Inspect));

    let Some(target) = app.selected_inspect_target() else {
        app.inspect.loading = false;
        app.inspect.error = Some("nothing selected".to_string());
        app.inspect.value = None;
        app.inspect.lines.clear();
        return;
    };
    app.open_inspect_state(target.clone());
    let _ = inspect_req_tx.send(target);
}

fn shell_back_from_full(app: &mut App) {
    if matches!(
        app.shell_view,
        ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages
    ) {
        // Full-screen views should never keep command-line mode active in the background.
        app.shell_cmdline.mode = false;
        app.shell_cmdline.confirm = None;
        let fallback = if app.shell_last_main_view == ShellView::Messages {
            ShellView::Dashboard
        } else {
            app.shell_last_main_view
        };
        app.shell_view = if app.shell_view == ShellView::Help {
            if app.shell_help.return_view == ShellView::Help {
                fallback
            } else {
                app.shell_help.return_view
            }
        } else if app.shell_view == ShellView::Messages {
            if app.shell_msgs.return_view == ShellView::Messages {
                fallback
            } else {
                app.shell_msgs.return_view
            }
        } else {
            fallback
        };
        app.shell_focus = ShellFocus::List;
        shell_sidebar_select_item(app, ShellSidebarItem::Module(app.shell_view));
    }
}

fn shell_switch_server(
    app: &mut App,
    idx: usize,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
) {
    let Some(s) = app.servers.get(idx).cloned() else {
        return;
    };
    app.server_selected = idx;
    app.active_server = Some(s.name.clone());
    app.clear_all_marks();
    app.action_inflight.clear();
    app.image_action_inflight.clear();
    app.volume_action_inflight.clear();
    app.network_action_inflight.clear();

    let runner = if s.target == "local" {
        Runner::Local
    } else {
        Runner::Ssh(Ssh {
            target: s.target.clone(),
            identity: s.identity.clone(),
            port: s.port,
        })
    };
    app.current_target = runner.key();
    app.clear_conn_error();
    app.start_loading(true);
    app.dashboard.loading = true;
    app.dashboard.error = None;
    app.dashboard.snap = None;
    let _ = conn_tx.send(Connection {
        runner,
        docker: DockerCfg {
            docker_cmd: s.docker_cmd,
        },
    });

    // Persist last_server only; no secrets stored.
    app.persist_config();
    let _ = refresh_tx.send(());
    let _ = dash_refresh_tx.send(());

    shell_set_main_view(app, ShellView::Dashboard);
    shell_sidebar_select_item(app, ShellSidebarItem::Server(idx));
}

fn shell_refresh(
    app: &mut App,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    refresh_pause_tx: &watch::Sender<bool>,
) {
    if app.servers.is_empty() && app.current_target.trim().is_empty() {
        app.set_warn("no server configured");
        return;
    }
    if app.refresh_paused {
        app.refresh_paused = false;
        app.refresh_pause_reason = None;
        app.refresh_error_streak = 0;
        let _ = refresh_pause_tx.send(false);
    }
    app.start_loading(true);
    app.dashboard.loading = true;
    let _ = refresh_tx.send(());
    let _ = dash_refresh_tx.send(());
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
            version: 5,
            image_updates: self.image_updates.clone(),
            rate_limits: self.rate_limits.clone(),
            template_deploys: self.template_deploys.clone(),
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
        let mut present_ids: HashSet<String> = HashSet::new();
        for c in &self.containers {
            if let Some(id) = template_id_from_labels(&c.labels) {
                present_ids.insert(id);
            }
        }
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
                self.log_msg(
                    MsgLevel::Info,
                    format!("template id found on server but missing locally: {id}"),
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
            if !entry.iter().any(|e| e.server_name == server) {
                entry.push(TemplateDeployEntry {
                    server_name: server.clone(),
                    timestamp: now_unix(),
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

#[derive(Debug, Deserialize)]
struct ContainerInspect {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Config")]
    config: Option<ContainerInspectConfig>,
    #[serde(rename = "HostConfig")]
    host_config: Option<ContainerInspectHostConfig>,
    #[serde(rename = "NetworkSettings")]
    network_settings: Option<ContainerInspectNetworkSettings>,
    #[serde(rename = "Mounts")]
    mounts: Option<Vec<ContainerInspectMount>>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectConfig {
    #[serde(rename = "Image")]
    image: Option<String>,
    #[serde(rename = "Env")]
    env: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    cmd: Option<Vec<String>>,
    #[serde(rename = "Entrypoint")]
    entrypoint: Option<Vec<String>>,
    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
    #[serde(rename = "WorkingDir")]
    working_dir: Option<String>,
    #[serde(rename = "User")]
    user: Option<String>,
    #[serde(rename = "ExposedPorts")]
    exposed_ports: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "Healthcheck")]
    healthcheck: Option<ContainerInspectHealthcheck>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectHealthcheck {
    #[serde(rename = "Test")]
    test: Option<Vec<String>>,
    #[serde(rename = "Interval")]
    interval: Option<i64>,
    #[serde(rename = "Timeout")]
    timeout: Option<i64>,
    #[serde(rename = "Retries")]
    retries: Option<i64>,
    #[serde(rename = "StartPeriod")]
    start_period: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectHostConfig {
    #[serde(rename = "RestartPolicy")]
    restart_policy: Option<ContainerInspectRestartPolicy>,
    #[serde(rename = "PortBindings")]
    port_bindings: Option<HashMap<String, Vec<ContainerInspectPortBinding>>>,
    #[serde(rename = "ReadonlyRootfs")]
    readonly_rootfs: Option<bool>,
    #[serde(rename = "Privileged")]
    privileged: Option<bool>,
    #[serde(rename = "ExtraHosts")]
    extra_hosts: Option<Vec<String>>,
    #[serde(rename = "NetworkMode")]
    network_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectRestartPolicy {
    #[serde(rename = "Name")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectPortBinding {
    #[serde(rename = "HostIp")]
    host_ip: Option<String>,
    #[serde(rename = "HostPort")]
    host_port: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectNetworkSettings {
    #[serde(rename = "Networks")]
    networks: Option<HashMap<String, ContainerInspectNetworkAttachment>>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectNetworkAttachment {
    #[serde(rename = "Aliases")]
    aliases: Option<Vec<String>>,
    #[serde(rename = "IPAddress")]
    ip_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectMount {
    #[serde(rename = "Type")]
    kind: Option<String>,
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Source")]
    source: Option<String>,
    #[serde(rename = "Destination")]
    destination: Option<String>,
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "ReadOnly")]
    read_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct NetworkInspect {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "Internal")]
    internal: Option<bool>,
    #[serde(rename = "Attachable")]
    attachable: Option<bool>,
    #[serde(rename = "EnableIPv6")]
    enable_ipv6: Option<bool>,
    #[serde(rename = "Options")]
    options: Option<HashMap<String, String>>,
    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
    #[serde(rename = "IPAM")]
    ipam: Option<NetworkInspectIpam>,
}

#[derive(Debug, Deserialize)]
struct NetworkInspectIpam {
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "Config")]
    config: Option<Vec<NetworkInspectIpamConfig>>,
}

#[derive(Debug, Deserialize)]
struct NetworkInspectIpamConfig {
    #[serde(rename = "Subnet")]
    subnet: Option<String>,
    #[serde(rename = "Gateway")]
    gateway: Option<String>,
    #[serde(rename = "IPRange")]
    ip_range: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageInspect {
    #[serde(rename = "RepoDigests")]
    repo_digests: Option<Vec<String>>,
    #[serde(rename = "Architecture")]
    architecture: Option<String>,
    #[serde(rename = "Os")]
    os: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageUpdateResult {
    image: String,
    entry: ImageUpdateEntry,
    debug: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeService {
    image: String,
    container_name: Option<String>,
    command: Vec<String>,
    entrypoint: Vec<String>,
    environment: Vec<String>,
    ports: Vec<String>,
    expose: Vec<String>,
    volumes: Vec<String>,
    tmpfs: Vec<String>,
    networks: BTreeMap<String, ComposeServiceNetwork>,
    labels: BTreeMap<String, String>,
    restart: Option<String>,
    working_dir: Option<String>,
    user: Option<String>,
    privileged: Option<bool>,
    read_only: Option<bool>,
    extra_hosts: Vec<String>,
    healthcheck: Option<ComposeHealthcheck>,
    network_mode: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeServiceNetwork {
    aliases: Vec<String>,
    ipv4_address: Option<String>,
}

#[derive(Clone, Debug)]
struct ComposeHealthcheck {
    test: Vec<String>,
    interval: Option<String>,
    timeout: Option<String>,
    retries: Option<i64>,
    start_period: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeNetwork {
    name: String,
    driver: Option<String>,
    internal: Option<bool>,
    attachable: Option<bool>,
    enable_ipv6: Option<bool>,
    ipam: Option<ComposeNetworkIpam>,
    options: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct ComposeNetworkIpam {
    driver: Option<String>,
    config: Vec<ComposeNetworkIpamConfig>,
}

#[derive(Clone, Debug)]
struct ComposeNetworkIpamConfig {
    subnet: Option<String>,
    gateway: Option<String>,
    ip_range: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeVolume {
    driver: Option<String>,
}

fn is_system_network_name(name: &str) -> bool {
    matches!(
        name,
        "bridge" | "host" | "none" | "ingress" | "docker_gwbridge"
    )
}

fn stack_name_from_label_map(labels: &HashMap<String, String>) -> Option<String> {
    labels
        .get("com.docker.compose.project")
        .or_else(|| labels.get("com.docker.stack.namespace"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn service_name_from_labels(
    labels: &HashMap<String, String>,
    stack_name: Option<&str>,
    container_name: &str,
) -> String {
    if let Some(name) = labels.get("com.docker.compose.service") {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    if let Some(name) = labels.get("com.docker.swarm.service.name") {
        let name = name.trim();
        if !name.is_empty() {
            if let Some(stack) = stack_name {
                for sep in ['_', '-', '.'] {
                    let prefix = format!("{stack}{sep}");
                    if name.starts_with(&prefix) {
                        return name[prefix.len()..].to_string();
                    }
                }
            }
            return name.to_string();
        }
    }
    let mut name = container_name.trim().trim_start_matches('/').to_string();
    if let Some(stack) = stack_name {
        for sep in ['_', '-', '.'] {
            let prefix = format!("{stack}{sep}");
            if name.starts_with(&prefix) {
                name = name[prefix.len()..].to_string();
                break;
            }
        }
    }
    if name.is_empty() {
        "service".to_string()
    } else {
        name
    }
}

fn sanitize_compose_key(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("item");
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn unique_compose_key(name: &str, used: &mut HashSet<String>) -> String {
    let base = sanitize_compose_key(name);
    if used.insert(base.clone()) {
        return base;
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{base}_{idx}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        idx = idx.saturating_add(1);
    }
}

fn yaml_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn is_digest_only_image(image: &str) -> bool {
    let image = image.trim();
    if image.starts_with("sha256:") && image.len() == "sha256:".len() + 64 {
        return image["sha256:".len()..].chars().all(|c| c.is_ascii_hexdigit());
    }
    if image.len() == 64 {
        return image.chars().all(|c| c.is_ascii_hexdigit());
    }
    false
}

fn normalize_image_ref(image: &str) -> String {
    let image = image.trim();
    if image.is_empty() {
        return String::new();
    }
    if is_digest_only_image(image) {
        return image.to_string();
    }
    let (name, digest) = match image.split_once('@') {
        Some((name, digest)) => (name, Some(digest)),
        None => (image, None),
    };
    let (base, tag) = match name.rsplit_once(':') {
        Some((base, tag)) if !tag.contains('/') => (base, Some(tag)),
        _ => (name, None),
    };
    let is_unqualified = !base.contains('/');
    let base = if is_unqualified {
        format!("docker.io/library/{base}")
    } else {
        base.to_string()
    };
    if let Some(digest) = digest {
        return format!("{base}@{digest}");
    }
    let tag = tag.unwrap_or("latest");
    format!("{base}:{tag}")
}

struct NormalizedImageRef {
    reference: String,
    digest: Option<String>,
}

fn normalize_image_ref_for_updates(image: &str) -> Option<NormalizedImageRef> {
    if is_digest_only_image(image) {
        return None;
    }
    let normalized = normalize_image_ref(image);
    if normalized.is_empty() {
        return None;
    }
    let digest = normalized.split_once('@').map(|(_, d)| d.to_string());
    Some(NormalizedImageRef {
        reference: normalized,
        digest,
    })
}

fn resolve_image_ref_for_updates(app: &App, image: &str) -> Option<String> {
    if image.trim().is_empty() {
        return None;
    }
    if is_digest_only_image(image) {
        let needle = normalize_image_id(image);
        for img in &app.images {
            if normalize_image_id(&img.id) == needle {
                if let Some(reference) = App::image_row_ref(img) {
                    return Some(reference);
                }
            }
        }
        return None;
    }
    Some(normalize_image_ref(image))
}

fn image_repo_name(image_ref: &str) -> String {
    let name = image_ref.split_once('@').map(|(n, _)| n).unwrap_or(image_ref);
    match name.rsplit_once(':') {
        Some((base, tag)) if !tag.contains('/') => base.to_string(),
        _ => name.to_string(),
    }
}

fn image_registry_for_ref(image_ref: &str) -> String {
    let name = image_ref.split_once('@').map(|(n, _)| n).unwrap_or(image_ref);
    let name = name.split_once(':').map(|(n, _)| n).unwrap_or(name);
    let first = name.split('/').next().unwrap_or("");
    let has_registry = first.contains('.') || first.contains(':') || first == "localhost";
    if has_registry {
        first.to_string()
    } else {
        "docker.io".to_string()
    }
}

fn normalize_docker_hub_repo(name: &str) -> String {
    let mut name = name.trim().to_string();
    if let Some(rest) = name.strip_prefix("docker.io/") {
        name = rest.to_string();
    }
    if !name.contains('/') {
        name = format!("library/{name}");
    }
    name
}

fn local_repo_digest(repo_digests: &[String], repo: &str) -> Option<String> {
    let repo_docker_hub = normalize_docker_hub_repo(repo);
    for entry in repo_digests {
        let (name, digest) = entry.split_once('@')?;
        if name == repo || name == repo_docker_hub {
            return Some(digest.to_string());
        }
        let name_docker_hub = normalize_docker_hub_repo(name);
        if name_docker_hub == repo_docker_hub {
            return Some(digest.to_string());
        }
    }
    None
}

enum ImageUpdateView {
    Unknown,
    Checking,
    UpToDate,
    UpdateAvailable,
    Error,
    RateLimited,
}

fn is_rate_limit_error(err: Option<&str>) -> bool {
    let Some(err) = err else {
        return false;
    };
    let err = err.to_ascii_lowercase();
    err.contains("toomanyrequests")
        || err.contains("rate limit")
        || err.contains("429")
}

fn image_update_view_for_ref(app: &App, image: &str) -> (Option<String>, ImageUpdateView) {
    let normalized = match resolve_image_ref_for_updates(app, image) {
        Some(n) => n,
        None => return (None, ImageUpdateView::Unknown),
    };
    let key = normalized.clone();
    if app.image_updates_inflight.contains(&key) {
        return (Some(key), ImageUpdateView::Checking);
    }
    let Some(entry) = app.image_update_entry(&key) else {
        return (Some(key), ImageUpdateView::Unknown);
    };
    let view = match entry.status {
        ImageUpdateKind::UpToDate => ImageUpdateView::UpToDate,
        ImageUpdateKind::UpdateAvailable => ImageUpdateView::UpdateAvailable,
        ImageUpdateKind::Error => {
            if is_rate_limit_error(entry.error.as_deref()) {
                ImageUpdateView::RateLimited
            } else {
                ImageUpdateView::Error
            }
        }
    };
    (Some(key), view)
}

fn image_update_view_for_stack(app: &App, stack_name: &str) -> ImageUpdateView {
    let mut has_update = false;
    let mut has_error = false;
    let mut has_unknown = false;
    let mut has_checking = false;
    let mut has_rate_limit = false;
    let mut seen = false;
    for c in app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack_name))
    {
        seen = true;
        let (_, view) = image_update_view_for_ref(app, &c.image);
        match view {
            ImageUpdateView::UpdateAvailable => has_update = true,
            ImageUpdateView::Error => has_error = true,
            ImageUpdateView::Unknown => has_unknown = true,
            ImageUpdateView::Checking => has_checking = true,
            ImageUpdateView::RateLimited => has_rate_limit = true,
            ImageUpdateView::UpToDate => {}
        }
    }
    if !seen {
        return ImageUpdateView::Unknown;
    }
    if has_update {
        ImageUpdateView::UpdateAvailable
    } else if has_checking {
        ImageUpdateView::Checking
    } else if has_rate_limit {
        ImageUpdateView::RateLimited
    } else if has_error {
        ImageUpdateView::Error
    } else if has_unknown {
        ImageUpdateView::Unknown
    } else {
        ImageUpdateView::UpToDate
    }
}

fn image_update_indicator(app: &App, view: ImageUpdateView, bg: Style) -> (String, Style) {
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

fn manifest_entries(raw: &str) -> Vec<Value> {
    let v: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    if let Some(arr) = v.as_array() {
        return arr.to_vec();
    }
    if let Some(obj) = v.as_object() {
        for key in ["manifests", "Manifests"] {
            if let Some(Value::Array(arr)) = obj.get(key) {
                return arr.to_vec();
            }
        }
        if get_ci(obj, "descriptor").is_some() || get_ci(obj, "ref").is_some() {
            return vec![v];
        }
    }
    Vec::new()
}

fn get_ci<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key).or_else(|| {
        obj.iter()
            .find_map(|(k, v)| if k.eq_ignore_ascii_case(key) { Some(v) } else { None })
    })
}

fn entry_descriptor(entry: &Value) -> Option<&serde_json::Map<String, Value>> {
    let obj = entry.as_object()?;
    let desc = get_ci(obj, "descriptor")?;
    desc.as_object()
}

fn entry_descriptor_digest(entry: &Value) -> Option<String> {
    if let Some(desc) = entry_descriptor(entry) {
        if let Some(digest) = get_ci(desc, "digest").and_then(|v| v.as_str()) {
            return Some(digest.to_string());
        }
    }
    let obj = entry.as_object()?;
    if let Some(reference) = get_ci(obj, "ref").and_then(|v| v.as_str()) {
        if let Some((_, digest)) = reference.split_once('@') {
            return Some(digest.to_string());
        }
    }
    get_ci(obj, "digest")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn entry_platform(entry: &Value) -> (Option<String>, Option<String>) {
    let obj = match entry.as_object() {
        Some(obj) => obj,
        None => return (None, None),
    };
    let platform = entry_descriptor(entry)
        .and_then(|desc| get_ci(desc, "platform").and_then(|v| v.as_object()))
        .or_else(|| get_ci(obj, "platform").and_then(|v| v.as_object()));
    let Some(platform) = platform else {
        return (None, None);
    };
    let arch = get_ci(platform, "architecture")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let os = get_ci(platform, "os")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (arch, os)
}

fn manifest_descriptor_digest(raw: &str) -> Option<String> {
    let entries = manifest_entries(raw);
    entries
        .iter()
        .find_map(|entry| entry_descriptor_digest(entry))
}

fn manifest_digest_for_platform(raw: &str, arch: &str, os: &str) -> Option<String> {
    let entries = manifest_entries(raw);
    let mut fallback: Option<String> = None;
    for entry in entries {
        let digest = entry_descriptor_digest(&entry);
        let (p_arch, p_os) = entry_platform(&entry);
        if let (Some(p_arch), Some(p_os), Some(digest)) = (p_arch, p_os, digest) {
            if p_arch == arch && p_os == os {
                return Some(digest);
            }
            if fallback.is_none()
                && p_arch != "unknown"
                && p_os != "unknown"
                && !p_arch.is_empty()
                && !p_os.is_empty()
            {
                fallback = Some(digest);
            }
        }
    }
    fallback
}

fn manifest_platform_summary(raw: &str) -> String {
    let entries = manifest_entries(raw);
    if entries.is_empty() {
        return "none".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for entry in entries {
        let (arch, os) = entry_platform(&entry);
        let arch = arch.as_deref().unwrap_or("?");
        let os = os.as_deref().unwrap_or("?");
        parts.push(format!("{arch}/{os}"));
    }
    parts.join(",")
}

fn images_from_compose(path: &Path) -> Vec<String> {
    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let (code, _) = split_yaml_comment(line);
        let trimmed = code.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("image:") {
            let mut val = rest.trim().to_string();
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                val = val[1..val.len().saturating_sub(1)].to_string();
            }
            if !val.is_empty() {
                out.push(val);
            }
        }
    }
    out
}

fn manifest_remote_digests(raw: &str) -> Vec<(String, Option<String>, Option<String>)> {
    let entries = manifest_entries(raw);
    let mut out: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for entry in entries {
        let digest = match entry_descriptor_digest(&entry) {
            Some(d) => d,
            None => continue,
        };
        let (arch, os) = entry_platform(&entry);
        out.push((digest, arch, os));
    }
    out
}

fn format_remote_digest_list(items: &[(String, Option<String>, Option<String>)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (digest, arch, os) in items {
        let arch = arch.as_deref().unwrap_or("?");
        let os = os.as_deref().unwrap_or("?");
        parts.push(format!("{digest}@{arch}/{os}"));
    }
    parts.join(",")
}

async fn check_image_update(
    runner: &Runner,
    docker: &DockerCfg,
    image: &str,
    debug: bool,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let normalized = normalize_image_ref_for_updates(image)
        .ok_or_else(|| anyhow::anyhow!("invalid image reference"))?;
    let repo = image_repo_name(&normalized.reference);

    let inspect_raw = docker::fetch_image_inspect(runner, docker, &normalized.reference).await?;
    let inspect: ImageInspect = serde_json::from_str(&inspect_raw)
        .context("image inspect output was not JSON")?;
    let repo_digests_len = inspect.repo_digests.as_ref().map(|v| v.len()).unwrap_or(0);
    let repo_digests_preview = inspect
        .repo_digests
        .as_ref()
        .map(|list| {
            let mut parts: Vec<String> = Vec::new();
            for item in list.iter().take(3) {
                parts.push(item.clone());
            }
            if list.len() > 3 {
                parts.push("...".to_string());
            }
            format!("[{}]", parts.join(", "))
        })
        .unwrap_or_else(|| "none".to_string());
    let local_digest = inspect
        .repo_digests
        .as_deref()
        .and_then(|list| local_repo_digest(list, &repo));

    let (status, remote_digest, error, debug_remote, debug_remote_digests, debug_local_index) =
        if let Some(digest) = normalized.digest.clone() {
        (
            ImageUpdateKind::UpToDate,
            Some(digest),
            None::<String>,
            None::<String>,
            None::<String>,
            None::<String>,
        )
    } else {
        match docker::fetch_manifest_inspect(runner, docker, &normalized.reference).await {
            Ok(raw) => {
                let summary = manifest_platform_summary(&raw);
                let remote_digests = manifest_remote_digests(&raw);
                let remote = if let (Some(arch), Some(os)) =
                    (inspect.architecture.as_deref(), inspect.os.as_deref())
                {
                    manifest_digest_for_platform(&raw, arch, os)
                        .or_else(|| manifest_descriptor_digest(&raw))
                } else {
                    manifest_descriptor_digest(&raw)
                };
                let (status, error, local_index_debug) = match (local_digest.clone(), remote.clone()) {
                    (Some(local), Some(remote)) => {
                        let inspect_arch = inspect.architecture.as_deref().unwrap_or("");
                        let inspect_os = inspect.os.as_deref().unwrap_or("");
                        let matches = remote_digests.iter().any(|(digest, arch, os)| {
                            if digest != &local {
                                return false;
                            }
                            let arch = arch.as_deref().unwrap_or("");
                            let os = os.as_deref().unwrap_or("");
                            if arch.is_empty() || os.is_empty() {
                                return true;
                            }
                            if arch == "unknown" || os == "unknown" {
                                return true;
                            }
                            arch == inspect_arch && os == inspect_os
                        });
                        if matches {
                            (ImageUpdateKind::UpToDate, None, None)
                        } else {
                            let idx_ref = format!("{}@{}", normalized.reference, local);
                            match docker::fetch_manifest_inspect(runner, docker, &idx_ref).await {
                                Ok(idx_raw) => {
                                    let idx_digest = if !inspect_arch.is_empty() && !inspect_os.is_empty() {
                                        manifest_digest_for_platform(&idx_raw, inspect_arch, inspect_os)
                                            .or_else(|| manifest_descriptor_digest(&idx_raw))
                                    } else {
                                        manifest_descriptor_digest(&idx_raw)
                                    };
                                    let idx_summary = manifest_platform_summary(&idx_raw);
                                    let idx_debug = Some(format!(
                                        "local_index_ref={idx_ref} local_index_platforms={idx_summary} local_index_digest={}",
                                        idx_digest.as_deref().unwrap_or("-")
                                    ));
                                    if let Some(idx_digest) = idx_digest {
                                        if idx_digest == remote {
                                            (
                                                ImageUpdateKind::UpToDate,
                                                None,
                                                idx_debug,
                                            )
                                        } else {
                                            (
                                                ImageUpdateKind::UpdateAvailable,
                                                None,
                                                idx_debug,
                                            )
                                        }
                                    } else {
                                        (
                                            ImageUpdateKind::UpdateAvailable,
                                            None,
                                            idx_debug,
                                        )
                                    }
                                }
                                Err(_) => (ImageUpdateKind::UpdateAvailable, None, None),
                            }
                        }
                    }
                    (None, _) => {
                        (
                            ImageUpdateKind::Error,
                            Some(format!(
                                "missing local digest (repo={repo}, repo_digests={repo_digests_len} {repo_digests_preview})"
                            )),
                            None,
                        )
                    }
                    (_, None) => {
                        (
                            ImageUpdateKind::Error,
                            Some(format!("missing remote digest (platforms={summary})")),
                            None,
                        )
                    }
                };
                (
                    status,
                    remote,
                    error,
                    Some(summary),
                    Some(format_remote_digest_list(&remote_digests)),
                    local_index_debug,
                )
            }
            Err(e) => (
                ImageUpdateKind::Error,
                None,
                Some(truncate_msg(&format!("{:#}", e), 200)),
                None,
                None,
                None,
            ),
        }
    };

    let debug_info = if debug {
        let arch = inspect.architecture.as_deref().unwrap_or("-");
        let os = inspect.os.as_deref().unwrap_or("-");
        let local = local_digest.as_deref().unwrap_or("-");
        let remote = remote_digest.as_deref().unwrap_or("-");
        let mut parts = vec![
            format!("image={}", normalized.reference),
            format!("repo={repo}"),
            format!("inspect_platform={arch}/{os}"),
            format!("local_digest={local}"),
            format!("remote_digest={remote}"),
            format!(
                "repo_digests={repo_digests_len} {repo_digests_preview}"
            ),
        ];
        if let Some(summary) = debug_remote.as_deref() {
            parts.push(format!("remote_platforms={summary}"));
        }
        if let Some(list) = debug_remote_digests.as_deref() {
            parts.push(format!("remote_digests={list}"));
        }
        if let Some(idx) = debug_local_index.as_deref() {
            parts.push(idx.to_string());
        }
        Some(parts.join(" | "))
    } else {
        None
    };

    let entry = ImageUpdateEntry {
        checked_at: now_unix(),
        status,
        local_digest,
        remote_digest,
        error,
    };
    let result = ImageUpdateResult {
        image: normalized.reference.clone(),
        entry,
        debug: debug_info,
    };
    Ok(serde_json::to_string(&result)?)
}

fn format_duration_ns(ns: i64) -> Option<String> {
    if ns <= 0 {
        return None;
    }
    const NS_PER_S: i64 = 1_000_000_000;
    const NS_PER_MS: i64 = 1_000_000;
    const NS_PER_US: i64 = 1_000;
    if ns % NS_PER_S == 0 {
        Some(format!("{}s", ns / NS_PER_S))
    } else if ns % NS_PER_MS == 0 {
        Some(format!("{}ms", ns / NS_PER_MS))
    } else if ns % NS_PER_US == 0 {
        Some(format!("{}us", ns / NS_PER_US))
    } else {
        Some(format!("{}ns", ns))
    }
}

fn filter_labels(labels: &HashMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in labels {
        if k.starts_with("com.docker.compose.")
            || k.starts_with("com.docker.stack.")
            || k.starts_with("com.docker.swarm.")
        {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    out
}

fn write_stack_template_compose(
    templates_dir: &PathBuf,
    name: &str,
    compose: &str,
) -> anyhow::Result<PathBuf> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    fs::create_dir_all(templates_dir)?;
    let dir = templates_dir.join(name);
    if dir.exists() && !dir.is_dir() {
        anyhow::bail!("template path exists but is not a directory: {}", dir.display());
    }
    fs::create_dir_all(&dir)?;
    let compose_path = dir.join("compose.yaml");
    fs::write(&compose_path, compose)?;
    Ok(compose_path)
}

async fn export_stack_template(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    source: &str,
    stack_name: Option<&str>,
    container_ids: &[String],
    templates_dir: &PathBuf,
) -> anyhow::Result<String> {
    anyhow::ensure!(!container_ids.is_empty(), "no containers selected");
    let raw = docker::fetch_inspects(runner, docker, container_ids).await?;
    let mut inspects: Vec<ContainerInspect> =
        serde_json::from_str(&raw).context("inspect output was not JSON array")?;
    if inspects.is_empty() {
        anyhow::bail!("no container inspect data returned");
    }

    inspects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let mut warnings: Vec<String> = Vec::new();
    let mut network_names: BTreeSet<String> = BTreeSet::new();
    for inspect in &inspects {
        if let Some(settings) = &inspect.network_settings {
            if let Some(nets) = &settings.networks {
                for name in nets.keys() {
                    if is_system_network_name(name) {
                        continue;
                    }
                    network_names.insert(name.clone());
                }
            }
        }
    }

    let mut networks: Vec<NetworkInspect> = Vec::new();
    for name in &network_names {
        match docker::fetch_network_inspect(runner, docker, name).await {
            Ok(raw) => match serde_json::from_str::<NetworkInspect>(&raw) {
                Ok(net) => networks.push(net),
                Err(e) => warnings.push(format!("network inspect parse failed for {name}: {e:#}")),
            },
            Err(e) => warnings.push(format!("network inspect failed for {name}: {e:#}")),
        }
    }

    let stack_name = stack_name.map(|s| s.to_string()).or_else(|| {
        inspects
            .first()
            .and_then(|inspect| inspect.config.as_ref())
            .and_then(|cfg| cfg.labels.as_ref())
            .and_then(stack_name_from_label_map)
    });
    let compose = build_compose_yaml(name, stack_name.as_deref(), source, &inspects, &networks);
    write_stack_template_compose(templates_dir, name, &compose)?;
    Ok(warnings.join("\n"))
}

fn build_compose_yaml(
    template_name: &str,
    stack_name: Option<&str>,
    source: &str,
    inspects: &[ContainerInspect],
    networks: &[NetworkInspect],
) -> String {
    let mut service_counts: HashMap<String, usize> = HashMap::new();
    for inspect in inspects {
        let labels = inspect
            .config
            .as_ref()
            .and_then(|cfg| cfg.labels.as_ref())
            .cloned()
            .unwrap_or_default();
        let name = inspect.name.trim_start_matches('/').to_string();
        let stack_hint = stack_name
            .map(|v| v.to_string())
            .or_else(|| stack_name_from_label_map(&labels));
        let svc = service_name_from_labels(&labels, stack_hint.as_deref(), &name);
        *service_counts.entry(svc).or_insert(0) += 1;
    }

    let mut services: BTreeMap<String, ComposeService> = BTreeMap::new();
    let mut volume_defs: BTreeMap<String, ComposeVolume> = BTreeMap::new();
    let mut network_refs: BTreeSet<String> = BTreeSet::new();

    for inspect in inspects {
        let labels = inspect
            .config
            .as_ref()
            .and_then(|cfg| cfg.labels.as_ref())
            .cloned()
            .unwrap_or_default();
        let container_name = inspect.name.trim_start_matches('/').to_string();
        let stack_hint = stack_name
            .map(|v| v.to_string())
            .or_else(|| stack_name_from_label_map(&labels));
        let service_name = service_name_from_labels(&labels, stack_hint.as_deref(), &container_name);
        let entry = services.entry(service_name.clone()).or_default();

        if entry.image.is_empty() {
            entry.image = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.image.as_ref())
                .map(|v| normalize_image_ref(v))
                .unwrap_or_default();
            entry.container_name = if service_counts.get(&service_name).copied().unwrap_or(1) > 1 {
                None
            } else if container_name.is_empty() {
                None
            } else {
                Some(container_name.clone())
            };
            entry.command = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.cmd.clone())
                .unwrap_or_default();
            entry.entrypoint = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.entrypoint.clone())
                .unwrap_or_default();
            let mut env = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.env.clone())
                .unwrap_or_default();
            env.sort();
            entry.environment = env;
            entry.labels = filter_labels(&labels);
            entry.working_dir = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.working_dir.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            entry.user = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.user.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            entry.healthcheck = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.healthcheck.as_ref())
                .and_then(|hc| hc.test.clone())
                .filter(|test| !test.is_empty())
                .map(|test| ComposeHealthcheck {
                    test,
                    interval: hc_duration(hc_interval(&inspect)),
                    timeout: hc_duration(hc_timeout(&inspect)),
                    retries: hc_retries(&inspect),
                    start_period: hc_duration(hc_start_period(&inspect)),
                });
            entry.restart = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.restart_policy.as_ref())
                .and_then(|rp| rp.name.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty() && v != "no");
            entry.read_only = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.readonly_rootfs);
            entry.privileged = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.privileged);
            let mut extra_hosts = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.extra_hosts.clone())
                .unwrap_or_default();
            extra_hosts.sort();
            entry.extra_hosts = extra_hosts;

            let network_mode = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.network_mode.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty() && v != "default" && v != "bridge");
            entry.network_mode = network_mode;

            if let Some(host_cfg) = &inspect.host_config {
                if let Some(bindings) = &host_cfg.port_bindings {
                    let mut ports = Vec::new();
                    let mut expose = Vec::new();
                    for (key, binds) in bindings {
                        let container_port = key.split('/').next().unwrap_or(key).trim();
                        if container_port.is_empty() {
                            continue;
                        }
                        if binds.is_empty() {
                            expose.push(container_port.to_string());
                            continue;
                        }
                        for binding in binds {
                            let host_port = binding
                                .host_port
                                .as_deref()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            if host_port.is_empty() {
                                expose.push(container_port.to_string());
                                continue;
                            }
                            let host_ip = binding
                                .host_ip
                                .as_deref()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let spec = if host_ip.is_empty() || host_ip == "0.0.0.0" {
                                format!("{host_port}:{container_port}")
                            } else {
                                format!("{host_ip}:{host_port}:{container_port}")
                            };
                            ports.push(spec);
                        }
                    }
                    ports.sort();
                    expose.sort();
                    entry.ports = ports;
                    entry.expose = expose;
                }
            }

            if entry.ports.is_empty() && entry.expose.is_empty() {
                if let Some(exposed) = inspect
                    .config
                    .as_ref()
                    .and_then(|cfg| cfg.exposed_ports.as_ref())
                {
                    let mut expose = Vec::new();
                    for key in exposed.keys() {
                        let port = key.split('/').next().unwrap_or(key).trim();
                        if !port.is_empty() {
                            expose.push(port.to_string());
                        }
                    }
                    expose.sort();
                    entry.expose = expose;
                }
            }

            if let Some(mounts) = &inspect.mounts {
                let mut volumes = Vec::new();
                let mut tmpfs = Vec::new();
                for mount in mounts {
                    let kind = mount.kind.as_deref().unwrap_or("").to_ascii_lowercase();
                    let dest = mount.destination.as_deref().unwrap_or("").trim().to_string();
                    if dest.is_empty() {
                        continue;
                    }
                    match kind.as_str() {
                        "bind" => {
                            let source = mount.source.as_deref().unwrap_or("").trim();
                            if source.is_empty() {
                                continue;
                            }
                            let mut spec = format!("{source}:{dest}");
                            if mount.read_only.unwrap_or(false) {
                                spec.push_str(":ro");
                            }
                            volumes.push(spec);
                        }
                        "volume" => {
                            let name = mount
                                .name
                                .as_deref()
                                .map(|v| v.trim().to_string())
                                .filter(|v| !v.is_empty())
                                .or_else(|| {
                                    mount
                                        .source
                                        .as_deref()
                                        .map(|v| v.trim().to_string())
                                        .filter(|v| !v.is_empty())
                                });
                            if let Some(name) = name {
                                let mut spec = format!("{name}:{dest}");
                                if mount.read_only.unwrap_or(false) {
                                    spec.push_str(":ro");
                                }
                                volumes.push(spec);
                                volume_defs.entry(name).or_insert_with(|| ComposeVolume {
                                    driver: mount
                                        .driver
                                        .as_deref()
                                        .map(|v| v.trim().to_string())
                                        .filter(|v| !v.is_empty()),
                                });
                            }
                        }
                        "tmpfs" => {
                            tmpfs.push(dest);
                        }
                        _ => {}
                    }
                }
                volumes.sort();
                tmpfs.sort();
                entry.volumes = volumes;
                entry.tmpfs = tmpfs;
            }
        }

        if entry.network_mode.is_none() {
            if let Some(settings) = &inspect.network_settings {
                if let Some(nets) = &settings.networks {
                    for (name, attachment) in nets {
                        if is_system_network_name(name) {
                            continue;
                        }
                        network_refs.insert(name.clone());
                        let svc_net = entry.networks.entry(name.clone()).or_default();
                        if let Some(aliases) = &attachment.aliases {
                            for alias in aliases {
                                let alias = alias.trim();
                                if alias.is_empty() || svc_net.aliases.contains(&alias.to_string())
                                {
                                    continue;
                                }
                                svc_net.aliases.push(alias.to_string());
                            }
                        }
                        if svc_net.ipv4_address.is_none() {
                            if let Some(ip) = &attachment.ip_address {
                                let ip = ip.trim();
                                if !ip.is_empty() {
                                    svc_net.ipv4_address = Some(ip.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut service_keys: HashMap<String, String> = HashMap::new();
    let mut used_service_keys: HashSet<String> = HashSet::new();
    for name in services.keys() {
        service_keys.insert(name.clone(), unique_compose_key(name, &mut used_service_keys));
    }

    let mut network_keys: HashMap<String, String> = HashMap::new();
    let mut used_network_keys: HashSet<String> = HashSet::new();
    for name in &network_refs {
        network_keys.insert(name.clone(), unique_compose_key(name, &mut used_network_keys));
    }

    let mut network_defs: BTreeMap<String, ComposeNetwork> = BTreeMap::new();
    for net in networks {
        if !network_refs.contains(&net.name) {
            continue;
        }
        let key = network_keys
            .get(&net.name)
            .cloned()
            .unwrap_or_else(|| net.name.clone());
        let mut entry = ComposeNetwork {
            name: net.name.clone(),
            driver: net
                .driver
                .as_deref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            internal: net.internal,
            attachable: net.attachable,
            enable_ipv6: net.enable_ipv6,
            ipam: None,
            options: BTreeMap::new(),
            labels: BTreeMap::new(),
        };
        if let Some(opts) = &net.options {
            for (k, v) in opts {
                entry.options.insert(k.clone(), v.clone());
            }
        }
        if let Some(labels) = &net.labels {
            entry.labels = filter_labels(labels);
        }
        if let Some(ipam) = &net.ipam {
            let mut configs = Vec::new();
            if let Some(cfgs) = &ipam.config {
                for cfg in cfgs {
                    if cfg.subnet.is_none() && cfg.gateway.is_none() && cfg.ip_range.is_none() {
                        continue;
                    }
                    configs.push(ComposeNetworkIpamConfig {
                        subnet: cfg.subnet.clone(),
                        gateway: cfg.gateway.clone(),
                        ip_range: cfg.ip_range.clone(),
                    });
                }
            }
            if !configs.is_empty() || ipam.driver.as_ref().is_some() {
                entry.ipam = Some(ComposeNetworkIpam {
                    driver: ipam.driver.clone(),
                    config: configs,
                });
            }
        }
        network_defs.insert(key, entry);
    }

    for (name, key) in &network_keys {
        if !network_defs.contains_key(key) {
            network_defs.insert(
                key.clone(),
                ComposeNetwork {
                    name: name.clone(),
                    ..ComposeNetwork::default()
                },
            );
        }
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("# Generated by containr".to_string());
    lines.push(format!("# Source: {source}"));
    lines.push(format!("# description: Exported from {source}"));
    lines.push("".to_string());
    lines.push(format!("name: {}", yaml_quote(template_name)));
    lines.push("".to_string());
    lines.push("services:".to_string());
    for (svc_name, svc) in &services {
        let key = service_keys
            .get(svc_name)
            .cloned()
            .unwrap_or_else(|| svc_name.clone());
        lines.push(format!("  {key}:"));
        if !svc.image.is_empty() {
            lines.push(format!("    image: {}", yaml_quote(&svc.image)));
        }
        if let Some(name) = &svc.container_name {
            lines.push(format!("    container_name: {}", yaml_quote(name)));
        }
        if !svc.command.is_empty() {
            lines.push("    command:".to_string());
            for part in &svc.command {
                lines.push(format!("      - {}", yaml_quote(part)));
            }
        }
        if !svc.entrypoint.is_empty() {
            lines.push("    entrypoint:".to_string());
            for part in &svc.entrypoint {
                lines.push(format!("      - {}", yaml_quote(part)));
            }
        }
        if let Some(workdir) = &svc.working_dir {
            lines.push(format!("    working_dir: {}", yaml_quote(workdir)));
        }
        if let Some(user) = &svc.user {
            lines.push(format!("    user: {}", yaml_quote(user)));
        }
        if let Some(restart) = &svc.restart {
            lines.push(format!("    restart: {}", yaml_quote(restart)));
        }
        if let Some(privileged) = svc.privileged {
            lines.push(format!("    privileged: {}", privileged));
        }
        if let Some(read_only) = svc.read_only {
            lines.push(format!("    read_only: {}", read_only));
        }
        if let Some(mode) = &svc.network_mode {
            lines.push(format!("    network_mode: {}", yaml_quote(mode)));
        }
        if !svc.extra_hosts.is_empty() {
            lines.push("    extra_hosts:".to_string());
            for host in &svc.extra_hosts {
                lines.push(format!("      - {}", yaml_quote(host)));
            }
        }
        if !svc.environment.is_empty() {
            lines.push("    environment:".to_string());
            for env in &svc.environment {
                lines.push(format!("      - {}", yaml_quote(env)));
            }
        }
        if !svc.labels.is_empty() {
            lines.push("    labels:".to_string());
            for (k, v) in &svc.labels {
                lines.push(format!("      {}: {}", yaml_quote(k), yaml_quote(v)));
            }
        }
        if !svc.ports.is_empty() {
            lines.push("    ports:".to_string());
            for port in &svc.ports {
                lines.push(format!("      - {}", yaml_quote(port)));
            }
        }
        if !svc.expose.is_empty() {
            lines.push("    expose:".to_string());
            for port in &svc.expose {
                lines.push(format!("      - {}", yaml_quote(port)));
            }
        }
        if !svc.volumes.is_empty() {
            lines.push("    volumes:".to_string());
            for volume in &svc.volumes {
                lines.push(format!("      - {}", yaml_quote(volume)));
            }
        }
        if !svc.tmpfs.is_empty() {
            lines.push("    tmpfs:".to_string());
            for dest in &svc.tmpfs {
                lines.push(format!("      - {}", yaml_quote(dest)));
            }
        }
        if let Some(hc) = &svc.healthcheck {
            lines.push("    healthcheck:".to_string());
            lines.push("      test:".to_string());
            for part in &hc.test {
                lines.push(format!("        - {}", yaml_quote(part)));
            }
            if let Some(interval) = &hc.interval {
                lines.push(format!("      interval: {}", yaml_quote(interval)));
            }
            if let Some(timeout) = &hc.timeout {
                lines.push(format!("      timeout: {}", yaml_quote(timeout)));
            }
            if let Some(retries) = hc.retries {
                lines.push(format!("      retries: {}", retries));
            }
            if let Some(start_period) = &hc.start_period {
                lines.push(format!("      start_period: {}", yaml_quote(start_period)));
            }
        }
        if svc.network_mode.is_none() && !svc.networks.is_empty() {
            let has_options = svc.networks.values().any(|n| {
                !n.aliases.is_empty() || n.ipv4_address.as_ref().is_some_and(|v| !v.is_empty())
            });
            lines.push("    networks:".to_string());
            for (name, opts) in &svc.networks {
                let key = network_keys
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                if has_options {
                    if opts.aliases.is_empty()
                        && opts
                            .ipv4_address
                            .as_ref()
                            .map(|v| v.is_empty())
                            .unwrap_or(true)
                    {
                        lines.push(format!("      {key}: {{}}"));
                        continue;
                    }
                    lines.push(format!("      {key}:"));
                    if !opts.aliases.is_empty() {
                        lines.push("        aliases:".to_string());
                        for alias in &opts.aliases {
                            lines.push(format!("          - {}", yaml_quote(alias)));
                        }
                    }
                    if let Some(ip) = &opts.ipv4_address {
                        if !ip.is_empty() {
                            lines.push(format!("        ipv4_address: {}", yaml_quote(ip)));
                        }
                    }
                } else {
                    lines.push(format!("      - {key}"));
                }
            }
        }
    }

    if !volume_defs.is_empty() {
        lines.push("".to_string());
        lines.push("volumes:".to_string());
        for (name, vol) in &volume_defs {
            if let Some(driver) = &vol.driver {
                lines.push(format!("  {}:", yaml_quote(name)));
                lines.push(format!("    driver: {}", yaml_quote(driver)));
            } else {
                lines.push(format!("  {}: {{}}", yaml_quote(name)));
            }
        }
    }

    if !network_defs.is_empty() {
        lines.push("".to_string());
        lines.push("networks:".to_string());
        for (key, net) in &network_defs {
            lines.push(format!("  {key}:"));
            lines.push(format!("    name: {}", yaml_quote(&net.name)));
            if let Some(driver) = &net.driver {
                lines.push(format!("    driver: {}", yaml_quote(driver)));
            }
            if let Some(internal) = net.internal {
                lines.push(format!("    internal: {}", internal));
            }
            if let Some(attachable) = net.attachable {
                lines.push(format!("    attachable: {}", attachable));
            }
            if let Some(enable_ipv6) = net.enable_ipv6 {
                lines.push(format!("    enable_ipv6: {}", enable_ipv6));
            }
            if let Some(ipam) = &net.ipam {
                lines.push("    ipam:".to_string());
                if let Some(driver) = &ipam.driver {
                    lines.push(format!("      driver: {}", yaml_quote(driver)));
                }
                if !ipam.config.is_empty() {
                    lines.push("      config:".to_string());
                    for cfg in &ipam.config {
                        lines.push("        -".to_string());
                        if let Some(subnet) = &cfg.subnet {
                            lines.push(format!("          subnet: {}", yaml_quote(subnet)));
                        }
                        if let Some(gateway) = &cfg.gateway {
                            lines.push(format!("          gateway: {}", yaml_quote(gateway)));
                        }
                        if let Some(ip_range) = &cfg.ip_range {
                            lines.push(format!("          ip_range: {}", yaml_quote(ip_range)));
                        }
                    }
                }
            }
            if !net.options.is_empty() {
                lines.push("    options:".to_string());
                for (k, v) in &net.options {
                    lines.push(format!("      {}: {}", yaml_quote(k), yaml_quote(v)));
                }
            }
            if !net.labels.is_empty() {
                lines.push("    labels:".to_string());
                for (k, v) in &net.labels {
                    lines.push(format!("      {}: {}", yaml_quote(k), yaml_quote(v)));
                }
            }
        }
    }

    if lines.last().is_some_and(|l| !l.is_empty()) {
        lines.push(String::new());
    }
    lines.join("\n")
}

fn hc_interval(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.interval)
}

fn hc_timeout(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.timeout)
}

fn hc_start_period(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.start_period)
}

fn hc_retries(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.retries)
}

fn hc_duration(value: Option<i64>) -> Option<String> {
    value.and_then(format_duration_ns)
}

fn create_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let dir = stacks_dir.join(name);
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;
    let compose = dir.join("compose.yaml");
    let skeleton = r#"# Stack template (docker compose)
# description: REPLACE_WITH_A_SHORT_DESCRIPTION
#
# Tips:
# - Keep values simple and edit after creation.
# - Add more services as needed.
# - Use named volumes for persistent data.
#
# Docs: https://docs.docker.com/compose/compose-file/

name: REPLACE_STACK_NAME

services:
  app:
    image: REPLACE_IMAGE:latest
    container_name: REPLACE_CONTAINER_NAME
    restart: unless-stopped

    # Optional: publish ports (host:container)
    ports:
      - "8080:80"

    # Optional: environment variables
    environment:
      TZ: "UTC"
      EXAMPLE: "value"

    # Optional: bind-mounts or named volumes
    volumes:
      - app_data:/var/lib/app

    # Optional: networks (useful when you run multiple services)
    networks:
      - app_net

    # Optional: healthcheck
    healthcheck:
      test: ["CMD", "sh", "-lc", "curl -fsS http://localhost/ || exit 1"]
      interval: 30s
      timeout: 5s
      retries: 3

    # Optional: labels (containr can add its own labels during deploy later)
    labels:
      com.example.stack: "REPLACE_STACK_NAME"

volumes:
  app_data:
    driver: local

networks:
  app_net:
    driver: bridge
"#;
    fs::write(&compose, skeleton)?;
    Ok(())
}

fn create_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let root = app.net_templates_dir();
    fs::create_dir_all(&root)?;
    let dir = root.join(name);
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;

    let cfg = dir.join("network.json");
    let skeleton = format!(
        r#"{{
  "description": "Shared network template (edit me)",
  "name": "{name}",
  "driver": "ipvlan",
  "parent": "eth0.10",
  "ipvlan_mode": "l2",
  "ipv4": {{
    "subnet": "192.168.10.0/24",
    "gateway": "192.168.10.1",
    "ip_range": null
  }},
  "internal": null,
  "attachable": null,
  "options": {{}},
  "labels": {{}}
}}
"#
    );
    fs::write(&cfg, skeleton)?;
    Ok(())
}

fn deploy_remote_dir_for(name: &str) -> String {
    format!(".config/containr/apps/{name}")
}

fn deploy_remote_net_dir_for(name: &str) -> String {
    format!(".config/containr/networks/{name}")
}

fn delete_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let dir = stacks_dir.join(name);
    anyhow::ensure!(dir.exists(), "template does not exist: {}", dir.display());

    let root = fs::canonicalize(&stacks_dir)?;
    let target = fs::canonicalize(&dir)?;
    anyhow::ensure!(
        target.starts_with(&root),
        "refusing to delete outside templates dir"
    );

    fs::remove_dir_all(&target)?;
    if let Some(info) = extract_template_id(&dir.join("compose.yaml")) {
        if app.template_deploys.remove(&info).is_some() {
            app.save_local_state();
        }
    }
    Ok(())
}

fn delete_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let root = app.net_templates_dir();
    fs::create_dir_all(&root)?;
    let dir = root.join(name);
    anyhow::ensure!(dir.exists(), "template does not exist: {}", dir.display());

    let root_can = fs::canonicalize(&root)?;
    let dir_can = fs::canonicalize(&dir)?;
    anyhow::ensure!(
        dir_can.starts_with(&root_can),
        "refusing to delete outside templates dir"
    );

    fs::remove_dir_all(&dir_can)?;
    Ok(())
}

fn maybe_autocommit_templates(app: &mut App, kind: TemplatesKind, action: &str, name: &str) {
    if !app.git_autocommit {
        return;
    }
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    if !commands::git_cmd::git_available() {
        return;
    }
    let dir = app.templates_state.dir.clone();
    if !commands::git_cmd::is_git_repo(&dir) {
        app.log_msg(
            MsgLevel::Warn,
            "git autocommit is enabled but templates repo is not initialized".to_string(),
        );
        return;
    }
    let status = match commands::git_cmd::run_git(&dir, &["status", "--porcelain"]) {
        Ok(out) => out,
        Err(e) => {
            app.log_msg(MsgLevel::Warn, format!("git autocommit skipped: {e:#}"));
            return;
        }
    };
    if status.trim().is_empty() {
        return;
    }
    let kind_label = match kind {
        TemplatesKind::Stacks => "stack",
        TemplatesKind::Networks => "network",
    };
    let msg = format!("templates: {action} {kind_label} {name}");
    if app.git_autocommit_confirm {
        let cmdline = format!(
            "git templates autocommit -m {}",
            shell_escape_sh_arg(&msg)
        );
        shell_begin_confirm(app, "git autocommit", cmdline);
        return;
    }
    if let Err(e) = commands::git_cmd::run_git(&dir, &["add", "-A"]) {
        app.log_msg(MsgLevel::Warn, format!("git autocommit failed: {e:#}"));
        return;
    }
    match commands::git_cmd::run_git(&dir, &["commit", "-m", msg.as_str()]) {
        Ok(out) => {
            if out.trim().is_empty() {
                app.log_msg(MsgLevel::Info, format!("git autocommit: {msg}"));
            } else {
                app.log_msg(MsgLevel::Info, format!("git autocommit: {out}"));
            }
        }
        Err(e) => app.log_msg(MsgLevel::Warn, format!("git autocommit failed: {e:#}")),
    }
}

fn parse_kv_args(
    mut it: impl Iterator<Item = String>,
) -> (
    Option<u16>,
    Option<String>,
    Option<crate::config::DockerCmd>,
    Vec<String>,
) {
    // Supports: -p <port>  -i <identity>  --cmd <docker_cmd>
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut docker_cmd: Option<crate::config::DockerCmd> = None;
    let mut rest: Vec<String> = Vec::new();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "-p" => {
                if let Some(v) = it.next() {
                    port = v.parse::<u16>().ok();
                }
            }
            "-i" => {
                if let Some(v) = it.next() {
                    identity = Some(v);
                }
            }
            "--cmd" => {
                if let Some(v) = it.next() {
                    let parsed = crate::shell_parse::parse_shell_tokens(&v)
                        .ok()
                        .unwrap_or_else(|| vec![v]);
                    if parsed.is_empty() {
                        docker_cmd = Some(crate::config::DockerCmd::default());
                    } else {
                        docker_cmd = Some(crate::config::DockerCmd::new(parsed));
                    }
                }
            }
            _ => rest.push(tok),
        }
    }
    (port, identity, docker_cmd, rest)
}

fn extract_template_description(path: &PathBuf) -> Option<String> {
    // Heuristic: find a "# description: ..." (or "# desc: ...") line near the top of compose.yaml.
    let data = fs::read_to_string(path).ok()?;
    for line in data.lines().take(40) {
        let l = line.trim_start();
        if !l.starts_with('#') {
            // Stop early once we hit non-comment content.
            if !l.is_empty() {
                break;
            }
            continue;
        }
        let body = l.trim_start_matches('#').trim_start();
        let low = body.to_ascii_lowercase();
        let key = if low.starts_with("description:") {
            "description:"
        } else if low.starts_with("desc:") {
            "desc:"
        } else {
            continue;
        };
        let value = body[key.len()..].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn extract_template_id(path: &PathBuf) -> Option<String> {
    // Heuristic: find a "# containr_template_id: ..." line near the top of compose.yaml.
    let data = fs::read_to_string(path).ok()?;
    for line in data.lines().take(40) {
        let l = line.trim_start();
        if !l.starts_with('#') {
            if !l.is_empty() {
                break;
            }
            continue;
        }
        let body = l.trim_start_matches('#').trim_start();
        let low = body.to_ascii_lowercase();
        if !low.starts_with("containr_template_id:") {
            continue;
        }
        let value = body["containr_template_id:".len()..].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn ensure_template_id(path: &PathBuf) -> anyhow::Result<String> {
    if let Some(existing) = extract_template_id(path) {
        return Ok(existing);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let data = fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!("# containr_template_id: {id}\n"));
    out.push_str(&data);
    fs::write(path, out)?;
    Ok(id)
}

fn extract_net_template_description(path: &PathBuf) -> Option<String> {
    let data = fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;
    let d = v.get("description")?.as_str()?.trim();
    if d.is_empty() {
        None
    } else {
        Some(d.to_string())
    }
}

fn shell_is_safe_token(s: &str) -> bool {
    // For interactive shells we only accept simple command tokens.
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
}

fn shell_escape_double_quoted(s: &str) -> String {
    // Escape for inclusion inside double quotes in a POSIX shell script.
    // We escape: backslash, double quote, dollar, backtick.
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' | '"' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn parse_cmdline_tokens(input: &str) -> Result<Vec<String>, String> {
    crate::shell_parse::parse_shell_tokens(input)
}

fn shell_open_console(app: &mut App, user: Option<&str>, shell: &str) {
    let Some(c) = app.selected_container() else {
        app.set_warn("no container selected");
        return;
    };
    if is_container_stopped(&c.status) {
        app.set_warn("container is not running");
        return;
    }
    if !shell_is_safe_token(shell) {
        app.set_warn("invalid shell");
        return;
    }
    let docker_cmd = current_docker_cmd_from_app(app).to_shell();
    let id = shell_single_quote(&c.id);
    let server = current_server_label(app);
    // Bash interprets prompt escapes like \\e and needs \\[ \\] wrappers for correct line editing.
    let ps1_bash = format!(
        "\\[\\e[37m\\]docker:\\[\\e[0m\\]\\[\\e[32m\\]{}\\[\\e[37m\\]@{}\\[\\e[0m\\]$ ",
        c.name, server
    );
    let ps1_bash = shell_single_quote(&ps1_bash);

    let user_part = user
        .filter(|u| !u.trim().is_empty())
        .map(|u| format!("-u {}", shell_single_quote(u.trim())))
        .unwrap_or_default();

    let shell_cmd = if shell == "bash" {
        format!("env PS1={ps1_bash} bash --noprofile --norc -i")
    } else if shell == "sh" {
        // POSIX sh typically does NOT interpret \\e-style escapes in PS1. We set PS1 via printf
        // using %b so that \\033 sequences become real ESC bytes, then exec an interactive sh.
        // Important: avoid nested single quotes here, because this command is embedded into other
        // shell layers (ssh/sh -lc).
        let ps1_sh_raw = format!(
            "\\033[37mdocker:\\033[0m\\033[32m{}\\033[37m@{}\\033[0m\\$ ",
            c.name, server
        );
        let ps1_sh = shell_escape_double_quoted(&ps1_sh_raw);
        format!("sh -lc 'export PS1=\"$(printf \"%b\" \"{ps1_sh}\")\"; exec sh -i'")
    } else {
        // Best-effort generic token. Prompt coloring depends on the shell.
        format!("env PS1={ps1_bash} {shell}")
    };

    let cmd = if user_part.is_empty() {
        format!("{docker_cmd} exec -it {id} {shell_cmd}")
    } else {
        format!("{docker_cmd} exec -it {user_part} {id} {shell_cmd}")
    };
    app.shell_pending_interactive = Some(ShellInteractive::RunCommand { cmd });
}

fn shell_execute_cmdline(
    app: &mut App,
    cmdline: &str,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    refresh_interval_tx: &watch::Sender<Duration>,
    refresh_pause_tx: &watch::Sender<bool>,
    image_update_limit_tx: &watch::Sender<usize>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return;
    }
    let cmdline = cmdline.trim_start_matches(':').trim();
    let cmdline_full = cmdline.to_string();

    let tokens = match parse_cmdline_tokens(cmdline) {
        Ok(v) => v,
        Err(e) => {
            app.set_warn(format!("invalid command line: {e}"));
            return;
        }
    };
    let mut it = tokens.iter().map(|s| s.as_str());
    let Some(cmd_raw) = it.next() else {
        return;
    };
    let (cmd, force) = if cmd_raw == "!" {
        let Some(next) = it.next() else {
            app.set_warn("usage: :! <command>");
            return;
        };
        (next, true)
    } else if let Some(rest) = cmd_raw.strip_prefix('!') {
        if rest.is_empty() {
            app.set_warn("usage: :! <command>");
            return;
        }
        (rest, true)
    } else if let Some(stripped) = cmd_raw.strip_suffix('!') {
        (stripped, true)
    } else {
        (cmd_raw, false)
    };

    match cmd {
        "q" => {
            if force {
                app.should_quit = true;
            } else {
                app.shell_cmdline.mode = true;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.shell_cmdline.confirm = Some(ShellConfirm {
                    label: "quit".to_string(),
                    cmdline: cmdline_full,
                });
            }
            return;
        }
        "?" | "help" => {
            // Ensure we don't get "stuck" in command-line mode while the Help view is active.
            // Otherwise 'q' is treated as input and won't close Help.
            app.shell_cmdline.mode = false;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            app.shell_help.return_view = app.shell_view;
            app.shell_view = ShellView::Help;
            app.shell_focus = ShellFocus::List;
            app.shell_help.scroll = 0;
            return;
        }
        "messages" | "msgs" => {
            let sub = it.next().unwrap_or("");
            if sub == "copy" {
                app.messages_copy_selected();
                return;
            }
            let (force, wants_save) = if sub == "save!" {
                (true, true)
            } else if sub == "save" {
                (false, true)
            } else {
                (false, false)
            };
            if wants_save {
                let rest: Vec<&str> = it.collect();
                let path = rest.join(" ").trim().to_string();
                if path.is_empty() {
                    app.set_warn("usage: :messages save <file>");
                } else {
                    app.messages_save(&path, force);
                }
                return;
            }
            // Messages is a full-screen view; leaving cmdline mode avoids confusing key handling.
            app.shell_cmdline.mode = false;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            if app.shell_view == ShellView::Messages {
                shell_back_from_full(app);
            } else {
                app.mark_messages_seen();
                app.shell_msgs.return_view = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs.scroll = usize::MAX;
                app.shell_msgs.hscroll = 0;
            }
            return;
        }
        "ack" => {
            let sub = it.next().unwrap_or("");
            if sub == "all" {
                app.container_action_error.clear();
                app.image_action_error.clear();
                app.volume_action_error.clear();
                app.network_action_error.clear();
                app.template_action_error.clear();
                app.net_template_action_error.clear();
                app.set_info("cleared all action error markers");
                return;
            }
            match app.shell_view {
                ShellView::Dashboard => {}
                ShellView::Stacks => {}
                ShellView::Containers => {
                    let ids: Vec<String> = if !app.marked.is_empty() {
                        app.marked.iter().cloned().collect()
                    } else {
                        app.selected_container()
                            .map(|c| vec![c.id.clone()])
                            .unwrap_or_default()
                    };
                    for id in ids {
                        app.container_action_error.remove(&id);
                    }
                }
                ShellView::Images => {
                    let keys: Vec<String> = if !app.marked_images.is_empty() {
                        app.marked_images.iter().cloned().collect()
                    } else {
                        app.selected_image()
                            .map(|img| vec![App::image_row_key(img)])
                            .unwrap_or_default()
                    };
                    for k in keys {
                        app.image_action_error.remove(&k);
                    }
                }
                ShellView::Volumes => {
                    let names: Vec<String> = if !app.marked_volumes.is_empty() {
                        app.marked_volumes.iter().cloned().collect()
                    } else {
                        app.selected_volume()
                            .map(|v| vec![v.name.clone()])
                            .unwrap_or_default()
                    };
                    for n in names {
                        app.volume_action_error.remove(&n);
                    }
                }
                ShellView::Networks => {
                    let ids: Vec<String> = if !app.marked_networks.is_empty() {
                        app.marked_networks.iter().cloned().collect()
                    } else {
                        app.selected_network()
                            .map(|n| vec![n.id.clone()])
                            .unwrap_or_default()
                    };
                    for id in ids {
                        app.network_action_error.remove(&id);
                    }
                }
                ShellView::Templates => match app.templates_state.kind {
                    TemplatesKind::Stacks => {
                        let name = app.selected_template().map(|t| t.name.clone());
                        if let Some(name) = name {
                            app.template_action_error.remove(&name);
                        }
                    }
                    TemplatesKind::Networks => {
                        let name = app.selected_net_template().map(|t| t.name.clone());
                        if let Some(name) = name {
                            app.net_template_action_error.remove(&name);
                        }
                    }
                },
                ShellView::Logs
                | ShellView::Inspect
                | ShellView::Help
                | ShellView::Messages
                | ShellView::Registries => {}
            }
            app.set_info("cleared action error marker(s) for selection");
            return;
        }
        "refresh" => {
            if app.shell_view == ShellView::Templates {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => app.refresh_templates(),
                    TemplatesKind::Networks => app.refresh_net_templates(),
                }
            } else {
                shell_refresh(app, refresh_tx, dash_refresh_tx, refresh_pause_tx);
            }
            return;
        }
        "theme" => {
            let sub = it.next().unwrap_or("");
            if sub.is_empty() || sub == "help" {
                app.set_info(format!("active theme: {}", app.theme_name));
                app.set_info("usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>");
                        app.shell_msgs.return_view = app.shell_view;
                        app.shell_view = ShellView::Messages;
                        app.shell_focus = ShellFocus::List;
                        app.shell_msgs.scroll = usize::MAX;
                        return;
                    }
            match sub {
                "list" => match theme::list_theme_names(&app.config_path) {
                    Ok(mut names) => {
                        if names.is_empty() {
                            app.set_info("no themes found");
                        } else {
                            // Ensure default is always visible.
                            if !names.iter().any(|n| n == "default") {
                                names.insert(0, "default".to_string());
                            }
                            app.set_info("Themes:");
                            for n in names {
                                if n == app.theme_name {
                                    app.set_info(format!("* {n} (active)"));
                                } else {
                                    app.set_info(format!("  {n}"));
                                }
                            }
                        }
                        app.shell_msgs.return_view = app.shell_view;
                        app.shell_view = ShellView::Messages;
                        app.shell_focus = ShellFocus::List;
                        app.shell_msgs.scroll = usize::MAX;
                    }
                    Err(e) => app.set_error(format!("theme list failed: {:#}", e)),
                },
                "use" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme use <name>");
                        return;
                    };
                    if let Err(e) = commands::theme_cmd::set_theme(app, name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                "new" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme new <name>");
                        return;
                    };
                    if let Err(e) = commands::theme_cmd::new_theme(app, name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                "edit" => {
                    let name = it
                        .next()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| app.theme_name.clone());
                    if let Err(e) = commands::theme_cmd::edit_theme(app, &name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                "rm" | "del" | "delete" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme rm <name>");
                        return;
                    };
                    if name == "default" {
                        app.set_warn("cannot delete default theme");
                        return;
                    }
                    if !force {
                        shell_begin_confirm(app, format!("theme rm {name}"), cmdline_full.clone());
                        return;
                    }
                    if let Err(e) = commands::theme_cmd::delete_theme(app, name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                _ => app.set_warn("usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>"),
            }
            return;
        }
        "git" => {
            let args: Vec<&str> = it.collect();
            let _ = commands::git_cmd::handle_git(app, &args);
            return;
        }
        "map" => {
            let first = it.next().unwrap_or("");
            let rest: Vec<&str> = it.collect();
            let _ = commands::keymap_cmd::handle_map(app, first, &rest);
            return;
        }
        "unmap" => {
            let first = it.next().unwrap_or("");
            let rest: Vec<&str> = it.collect();
            let _ = commands::keymap_cmd::handle_unmap(app, first, &rest);
            return;
        }
        _ => {}
    }

    if cmd == "container" || cmd == "ctr" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = commands::container_cmd::handle_container(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "stack" || cmd == "stacks" || cmd == "stk" {
        let sub = it.next().unwrap_or("");
        let args: Vec<&str> = it.collect();
        let name = args.first().copied();
        if sub.is_empty() {
            shell_set_main_view(app, ShellView::Stacks);
            shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Stacks));
            return;
        }
        match sub {
            "running" => {
                app.stacks_only_running = !app.stacks_only_running;
                app.rebuild_stacks();
                app.set_info(format!(
                    "stacks filter: {}",
                    if app.stacks_only_running {
                        "running"
                    } else {
                        "all"
                    }
                ));
            }
            "all" => {
                app.stacks_only_running = false;
                app.rebuild_stacks();
                app.set_info("stacks filter: all");
            }
            "start" => {
                shell_exec_stack_action(app, ContainerAction::Start, name, action_req_tx);
            }
            "stop" => {
                shell_exec_stack_action(app, ContainerAction::Stop, name, action_req_tx);
            }
            "restart" => {
                shell_exec_stack_action(app, ContainerAction::Restart, name, action_req_tx);
            }
            "check" | "updates" => {
                if args.iter().any(|v| *v == "--pull" || *v == "pull") {
                    app.set_warn("usage: :stack check [name]");
                    return;
                }
                let target = name
                    .map(|s| s.to_string())
                    .or_else(|| app.selected_stack_entry().map(|s| s.name.clone()));
                let Some(target) = target else {
                    app.set_warn("no stack selected");
                    return;
                };
                let ids = app.stack_container_ids(&target);
                if ids.is_empty() {
                    app.set_warn("no containers in stack");
                    return;
                }
                let mut images: HashSet<String> = HashSet::new();
                for id in ids {
                    if let Some(idx) = app.container_idx_by_id.get(&id).copied() {
                        if let Some(c) = app.containers.get(idx) {
                            images.insert(c.image.clone());
                        }
                    }
                }
                shell_check_image_updates(app, images.into_iter().collect(), action_req_tx);
            }
            "recreate" => {
                let _ = force;
                app.set_warn("use :template deploy --recreate [--pull] <name>");
            }
            "rm" | "del" | "delete" => {
                let target = name
                    .map(|s| s.to_string())
                    .or_else(|| app.selected_stack_entry().map(|s| s.name.clone()));
                let Some(target) = target else {
                    app.set_warn("no stack selected");
                    return;
                };
                if !force {
                    shell_begin_confirm(
                        app,
                        format!("stack rm {target}"),
                        format!("stack rm {target}"),
                    );
                    return;
                }
                shell_exec_stack_action(app, ContainerAction::Remove, Some(&target), action_req_tx);
            }
            _ => {
        app.set_warn("usage: :stack [start|stop|restart|rm|check] [name] | :stacks running|all");
            }
        }
        return;
    }

    if cmd == "image" || cmd == "img" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = commands::image_cmd::handle_image(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "volume" || cmd == "vol" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = commands::volume_cmd::handle_volume(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "network" || cmd == "net" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = commands::network_cmd::handle_network(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "sidebar" {
        let sub = it.next().unwrap_or("toggle");
        let mut args: Vec<&str> = Vec::new();
        args.push(sub);
        args.extend(it);
        let _ = commands::sidebar_cmd::handle_sidebar(app, &args);
        return;
    }

    if cmd == "logs" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = commands::logs_cmd::handle_logs(app, &args, logs_req_tx);
        return;
    }

    if cmd == "set" {
        let args: Vec<&str> = it.collect();
        let _ = commands::set_cmd::handle_set(
            app,
            &args,
            refresh_interval_tx,
            image_update_limit_tx,
            logs_req_tx,
        );
        return;
    }

    if cmd == "layout" {
        let sub = it.next().unwrap_or("toggle");
        let mut args: Vec<&str> = Vec::new();
        args.push(sub);
        args.extend(it);
        let _ = commands::layout_cmd::handle_layout(app, &args);
        return;
    }

    if cmd == "templates" {
        let args: Vec<&str> = it.collect();
        let _ = commands::templates_cmd::handle_templates(app, &args);
        return;
    }

    if cmd == "registries" {
        let args: Vec<&str> = it.collect();
        let _ = commands::registry_cmd::handle_registries(app, &args);
        return;
    }

    if cmd == "template" || cmd == "tpl" {
        let args: Vec<&str> = it.collect();
        let _ = commands::templates_cmd::handle_template(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "registry" || cmd == "reg" {
        let args: Vec<&str> = it.collect();
        let _ = commands::registry_cmd::handle_registry(app, force, &args, action_req_tx);
        return;
    }

    if matches!(cmd, "nettemplate" | "nettpl" | "ntpl" | "nt") {
        let args: Vec<&str> = it.collect();
        let _ = commands::templates_cmd::handle_nettemplate(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "server" {
        let args: Vec<&str> = it.collect();
        let _ = commands::server_cmd::handle_server(
            app,
            force,
            cmdline_full.clone(),
            &args,
            conn_tx,
            refresh_tx,
            dash_refresh_tx,
        );
        return;
    }

    app.set_error(format!("unknown command: {cmd}"));
    return;
}

fn shell_exec_container_action(
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

fn shell_check_image_updates(
    app: &mut App,
    images: Vec<String>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let mut queued = 0usize;
    for image in images {
        let Some(normalized) = resolve_image_ref_for_updates(app, &image) else {
            app.log_msg(
                MsgLevel::Warn,
                format!("image update skipped (unresolved ref): {image}"),
            );
            continue;
        };
        let key = normalized;
        if app.image_updates_inflight.contains(&key) {
            continue;
        }
        app.note_rate_limit_request(&key);
        app.image_updates_inflight.insert(key.clone());
        let _ = action_req_tx.send(ActionRequest::ImageUpdateCheck {
            image: key.clone(),
            debug: app.image_update_debug,
        });
        app.log_msg(
            MsgLevel::Info,
            format!("image update queued: {key}"),
        );
        queued += 1;
    }
    if queued == 0 {
        app.set_warn("no images to check");
    } else {
        app.set_info(format!("checking {queued} image(s)"));
    }
    app.save_local_state();
}

fn shell_exec_stack_action(
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
            .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack_name.as_str()))
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

fn shell_exec_image_action(
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

fn shell_exec_volume_remove(app: &mut App, action_req_tx: &mpsc::UnboundedSender<ActionRequest>) {
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

fn shell_exec_network_remove(app: &mut App, action_req_tx: &mpsc::UnboundedSender<ActionRequest>) {
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
        app.set_warn("no networks selected (system networks cannot be modified)");
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

fn shell_registry_test_selected(
    app: &mut App,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let Some(entry) = app.registries_cfg.registries.get(app.registries_selected) else {
        app.set_warn("no registry selected");
        return;
    };
    let host = entry.host.clone();
    let test_repo = entry.test_repo.clone();
    let auth = match app.registry_auth_for_host(&host) {
        Ok(v) => v,
        Err(e) => {
            app.set_warn(format!("{e:#}"));
            return;
        }
    };
    app.set_info(format!("testing registry {host}"));
    let _ = action_req_tx.send(ActionRequest::RegistryTest {
        host,
        auth,
        test_repo,
    });
}

fn shell_execute_action(
    app: &mut App,
    a: ShellAction,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    match a {
        ShellAction::Start => {
            if app.shell_view == ShellView::Stacks {
                shell_exec_stack_action(app, ContainerAction::Start, None, action_req_tx);
            } else {
                shell_exec_container_action(app, ContainerAction::Start, action_req_tx);
            }
        }
        ShellAction::Stop => {
            if app.shell_view == ShellView::Stacks {
                shell_exec_stack_action(app, ContainerAction::Stop, None, action_req_tx);
            } else {
                shell_exec_container_action(app, ContainerAction::Stop, action_req_tx);
            }
        }
        ShellAction::Restart => {
            if app.shell_view == ShellView::Stacks {
                shell_exec_stack_action(app, ContainerAction::Restart, None, action_req_tx);
            } else {
                shell_exec_container_action(app, ContainerAction::Restart, action_req_tx);
            }
        }
        ShellAction::Delete => {
            if app.shell_view == ShellView::Stacks {
                let name = app.selected_stack_entry().map(|s| s.name.clone());
                if let Some(name) = name {
                    shell_begin_confirm(app, format!("stack rm {name}"), format!("stack rm {name}"));
                } else {
                    app.set_warn("no stack selected");
                }
            } else {
                shell_begin_confirm(app, "container rm", "container rm");
            }
        }
        ShellAction::Console => {
            shell_open_console(app, Some("root"), "bash");
        }
        ShellAction::ImageUntag => shell_begin_confirm(app, "image untag", "image untag"),
        ShellAction::ImageForceRemove => shell_begin_confirm(app, "image rm", "image rm"),
        ShellAction::VolumeRemove => shell_begin_confirm(app, "volume rm", "volume rm"),
        ShellAction::NetworkRemove => {
            let any_removable = if !app.marked_networks.is_empty() {
                app.marked_networks
                    .iter()
                    .any(|id| !app.is_system_network_id(id))
            } else {
                app.selected_network()
                    .map(|n| !App::is_system_network(n))
                    .unwrap_or(false)
            };
            if !any_removable {
                app.set_warn("system networks cannot be modified");
                return;
            }
            shell_begin_confirm(app, "network rm", "network rm");
        }
        ShellAction::TemplateEdit => shell_edit_selected_template(app),
        ShellAction::TemplateNew => {
            app.shell_cmdline.mode = true;
            set_text_and_cursor(
                &mut app.shell_cmdline.input,
                &mut app.shell_cmdline.cursor,
                match app.templates_state.kind {
                    TemplatesKind::Stacks => "template add ".to_string(),
                    TemplatesKind::Networks => "nettemplate add ".to_string(),
                },
            );
            app.shell_cmdline.confirm = None;
        }
        ShellAction::TemplateDelete => {
            let name = match app.templates_state.kind {
                TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
            };
            if let Some(name) = name {
                shell_begin_confirm(
                    app,
                    format!(
                        "{} rm {name}",
                        match app.templates_state.kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                    format!(
                        "{} rm {name}",
                        match app.templates_state.kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                );
            } else {
                app.set_warn("no template selected");
            }
        }
        ShellAction::TemplateDeploy => {
            match app.templates_state.kind {
                TemplatesKind::Stacks => {
                    if let Some(name) = app.selected_template().map(|t| t.name.clone()) {
                        shell_deploy_template(app, &name, false, false, action_req_tx);
                    } else {
                        app.set_warn("no template selected");
                    }
                }
                TemplatesKind::Networks => {
                    if let Some(name) = app.selected_net_template().map(|t| t.name.clone()) {
                        shell_deploy_net_template(app, &name, false, action_req_tx);
                    } else {
                        app.set_warn("no template selected");
                    }
                }
            }
        }
        ShellAction::RegistryTest => {
            shell_registry_test_selected(app, action_req_tx);
        }
    }
}

fn shell_deploy_template(
    app: &mut App,
    name: &str,
    pull: bool,
    force_recreate: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if app.templates_state.template_deploy_inflight.contains_key(name) {
        app.set_warn(format!("template '{name}' is already deploying"));
        return;
    }
    let Some(tpl) = app.templates_state.templates.iter().find(|t| t.name == name).cloned() else {
        app.set_warn(format!("unknown template: {name}"));
        return;
    };
    if !tpl.has_compose {
        app.set_warn("template has no compose.yaml");
        return;
    }
    let template_id = match ensure_template_id(&tpl.compose_path) {
        Ok(id) => id,
        Err(e) => {
            app.set_error(format!("template id create failed: {e:#}"));
            return;
        }
    };
    if tpl.template_id.as_deref() != Some(&template_id) {
        app.refresh_templates();
    }
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let server_name = app.active_server.clone().unwrap_or_default();
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let _ = action_req_tx.send(ActionRequest::TemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_compose: tpl.compose_path.clone(),
        pull,
        force_recreate,
        server_name,
        template_id,
    });
    app.templates_state.template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    let mut msg = format!("deploying template {name}");
    if force_recreate {
        msg.push_str(" (recreate)");
    }
    if pull {
        msg.push_str(" [pull]");
    }
    app.set_info(msg);
}

fn shell_deploy_net_template(
    app: &mut App,
    name: &str,
    force: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if app.templates_state.net_template_deploy_inflight.contains_key(name) {
        app.set_warn(format!("network template '{name}' is already deploying"));
        return;
    }
    let Some(tpl) = app.templates_state.net_templates.iter().find(|t| t.name == name).cloned() else {
        app.set_warn(format!("unknown network template: {name}"));
        return;
    };
    if !tpl.has_cfg {
        app.set_warn("template has no network.json");
        return;
    }
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let _ = action_req_tx.send(ActionRequest::NetTemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_cfg: tpl.cfg_path.clone(),
        force,
    });
    app.templates_state.net_template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.set_info(format!("deploying network template {name}"));
}

fn shell_edit_selected_template(app: &mut App) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => {
            let Some((name, has_compose, compose_path, dir)) = app.selected_template().map(|t| {
                (
                    t.name.clone(),
                    t.has_compose,
                    t.compose_path.clone(),
                    t.dir.clone(),
                )
            }) else {
                app.set_warn("no template selected");
                return;
            };
            app.templates_state.templates_refresh_after_edit = Some(name);
            let editor = app.editor_cmd();
            let target = if has_compose { compose_path } else { dir };
            let cmd = format!(
                "{} {}",
                editor,
                shell_escape_sh_arg(&target.to_string_lossy())
            );
            app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
        }
        TemplatesKind::Networks => shell_edit_selected_net_template(app),
    }
}

fn shell_edit_selected_net_template(app: &mut App) {
    let Some((name, has_cfg, cfg_path, dir)) = app.selected_net_template().map(|t| {
        (
            t.name.clone(),
            t.has_cfg,
            t.cfg_path.clone(),
            t.dir.clone(),
        )
    }) else {
        app.set_warn("no network template selected");
        return;
    };
    app.templates_state.net_templates_refresh_after_edit = Some(name);
    let editor = app.editor_cmd();
    let target = if has_cfg { cfg_path } else { dir };
    let cmd = format!(
        "{} {}",
        editor,
        shell_escape_sh_arg(&target.to_string_lossy())
    );
    app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
}

fn handle_shell_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    refresh_interval_tx: &watch::Sender<Duration>,
    refresh_pause_tx: &watch::Sender<bool>,
    image_update_limit_tx: &watch::Sender<usize>,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    // "always" bindings are evaluated before everything else (including input modes).
    if let Some(spec) = key_spec_from_event(key) {
        if let Some(hit) = lookup_binding(app, KeyScope::Always, spec) {
            match hit {
                BindingHit::Disabled => return,
                BindingHit::Cmd(cmd) => {
                    shell_execute_cmdline(
                        app,
                        &cmd,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        refresh_interval_tx,
                        refresh_pause_tx,
                        image_update_limit_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
            }
        }
    }

    if app.refresh_paused
        && key.modifiers.is_empty()
        && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
    {
        shell_refresh(app, refresh_tx, dash_refresh_tx, refresh_pause_tx);
        return;
    }

    if app.shell_cmdline.mode {
        if let Some(confirm) = app.shell_cmdline.confirm.clone() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Re-run the original command with the force modifier to auto-confirm.
                    let cmdline = format!("!{}", confirm.cmdline);
                    app.shell_cmdline.confirm = None;
                    app.shell_cmdline.mode = false;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.history.reset_nav();
                    shell_execute_cmdline(
                        app,
                        &cmdline,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        refresh_interval_tx,
                        refresh_pause_tx,
                        image_update_limit_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // Cancel.
                    app.shell_cmdline.confirm = None;
                    app.shell_cmdline.mode = false;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.history.reset_nav();
                    return;
                }
                _ => return,
            }
        }

        match key.code {
            KeyCode::Enter => {
                let cmdline = app.shell_cmdline.input.trim().to_string();
                app.shell_cmdline.mode = false;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.push_cmd_history(&cmdline);
                shell_execute_cmdline(
                    app,
                    &cmdline,
                    conn_tx,
                    refresh_tx,
                    dash_refresh_tx,
                    refresh_interval_tx,
                    refresh_pause_tx,
                    image_update_limit_tx,
                    logs_req_tx,
                    action_req_tx,
                );
            }
            KeyCode::Esc => {
                app.shell_cmdline.mode = false;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.shell_cmdline.confirm = None;
                app.shell_cmdline.history.reset_nav();
            }
            KeyCode::Up => {
                if let Some(s) = app.shell_cmdline.history.prev(&app.shell_cmdline.input) {
                    set_text_and_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, s);
                }
            }
            KeyCode::Down => {
                if let Some(s) = app.shell_cmdline.history.next() {
                    set_text_and_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, s);
                }
            }
            KeyCode::Backspace => {
                backspace_at_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor);
                app.shell_cmdline.history.on_edit();
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor);
                app.shell_cmdline.history.on_edit();
            }
            KeyCode::Left => {
                app.shell_cmdline.cursor = clamp_cursor_to_text(&app.shell_cmdline.input, app.shell_cmdline.cursor)
                    .saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.shell_cmdline.input.chars().count();
                app.shell_cmdline.cursor =
                    clamp_cursor_to_text(&app.shell_cmdline.input, app.shell_cmdline.cursor).saturating_add(1).min(len);
            }
            KeyCode::Home => app.shell_cmdline.cursor = 0,
            KeyCode::End => app.shell_cmdline.cursor = app.shell_cmdline.input.chars().count(),
            KeyCode::Char(ch) => {
                // Common readline-like movement shortcuts.
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match ch {
                        'a' | 'A' => app.shell_cmdline.cursor = 0,
                        'e' | 'E' => app.shell_cmdline.cursor = app.shell_cmdline.input.chars().count(),
                        'u' | 'U' => {
                            app.shell_cmdline.input.clear();
                            app.shell_cmdline.cursor = 0;
                            app.shell_cmdline.history.on_edit();
                        }
                        _ => {}
                    }
                } else if !ch.is_control() {
                    insert_char_at_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, ch);
                    app.shell_cmdline.history.on_edit();
                }
            }
            _ => {}
        }
        return;
    }

    // Input modes first (vim-like): when editing, do not treat keys as global shortcuts.
    if app.shell_view == ShellView::Logs {
        match app.logs.mode {
            LogsMode::Search => match key.code {
                KeyCode::Enter => app.logs_commit_search(),
                KeyCode::Esc => app.logs_cancel_search(),
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.logs.input, &mut app.logs.input_cursor);
                    app.logs_rebuild_matches();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.logs.input, &mut app.logs.input_cursor);
                    app.logs_rebuild_matches();
                }
                KeyCode::Left => {
                    app.logs.input_cursor =
                        clamp_cursor_to_text(&app.logs.input, app.logs.input_cursor).saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.logs.input.chars().count();
                    app.logs.input_cursor = clamp_cursor_to_text(&app.logs.input, app.logs.input_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.logs.input_cursor = 0,
                KeyCode::End => app.logs.input_cursor = app.logs.input.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        insert_char_at_cursor(&mut app.logs.input, &mut app.logs.input_cursor, ch);
                        app.logs_rebuild_matches();
                    }
                }
                _ => {}
            },
            LogsMode::Command => match key.code {
                KeyCode::Enter => {
                    // Minimal command mode for now.
                    let cmdline = app.logs.command.trim().to_string();
                    app.push_cmd_history(&cmdline);
                    let (force, path) = if let Some(rest) = cmdline.strip_prefix("save!") {
                        (true, rest.trim())
                    } else if let Some(rest) = cmdline.strip_prefix("save") {
                        (false, rest.trim())
                    } else {
                        (false, "")
                    };
                    if cmdline.starts_with("save") {
                        if path.is_empty() {
                            app.set_warn("usage: save <file>");
                        } else {
                            match app.logs.text.as_deref() {
                                None => app.set_warn("no logs loaded"),
                                Some(text) => match write_text_file(path, text, force) {
                                    Ok(p) => app.set_info(format!("saved logs to {}", p.display())),
                                    Err(e) => app.set_error(format!("save failed: {e:#}")),
                                },
                            }
                        }
                        app.logs.mode = LogsMode::Normal;
                        app.logs.command.clear();
                        app.logs.command_cursor = 0;
                        app.logs_rebuild_matches();
                        return;
                    }
                    let mut parts = cmdline.split_whitespace();
                    let cmd = parts.next().unwrap_or("");
                    match cmd {
                        "" => {}
                        "q" | "quit" => shell_back_from_full(app),
                        "j" => {
                            let Some(n) = parts.next() else {
                                app.set_warn("usage: j <line>");
                                // keep mode change below
                                app.logs.mode = LogsMode::Normal;
                                app.logs.command.clear();
                                app.logs.command_cursor = 0;
                                app.logs_rebuild_matches();
                                return;
                            };
                            match n.parse::<usize>() {
                                Ok(n) if n > 0 => {
                                    let total = app.logs_total_lines();
                                    app.logs.cursor =
                                        n.saturating_sub(1).min(total.saturating_sub(1));
                                }
                                _ => app.set_warn("usage: j <line>"),
                            }
                        }
                        "set" => match parts.next().unwrap_or("") {
                            "number" => app.logs.show_line_numbers = true,
                            "nonumber" => app.logs.show_line_numbers = false,
                            "logtail" => {
                                let Some(v) = parts.next() else {
                                    app.set_warn("usage: set logtail <lines>");
                                    app.logs.mode = LogsMode::Normal;
                                    app.logs.command.clear();
                                    app.logs.command_cursor = 0;
                                    app.logs_rebuild_matches();
                                    return;
                                };
                                match v.parse::<usize>() {
                                    Ok(n) if (1..=200_000).contains(&n) => {
                                        app.logs.tail = n;
                                        app.persist_config();
                                        if let Some(id) = app.logs.for_id.clone() {
                                            app.logs.loading = true;
                                            let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
                                        }
                                    }
                                    _ => app.set_warn("logtail must be 1..200000"),
                                }
                            }
                            "regex" => {
                                app.logs.use_regex = true;
                                app.logs_rebuild_matches();
                            }
                            "noregex" => {
                                app.logs.use_regex = false;
                                app.logs_rebuild_matches();
                            }
                            x => app.set_warn(format!("unknown option: {x}")),
                        },
                        _ => app.set_warn(format!("unknown command: {cmdline}")),
                    }
                    app.logs.mode = LogsMode::Normal;
                    app.logs.command.clear();
                    app.logs.command_cursor = 0;
                    app.logs_rebuild_matches();
                }
                KeyCode::Esc => {
                    app.logs.mode = LogsMode::Normal;
                    app.logs.command.clear();
                    app.logs.command_cursor = 0;
                    app.logs_rebuild_matches();
                    app.logs.cmd_history.reset_nav();
                }
                KeyCode::Up => {
                    if let Some(s) = app.logs.cmd_history.prev(&app.logs.command) {
                        set_text_and_cursor(&mut app.logs.command, &mut app.logs.command_cursor, s);
                    }
                }
                KeyCode::Down => {
                    if let Some(s) = app.logs.cmd_history.next() {
                        set_text_and_cursor(&mut app.logs.command, &mut app.logs.command_cursor, s);
                    }
                }
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.logs.command, &mut app.logs.command_cursor);
                    app.logs.cmd_history.on_edit();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.logs.command, &mut app.logs.command_cursor);
                    app.logs.cmd_history.on_edit();
                }
                KeyCode::Left => {
                    app.logs.command_cursor =
                        clamp_cursor_to_text(&app.logs.command, app.logs.command_cursor).saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.logs.command.chars().count();
                    app.logs.command_cursor = clamp_cursor_to_text(&app.logs.command, app.logs.command_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.logs.command_cursor = 0,
                KeyCode::End => app.logs.command_cursor = app.logs.command.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() {
                        insert_char_at_cursor(
                            &mut app.logs.command,
                            &mut app.logs.command_cursor,
                            ch,
                        );
                        app.logs.cmd_history.on_edit();
                    }
                }
                _ => {}
            },
            LogsMode::Normal => {}
        }
        if app.logs.mode != LogsMode::Normal {
            return;
        }
    }

    if app.shell_view == ShellView::Inspect {
        match app.inspect.mode {
            InspectMode::Search => match key.code {
                KeyCode::Enter => app.inspect_commit_search(),
                KeyCode::Esc => app.inspect_exit_input(),
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                    app.rebuild_inspect_lines();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                    app.rebuild_inspect_lines();
                }
                KeyCode::Left => {
                    app.inspect.input_cursor =
                        clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor).saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.inspect.input.chars().count();
                    app.inspect.input_cursor = clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.inspect.input_cursor = 0,
                KeyCode::End => app.inspect.input_cursor = app.inspect.input.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        insert_char_at_cursor(
                            &mut app.inspect.input,
                            &mut app.inspect.input_cursor,
                            ch,
                        );
                        app.rebuild_inspect_lines();
                    }
                }
                _ => {}
            },
            InspectMode::Command => match key.code {
                KeyCode::Enter => {
                    let cmd = app.inspect.input.trim().to_string();
                    app.push_cmd_history(&cmd);
                    let (force, path) = if let Some(rest) = cmd.strip_prefix("save!") {
                        (true, rest.trim())
                    } else if let Some(rest) = cmd.strip_prefix("save") {
                        (false, rest.trim())
                    } else {
                        (false, "")
                    };
                    if cmd.starts_with("save") {
                        if path.is_empty() {
                            app.inspect.error = Some("usage: save <file>".to_string());
                        } else {
                            match app.inspect.value.as_ref() {
                                None => {
                                    app.inspect.error = Some("no inspect data loaded".to_string())
                                }
                                Some(v) => match serde_json::to_string_pretty(v) {
                                    Ok(s) => match write_text_file(path, &s, force) {
                                        Ok(p) => app
                                            .set_info(format!("saved inspect to {}", p.display())),
                                        Err(e) => {
                                            app.inspect.error = Some(format!("save failed: {e:#}"))
                                        }
                                    },
                                    Err(e) => {
                                        app.inspect.error =
                                            Some(format!("failed to serialize inspect: {e:#}"))
                                    }
                                },
                            }
                        }
                        app.inspect.mode = InspectMode::Normal;
                        app.inspect.input.clear();
                        app.inspect.input_cursor = 0;
                        app.rebuild_inspect_lines();
                        return;
                    }
                    match cmd.as_str() {
                        "" => {}
                        "q" | "quit" => shell_back_from_full(app),
                        "e" | "expand" | "expandall" => app.inspect_expand_all(),
                        "c" | "collapse" | "collapseall" => app.inspect_collapse_all(),
                        "y" => app.inspect_copy_selected_value(true),
                        "p" => app.inspect_copy_selected_path(),
                        _ => app.inspect.error = Some(format!("unknown command: {cmd}")),
                    }
                    app.inspect.mode = InspectMode::Normal;
                    app.inspect.input.clear();
                    app.inspect.input_cursor = 0;
                    app.rebuild_inspect_lines();
                }
                KeyCode::Esc => {
                    app.inspect.mode = InspectMode::Normal;
                    app.inspect.input.clear();
                    app.inspect.input_cursor = 0;
                    app.rebuild_inspect_lines();
                    app.inspect.cmd_history.reset_nav();
                }
                KeyCode::Up => {
                    if let Some(s) = app.inspect.cmd_history.prev(&app.inspect.input) {
                        set_text_and_cursor(
                            &mut app.inspect.input,
                            &mut app.inspect.input_cursor,
                            s,
                        );
                    }
                }
                KeyCode::Down => {
                    if let Some(s) = app.inspect.cmd_history.next() {
                        set_text_and_cursor(
                            &mut app.inspect.input,
                            &mut app.inspect.input_cursor,
                            s,
                        );
                    }
                }
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                    app.inspect.cmd_history.on_edit();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                    app.inspect.cmd_history.on_edit();
                }
                KeyCode::Left => {
                    app.inspect.input_cursor = clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.inspect.input.chars().count();
                    app.inspect.input_cursor = clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.inspect.input_cursor = 0,
                KeyCode::End => app.inspect.input_cursor = app.inspect.input.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() {
                        insert_char_at_cursor(
                            &mut app.inspect.input,
                            &mut app.inspect.input_cursor,
                            ch,
                        );
                        app.inspect.cmd_history.on_edit();
                    }
                }
                _ => {}
            },
            InspectMode::Normal => {}
        }
        if app.inspect.mode != InspectMode::Normal {
            return;
        }
    }

    // Custom key bindings (outside of input modes). Skip single-letter shortcuts when sidebar has focus.
    if let Some(spec) = key_spec_from_event(key) {
        if app.shell_focus != ShellFocus::Sidebar || !is_single_letter_without_modifiers(spec) {
            if let Some(hit) = lookup_scoped_binding(app, spec) {
                match hit {
                    BindingHit::Disabled => return,
                    BindingHit::Cmd(cmd) => {
                        shell_execute_cmdline(
                            app,
                            &cmd,
                            conn_tx,
                            refresh_tx,
                            dash_refresh_tx,
                            refresh_interval_tx,
                            refresh_pause_tx,
                            image_update_limit_tx,
                            logs_req_tx,
                            action_req_tx,
                        );
                        return;
                    }
                }
            }
        }
    }

    // Global keys.
    match key.code {
        KeyCode::Tab => {
            shell_cycle_focus(app);
            return;
        }
        KeyCode::Char(':') if key.modifiers.is_empty() => {
            // In Logs/Inspect, ':' is view-local command mode (vim-like).
            match app.shell_view {
                ShellView::Logs => {
                    app.logs.mode = LogsMode::Command;
                    app.logs.command.clear();
                    app.logs.command_cursor = 0;
                    app.logs_rebuild_matches();
                }
                ShellView::Inspect => app.inspect_enter_command(),
                _ => {
                    app.shell_cmdline.mode = true;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.confirm = None;
                }
            }
            return;
        }
        KeyCode::Char('q') if key.modifiers.is_empty() => {
            shell_back_from_full(app);
            return;
        }
        _ => {}
    }

    // Direct shortcuts (servers/modules/actions).
    if key.modifiers.is_empty() {
        if let KeyCode::Char(mut ch) = key.code {
            // Servers: 1..9 and assigned letters.
            for (i, hint) in app.shell_server_shortcuts.iter().copied().enumerate() {
                if hint == '\0' {
                    continue;
                }
                if hint.is_ascii_alphabetic() {
                    ch = ch.to_ascii_uppercase();
                }
                if ch == hint {
                    shell_switch_server(app, i, conn_tx, refresh_tx, dash_refresh_tx);
                    return;
                }
            }
            // Modules (disabled in full-screen views like Logs/Inspect to avoid conflicts with
            // in-view navigation keys like n/N, j/k, etc.).
            if !matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
                let ch_lc = ch.to_ascii_lowercase();
                for v in [
                    ShellView::Dashboard,
                    ShellView::Stacks,
                    ShellView::Containers,
                    ShellView::Images,
                    ShellView::Volumes,
                    ShellView::Networks,
                    ShellView::Templates,
                    ShellView::Registries,
                    ShellView::Inspect,
                    ShellView::Logs,
                ] {
                    if ch_lc == shell_module_shortcut(v) {
                        match v {
                            ShellView::Inspect => shell_enter_inspect(app, inspect_req_tx),
                            ShellView::Logs => shell_enter_logs(app, logs_req_tx),
                            _ => {
                                shell_set_main_view(app, v);
                                shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                            }
                        }
                        return;
                    }
                }
            }
        }
    }

    // Focus-specific navigation / activation.
    if app.shell_focus == ShellFocus::Sidebar {
        match key.code {
            KeyCode::Up => shell_move_sidebar(app, -1),
            KeyCode::Down => shell_move_sidebar(app, 1),
            KeyCode::Enter => {
                let items = shell_sidebar_items(app);
                let Some(it) = items.get(app.shell_sidebar_selected).copied() else {
                    return;
                };
                match it {
                    ShellSidebarItem::Server(i) => {
                        shell_switch_server(app, i, conn_tx, refresh_tx, dash_refresh_tx)
                    }
                    ShellSidebarItem::Module(v) => match v {
                        ShellView::Inspect => shell_enter_inspect(app, inspect_req_tx),
                        ShellView::Logs => shell_enter_logs(app, logs_req_tx),
                        _ => {
                            shell_set_main_view(app, v);
                            shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                        }
                    },
                    ShellSidebarItem::Action(a) => shell_execute_action(app, a, action_req_tx),
                    ShellSidebarItem::Separator => {}
                    ShellSidebarItem::Gap => {}
                }
            }
            _ => {}
        }
        return;
    }

    // Main list / view handling.
    match app.shell_view {
        ShellView::Dashboard => {}
        ShellView::Stacks
        | ShellView::Containers
        | ShellView::Images
        | ShellView::Volumes
        | ShellView::Networks => {
            if app.shell_focus == ShellFocus::Details {
                let stack_name = if app.shell_view == ShellView::Stacks {
                    let name = app.selected_stack_entry().map(|s| s.name.clone());
                    if let Some(ref n) = name {
                        if app.stack_network_count(n) == 0 {
                            app.stack_details_focus = StackDetailsFocus::Containers;
                        }
                    }
                    name
                } else {
                    None
                };
                let stack_counts = if let (ShellView::Stacks, Some(ref name)) =
                    (app.shell_view, stack_name.as_ref())
                {
                    let containers = app.stack_container_count(name);
                    let networks = app.stack_network_count(name);
                    Some((containers, networks))
                } else {
                    None
                };
                let scroll = match app.shell_view {
                    ShellView::Stacks => match app.stack_details_focus {
                        StackDetailsFocus::Containers => &mut app.stacks_details_scroll,
                        StackDetailsFocus::Networks => &mut app.stacks_networks_scroll,
                    },
                    ShellView::Containers => &mut app.container_details_scroll,
                    ShellView::Images => &mut app.image_details_scroll,
                    ShellView::Volumes => &mut app.volume_details_scroll,
                    ShellView::Networks => &mut app.network_details_scroll,
                    _ => &mut app.container_details_scroll,
                };
                match key.code {
                    KeyCode::Left | KeyCode::Right => {
                        if app.shell_view == ShellView::Stacks {
                            if let Some((_, networks)) = stack_counts {
                                if networks > 0 {
                                    app.stack_details_focus = match app.stack_details_focus {
                                        StackDetailsFocus::Containers => {
                                            StackDetailsFocus::Networks
                                        }
                                        StackDetailsFocus::Networks => {
                                            StackDetailsFocus::Containers
                                        }
                                    };
                                    return;
                                }
                            }
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        *scroll = scroll.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        *scroll = scroll.saturating_add(1);
                    }
                    KeyCode::PageUp => {
                        *scroll = scroll.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        *scroll = scroll.saturating_add(10);
                    }
                    KeyCode::Home => *scroll = 0,
                    KeyCode::End => {
                        if app.shell_view == ShellView::Stacks {
                            if let Some((containers, networks)) = stack_counts {
                                let count = match app.stack_details_focus {
                                    StackDetailsFocus::Containers => containers,
                                    StackDetailsFocus::Networks => networks,
                                };
                                *scroll = count.saturating_sub(1);
                            } else {
                                *scroll = 0;
                            }
                        } else {
                            *scroll = usize::MAX;
                        }
                    }
                    _ => {}
                }
                return;
            }
            // Ensure active_view matches (used by the existing selection/mark logic).
            app.active_view = match app.shell_view {
                ShellView::Stacks => ActiveView::Stacks,
                ShellView::Containers => ActiveView::Containers,
                ShellView::Images => ActiveView::Images,
                ShellView::Volumes => ActiveView::Volumes,
                ShellView::Networks => ActiveView::Networks,
                _ => app.active_view,
            };

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        app.move_up();
                    }
                }
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        app.move_down();
                    }
                }
                KeyCode::Home => match app.active_view {
                    ActiveView::Stacks => app.stacks_selected = 0,
                    ActiveView::Containers => app.selected = 0,
                    ActiveView::Images => app.images_selected = 0,
                    ActiveView::Volumes => app.volumes_selected = 0,
                    ActiveView::Networks => app.networks_selected = 0,
                },
                KeyCode::End => match app.active_view {
                    ActiveView::Stacks => {
                        app.stacks_selected = app.stacks.len().saturating_sub(1);
                    }
                    ActiveView::Containers => {
                        let max = app.view_len().saturating_sub(1);
                        app.selected = max;
                    }
                    ActiveView::Images => {
                        app.images_selected = app.images_visible_len().saturating_sub(1);
                    }
                    ActiveView::Volumes => {
                        app.volumes_selected = app.volumes_visible_len().saturating_sub(1);
                    }
                    ActiveView::Networks => {
                        app.networks_selected = app.networks.len().saturating_sub(1)
                    }
                },
                KeyCode::Char(' ') => {
                    if app.active_view == ActiveView::Containers
                        && app.list_mode == ListMode::Tree
                        && app.toggle_tree_expanded_selected()
                    {
                        // Stack header toggle.
                    } else {
                        app.toggle_mark_selected();
                    }
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.mark_all();
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.clear_marks();
                }
                KeyCode::Enter => {
                    if app.active_view == ActiveView::Containers
                        && app.list_mode == ListMode::Tree
                        && app.toggle_tree_expanded_selected()
                    {
                        // Stack header toggle.
                    }
                }
                _ => {}
            }
        }
        ShellView::Templates => {
            if app.shell_focus == ShellFocus::Details {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.templates_state.templates_details_scroll =
                                app.templates_state.templates_details_scroll.saturating_sub(1)
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.templates_state.templates_details_scroll += 1,
                        KeyCode::PageUp => {
                            app.templates_state.templates_details_scroll =
                                app.templates_state.templates_details_scroll.saturating_sub(10)
                        }
                        KeyCode::PageDown => app.templates_state.templates_details_scroll += 10,
                        KeyCode::Home => app.templates_state.templates_details_scroll = 0,
                        KeyCode::End => app.templates_state.templates_details_scroll = usize::MAX,
                        _ => {}
                    },
                    TemplatesKind::Networks => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.templates_state.net_templates_details_scroll =
                                app.templates_state.net_templates_details_scroll.saturating_sub(1)
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.templates_state.net_templates_details_scroll += 1,
                        KeyCode::PageUp => {
                            app.templates_state.net_templates_details_scroll =
                                app.templates_state.net_templates_details_scroll.saturating_sub(10)
                        }
                        KeyCode::PageDown => app.templates_state.net_templates_details_scroll += 10,
                        KeyCode::Home => app.templates_state.net_templates_details_scroll = 0,
                        KeyCode::End => app.templates_state.net_templates_details_scroll = usize::MAX,
                        _ => {}
                    },
                }
            } else {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => {
                        let before = app.templates_state.templates_selected;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.templates_state.templates_selected = app.templates_state.templates_selected.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !app.templates_state.templates.is_empty() {
                                    app.templates_state.templates_selected =
                                        (app.templates_state.templates_selected + 1).min(app.templates_state.templates.len() - 1);
                                } else {
                                    app.templates_state.templates_selected = 0;
                                }
                            }
                            KeyCode::PageUp => {
                                app.templates_state.templates_selected = app.templates_state.templates_selected.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                if !app.templates_state.templates.is_empty() {
                                    app.templates_state.templates_selected =
                                        (app.templates_state.templates_selected + 10).min(app.templates_state.templates.len() - 1);
                                } else {
                                    app.templates_state.templates_selected = 0;
                                }
                            }
                            KeyCode::Home => app.templates_state.templates_selected = 0,
                            KeyCode::End => {
                                app.templates_state.templates_selected = app.templates_state.templates.len().saturating_sub(1)
                            }
                            _ => {}
                        }
                        if app.templates_state.templates_selected != before {
                            app.templates_state.templates_details_scroll = 0;
                        }
                    }
                    TemplatesKind::Networks => {
                        let before = app.templates_state.net_templates_selected;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.templates_state.net_templates_selected =
                                    app.templates_state.net_templates_selected.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !app.templates_state.net_templates.is_empty() {
                                    app.templates_state.net_templates_selected = (app.templates_state.net_templates_selected + 1)
                                        .min(app.templates_state.net_templates.len() - 1);
                                } else {
                                    app.templates_state.net_templates_selected = 0;
                                }
                            }
                            KeyCode::PageUp => {
                                app.templates_state.net_templates_selected =
                                    app.templates_state.net_templates_selected.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                if !app.templates_state.net_templates.is_empty() {
                                    app.templates_state.net_templates_selected = (app.templates_state.net_templates_selected + 10)
                                        .min(app.templates_state.net_templates.len() - 1);
                                } else {
                                    app.templates_state.net_templates_selected = 0;
                                }
                            }
                            KeyCode::Home => app.templates_state.net_templates_selected = 0,
                            KeyCode::End => {
                                app.templates_state.net_templates_selected =
                                    app.templates_state.net_templates.len().saturating_sub(1)
                            }
                            _ => {}
                        }
                        if app.templates_state.net_templates_selected != before {
                            app.templates_state.net_templates_details_scroll = 0;
                        }
                    }
                }
            }
        }
        ShellView::Registries => {
            if app.shell_focus == ShellFocus::Details {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.registries_details_scroll =
                            app.registries_details_scroll.saturating_sub(1)
                    }
                    KeyCode::Down | KeyCode::Char('j') => app.registries_details_scroll += 1,
                    KeyCode::PageUp => {
                        app.registries_details_scroll =
                            app.registries_details_scroll.saturating_sub(10)
                    }
                    KeyCode::PageDown => app.registries_details_scroll += 10,
                    KeyCode::Home => app.registries_details_scroll = 0,
                    KeyCode::End => app.registries_details_scroll = usize::MAX,
                    _ => {}
                }
            } else {
                let before = app.registries_selected;
                let total = app.registries_cfg.registries.len();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.registries_selected = app.registries_selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if total > 0 {
                            app.registries_selected =
                                (app.registries_selected + 1).min(total - 1);
                        } else {
                            app.registries_selected = 0;
                        }
                    }
                    KeyCode::PageUp => {
                        app.registries_selected = app.registries_selected.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        if total > 0 {
                            app.registries_selected =
                                (app.registries_selected + 10).min(total - 1);
                        } else {
                            app.registries_selected = 0;
                        }
                    }
                    KeyCode::Home => app.registries_selected = 0,
                    KeyCode::End => {
                        app.registries_selected = total.saturating_sub(1);
                    }
                    _ => {}
                }
                if app.registries_selected != before {
                    app.registries_details_scroll = 0;
                }
            }
        }
        ShellView::Logs => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.logs_move_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.logs_move_down(1),
            KeyCode::PageUp => app.logs_move_up(10),
            KeyCode::PageDown => app.logs_move_down(10),
            KeyCode::Left => app.logs.hscroll = app.logs.hscroll.saturating_sub(4),
            KeyCode::Right => app.logs.hscroll = app.logs.hscroll.saturating_add(4),
            KeyCode::Home => app.logs.cursor = 0,
            KeyCode::End => app.logs.cursor = app.logs_total_lines().saturating_sub(1),
            KeyCode::Esc => {
                if app.logs.select_anchor.is_some() {
                    app.logs_clear_selection();
                }
            }
            KeyCode::Char(' ') => app.logs_toggle_selection(),
            KeyCode::Char('m') => {
                app.logs.use_regex = !app.logs.use_regex;
                app.logs_rebuild_matches();
            }
            KeyCode::Char('l') => app.logs.show_line_numbers = !app.logs.show_line_numbers,
            KeyCode::Char('/') => {
                app.logs.mode = LogsMode::Search;
                app.logs.input = app.logs.query.clone();
                app.logs.input_cursor = app.logs.input.chars().count();
                app.logs_rebuild_matches();
            }
            KeyCode::Char(':') => {
                app.logs.mode = LogsMode::Command;
                app.logs.command.clear();
                app.logs.command_cursor = 0;
                app.logs_rebuild_matches();
            }
            KeyCode::Char('n') => app.logs_next_match(),
            KeyCode::Char('N') => app.logs_prev_match(),
            _ => {}
        },
        ShellView::Inspect => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.inspect_move_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.inspect_move_down(1),
            KeyCode::PageUp => app.inspect_move_up(10),
            KeyCode::PageDown => app.inspect_move_down(10),
            KeyCode::Left => app.inspect.scroll = app.inspect.scroll.saturating_sub(4),
            KeyCode::Right => app.inspect.scroll = app.inspect.scroll.saturating_add(4),
            KeyCode::Home => {
                app.inspect.selected = 0;
                app.inspect.scroll = 0;
            }
            KeyCode::End => {
                if !app.inspect.lines.is_empty() {
                    app.inspect.selected = app.inspect.lines.len() - 1;
                } else {
                    app.inspect.selected = 0;
                }
            }
            KeyCode::Enter => app.inspect_toggle_selected(),
            KeyCode::Char('/') => app.inspect_enter_search(),
            KeyCode::Char(':') => app.inspect_enter_command(),
            KeyCode::Char('n') => app.inspect_jump_next_match(),
            KeyCode::Char('N') => app.inspect_jump_prev_match(),
            _ => {}
        },
        ShellView::Help => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.shell_help.scroll = app.shell_help.scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.shell_help.scroll = app.shell_help.scroll.saturating_add(1)
            }
            KeyCode::PageUp => app.shell_help.scroll = app.shell_help.scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_help.scroll = app.shell_help.scroll.saturating_add(10),
            KeyCode::Home => app.shell_help.scroll = 0,
            KeyCode::End => app.shell_help.scroll = usize::MAX,
            _ => {}
        },
        ShellView::Messages => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(1)
            }
            KeyCode::PageUp => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(10),
            KeyCode::Left => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_sub(4),
            KeyCode::Right => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_add(4),
            KeyCode::Home => app.shell_msgs.scroll = 0,
            KeyCode::End => app.shell_msgs.scroll = usize::MAX,
            _ => {}
        },
    }
}

fn draw_shell_header(
    f: &mut ratatui::Frame,
    app: &App,
    _refresh: Duration,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.header.to_style();
    f.render_widget(Block::default().style(bg), area);

    let server = current_server_label(app);
    let crumb = shell_breadcrumbs(app);
    let conn = if app.conn_error.is_some() {
        "○"
    } else {
        "●"
    };
    let conn_style = if app.conn_error.is_some() {
        app.theme
            .text_error
            .to_style()
            .bg(theme::parse_color(&app.theme.header.bg))
    } else {
        app.theme
            .text_ok
            .to_style()
            .bg(theme::parse_color(&app.theme.header.bg))
    };

    let left = " CONTAINR  ";
    let unseen_errors = app.unseen_error_count();
    let err_badge = if unseen_errors > 0 {
        format!("  !{unseen_errors}")
    } else {
        String::new()
    };
    let deploy = if let Some((name, marker)) = app.templates_state.template_deploy_inflight.iter().next() {
        let secs = marker.started.elapsed().as_secs();
        let spin = spinner_char(marker.started, app.ascii_only);
        format!("  Deploy: {name} {spin} {secs}s")
    } else {
        String::new()
    };
    let commit_label = if commands::git_cmd::git_available() && app.git_autocommit {
        "  Commit: auto"
    } else {
        ""
    };
    let mid = format!(
        "Server: {server}  {conn} connected{err_badge}  ⟳ {}s{commit_label}  View: {}{crumb}{deploy}",
        app.refresh_secs.max(1),
        app.shell_view.title(),
    );
    let right = "";

    let w = area.width.max(1) as usize;
    let mut line = String::new();
    line.push_str(left);
    line.push_str(&mid);
    let min_right = right.chars().count();
    let shown = truncate_end(&line, w.saturating_sub(min_right));
    let rem = w.saturating_sub(shown.chars().count());
    let right_shown = truncate_start(right, rem);

    let mut spans: Vec<Span> = Vec::new();
    let (logo, rest) = split_at_chars(&shown, left.chars().count());
    spans.extend(header_logo_spans(app, bg, logo));
    // Bolden breadcrumb for better scanability.
    if !crumb.is_empty() && rest.contains(&crumb) {
        let mut parts = rest.splitn(2, &crumb);
        let before = parts.next().unwrap_or_default();
        let after = parts.next().unwrap_or_default();
        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), bg));
        }
        spans.push(Span::styled(crumb.clone(), bg.add_modifier(Modifier::BOLD)));
        if !after.is_empty() {
            spans.push(Span::styled(after.to_string(), bg));
        }
    } else {
        spans.push(Span::styled(rest.to_string(), bg));
    }
    // Color the connection dot to reflect current status.
    if spans.len() == 1 && spans[0].content.contains(conn) {
        // Not expected with current layout, but keep safe.
    }
    if spans
        .iter()
        .map(|s| s.content.clone())
        .collect::<String>()
        .contains(conn)
    {
        // If the conn symbol is inside existing spans, split the last span that contains it.
        let mut updated: Vec<Span> = Vec::new();
        for s in spans.into_iter() {
            if s.content.contains(conn) {
                let parts: Vec<&str> = s.content.split(conn).collect();
                if parts.len() == 2 {
                    updated.push(Span::styled(parts[0].to_string(), s.style));
                    updated.push(Span::styled(conn.to_string(), conn_style));
                    updated.push(Span::styled(parts[1].to_string(), s.style));
                } else {
                    updated.push(s);
                }
            } else {
                updated.push(s);
            }
        }
        spans = updated;
    }
    // Color the error badge.
    if unseen_errors > 0 {
        let badge = format!("!{unseen_errors}");
        let mut updated: Vec<Span> = Vec::new();
        for s in spans.into_iter() {
            if s.content.contains(&badge) {
                let parts: Vec<&str> = s.content.split(&badge).collect();
                if parts.len() == 2 {
                    updated.push(Span::styled(parts[0].to_string(), s.style));
                    let badge_style = app
                        .theme
                        .text_error
                        .to_style()
                        .bg(theme::parse_color(&app.theme.header.bg))
                        .add_modifier(Modifier::BOLD);
                    updated.push(Span::styled(badge.clone(), badge_style));
                    updated.push(Span::styled(parts[1].to_string(), s.style));
                } else {
                    updated.push(s);
                }
            } else {
                updated.push(s);
            }
        }
        spans = updated;
    }
    if !right_shown.is_empty() {
        spans.push(Span::styled(right_shown, bg.fg(Color::Gray)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn action_error_label(err: &LastActionError) -> &'static str {
    match err.kind {
        ActionErrorKind::InUse => "in use",
        ActionErrorKind::Other => "error",
    }
}

fn action_error_details(err: &LastActionError) -> String {
    let ts = format_action_ts(err.at);
    if err.action.trim().is_empty() {
        ts
    } else {
        format!("{} {}", err.action, ts)
    }
}

fn split_at_chars(s: &str, n: usize) -> (&str, &str) {
    if n == 0 {
        return ("", s);
    }
    let mut idx = 0usize;
    let mut chars = 0usize;
    for (i, _) in s.char_indices() {
        if chars == n {
            idx = i;
            break;
        }
        chars += 1;
        idx = s.len();
    }
    if chars < n {
        (s, "")
    } else {
        s.split_at(idx)
    }
}

fn header_logo_spans(app: &App, base: Style, shown: &str) -> Vec<Span<'static>> {
    // Render the "CONTAINR" logo in per-run colors without changing background.
    let bg = theme::parse_color(&app.theme.header.bg);
    let bg_rgb = match bg {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    };
    let is_dark = bg_rgb.map(|(r, g, b)| rel_luma(r, g, b) < 0.55).unwrap_or(true);

    let bright_palette: [Color; 8] = [
        Color::Rgb(255, 95, 86),  // red
        Color::Rgb(255, 189, 46), // yellow
        Color::Rgb(39, 201, 63),  // green
        Color::Rgb(64, 156, 255), // blue
        Color::Rgb(175, 82, 222), // purple
        Color::Rgb(255, 105, 180), // pink
        Color::Rgb(0, 212, 212),  // cyan
        Color::Rgb(255, 255, 255), // white
    ];
    let dark_palette: [Color; 8] = [
        Color::Rgb(120, 20, 20),
        Color::Rgb(120, 80, 0),
        Color::Rgb(0, 90, 40),
        Color::Rgb(0, 60, 120),
        Color::Rgb(70, 30, 110),
        Color::Rgb(120, 30, 70),
        Color::Rgb(0, 90, 90),
        Color::Rgb(0, 0, 0),
    ];
    let palette: &[Color] = if is_dark { &bright_palette } else { &dark_palette };

    let seed = app.header_logo_seed;
    let offset = (seed as usize) % palette.len();
    // Ensure we don't fall into short cycles (e.g. len=8, step=2 repeats every 4).
    let mut step = (((seed >> 8) as usize) % (palette.len().saturating_sub(1)).max(1)).max(1);
    step = coprime_step(step, palette.len());

    let mut out: Vec<Span<'static>> = Vec::new();
    let mut letter_i = 0usize;
    for ch in shown.chars() {
        if ch.is_ascii_alphabetic() {
            let mut c = palette[(offset + letter_i.saturating_mul(step)) % palette.len()];
            if let Some((br, bg, bb)) = bg_rgb {
                let ratio = contrast_ratio((br, bg, bb), c);
                if ratio < 3.0 {
                    c = if is_dark { Color::White } else { Color::Black };
                }
            }
            out.push(Span::styled(
                ch.to_string(),
                base.fg(c).add_modifier(Modifier::BOLD),
            ));
            letter_i = letter_i.saturating_add(1);
        } else {
            out.push(Span::styled(ch.to_string(), base));
        }
    }
    out
}

fn coprime_step(mut step: usize, len: usize) -> usize {
    if len <= 1 {
        return 1;
    }
    step = step.clamp(1, len - 1);
    while gcd(step, len) != 1 {
        step += 1;
        if step >= len {
            step = 1;
        }
    }
    step
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn rel_luma(r: u8, g: u8, b: u8) -> f32 {
    fn to_lin(u: u8) -> f32 {
        let c = (u as f32) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * to_lin(r) + 0.7152 * to_lin(g) + 0.0722 * to_lin(b)
}

fn contrast_ratio(bg: (u8, u8, u8), fg: Color) -> f32 {
    let fg = match fg {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        _ => (255, 255, 255),
    };
    let l1 = rel_luma(bg.0, bg.1, bg.2);
    let l2 = rel_luma(fg.0, fg.1, fg.2);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

fn shell_breadcrumbs(app: &App) -> String {
    match app.shell_view {
        ShellView::Dashboard => String::new(),
        ShellView::Stacks => app
            .selected_stack_entry()
            .map(|s| format!("/{}", s.name))
            .unwrap_or_default(),
        ShellView::Containers => {
            if let Some((name, ..)) = app.selected_stack() {
                return format!("/{name}");
            }
            if let Some(c) = app.selected_container() {
                if let Some(stack) = stack_name_from_labels(&c.labels) {
                    format!("/{stack}/{}", c.name)
                } else {
                    format!("/{}", c.name)
                }
            } else {
                String::new()
            }
        }
        ShellView::Images => app
            .selected_image()
            .map(|i| format!("/{}", i.name()))
            .unwrap_or_default(),
        ShellView::Volumes => app
            .selected_volume()
            .map(|v| format!("/{}", v.name))
            .unwrap_or_default(),
        ShellView::Networks => app
            .selected_network()
            .map(|n| format!("/{}", n.name))
            .unwrap_or_default(),
        ShellView::Templates => match app.templates_state.kind {
            TemplatesKind::Stacks => app
                .selected_template()
                .map(|t| format!("/{}", t.name))
                .unwrap_or_default(),
            TemplatesKind::Networks => app
                .selected_net_template()
                .map(|t| format!("/{}", t.name))
                .unwrap_or_default(),
        },
        ShellView::Registries => app
            .registries_cfg
            .registries
            .get(app.registries_selected)
            .map(|r| format!("/{}", r.host))
            .unwrap_or_default(),
        ShellView::Inspect => app
            .inspect.target
            .as_ref()
            .map(|t| format!("/{}", t.label))
            .unwrap_or_default(),
        ShellView::Logs => app
            .logs.for_id
            .as_ref()
            .and_then(|_| app.selected_container().map(|c| c.name.clone()))
            .map(|n| format!("/{n}"))
            .unwrap_or_default(),
        ShellView::Help => String::new(),
        ShellView::Messages => String::new(),
    }
}

fn draw_shell_body(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    if app.shell_sidebar_hidden {
        draw_shell_main(f, app, area);
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if app.shell_sidebar_collapsed { 18 } else { 28 }),
            Constraint::Min(1),
        ])
        .split(area);
    draw_shell_sidebar(f, app, cols[0]);
    draw_shell_main(f, app, cols[1]);
}

fn draw_shell_sidebar(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Sidebar {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner_area = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let inner_w = inner_area.width.max(1) as usize;

    let items = shell_sidebar_items(app);
    let mut rendered: Vec<ListItem> = Vec::new();
    for (idx, it) in items.iter().enumerate() {
        let selected = app.shell_focus == ShellFocus::Sidebar && idx == app.shell_sidebar_selected;
        let st = if selected {
            shell_row_highlight(app)
        } else {
            bg
        };

        match *it {
            ShellSidebarItem::Separator => {
                let base_bg = if app.shell_focus == ShellFocus::Sidebar {
                    theme::parse_color(&app.theme.panel_focused.bg)
                } else {
                    theme::parse_color(&app.theme.panel.bg)
                };
                let divider_style = app.theme.divider.to_style().bg(base_bg);
                rendered.push(ListItem::new(Line::from(Span::styled(
                    "─".repeat(inner_w),
                    divider_style,
                ))));
            }
            ShellSidebarItem::Gap => {
                rendered.push(ListItem::new(Line::from(Span::styled(" ".to_string(), bg))));
            }
            ShellSidebarItem::Server(i) => {
                let name = app.servers.get(i).map(|s| s.name.as_str()).unwrap_or("?");
                let base = format!(" {name}");
                let active_style = app.theme.active.to_style();
                if app.shell_sidebar_collapsed {
                    let st = if !selected && i == app.server_selected {
                        active_style
                    } else {
                        st
                    };
                    rendered.push(ListItem::new(Line::from(Span::styled(base, st))));
                } else {
                    let hint = app.shell_server_shortcuts.get(i).copied().unwrap_or('?');
                    let hint = format!("[{hint}]");
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let base_style = if !selected && i == app.server_selected {
                        active_style
                    } else {
                        st
                    };
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(Color::White)
                    } else {
                        bg.fg(theme::parse_color(&app.theme.text_dim.fg))
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
            ShellSidebarItem::Module(v) => {
                let name = v.title();
                let base = format!(" {name}");
                let active_style = app.theme.active.to_style();
                if app.shell_sidebar_collapsed {
                    let base_style = if !selected && v == app.shell_view {
                        active_style
                    } else {
                        st
                    };
                    rendered.push(ListItem::new(Line::from(Span::styled(base, base_style))));
                } else {
                    let hint = shell_module_shortcut(v);
                    let hint = format!("[{hint}]");
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let base_style = if !selected && v == app.shell_view {
                        active_style
                    } else {
                        st
                    };
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(theme::parse_color(&app.theme.panel.fg))
                    } else {
                        bg.patch(app.theme.text_dim.to_style())
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
            ShellSidebarItem::Action(a) => {
                let label = a.label();
                let base = format!(" {label}");
                let base_style = if selected {
                    shell_row_highlight(app)
                } else {
                    bg.patch(app.theme.text.to_style())
                };
                if app.shell_sidebar_collapsed {
                    rendered.push(ListItem::new(Line::from(Span::styled(base, base_style))));
                } else {
                    // Show action chords as Ctrl-based hints.
                    let hint = format!("[{}]", a.ctrl_hint());
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(theme::parse_color(&app.theme.panel.fg))
                    } else {
                        bg.patch(app.theme.text_dim.to_style())
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
        }
    }
    if rendered.is_empty() {
        rendered.push(ListItem::new(Line::from("")));
    }
    let mut state = ListState::default();
    state.select(Some(
        app.shell_sidebar_selected
            .min(rendered.len().saturating_sub(1)),
    ));
    let list = List::new(rendered).highlight_symbol("").style(bg);
    f.render_stateful_widget(list, inner_area, &mut state);
}

fn draw_shell_main(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    // Dashboard is a single-pane view (no details section).
    if app.shell_view == ShellView::Dashboard {
        draw_shell_main_list(f, app, area);
        return;
    }

    let is_full = matches!(app.shell_view, ShellView::Logs | ShellView::Inspect);
    let is_split_view = matches!(
        app.shell_view,
        ShellView::Stacks
            | ShellView::Containers
            | ShellView::Images
            | ShellView::Volumes
            | ShellView::Networks
            | ShellView::Templates
            | ShellView::Registries
    );

    if is_split_view && app.shell_split_mode == ShellSplitMode::Vertical {
        let parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Length(1),
                Constraint::Percentage(50),
            ])
            .split(area);
        draw_shell_main_list(f, app, parts[0]);
        draw_shell_vr(f, app, parts[1]);
        draw_shell_main_details(f, app, parts[2]);
        return;
    }

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            if matches!(
                app.shell_view,
                ShellView::Logs | ShellView::Inspect | ShellView::Messages | ShellView::Help
            ) {
                // Keep the meta area compact (3 lines) and centered.
                [
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                ]
            } else if is_full {
                [
                    Constraint::Percentage(85),
                    Constraint::Length(1),
                    Constraint::Percentage(15),
                ]
            } else {
                [
                    Constraint::Percentage(62),
                    Constraint::Length(1),
                    Constraint::Percentage(38),
                ]
            },
        )
        .split(area);

    draw_shell_main_list(f, app, parts[0]);
    draw_shell_hr(f, app, parts[1]);
    draw_shell_main_details(f, app, parts[2]);
}

fn draw_shell_hr(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let st = app.theme.divider.to_style();
    let line = "─".repeat(area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(line).style(st).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_vr(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let st = app.theme.divider.to_style();
    let line = "│".repeat(area.height.max(1) as usize);
    f.render_widget(
        Paragraph::new(line).style(st).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_title(
    f: &mut ratatui::Frame,
    app: &App,
    title: &str,
    count: usize,
    area: ratatui::layout::Rect,
) {
    // Subtle focus indication: highlight the list title when list has focus.
    let bg = if app.shell_focus == ShellFocus::List {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let left = if count == usize::MAX {
        format!(" {title}")
    } else {
        format!(" {title} ({count})")
    };
    let shown = truncate_end(&left, area.width.max(1) as usize);
    let fg = if app.shell_focus == ShellFocus::List {
        theme::parse_color(&app.theme.panel_focused.fg)
    } else {
        theme::parse_color(&app.theme.syntax_text.fg)
    };
    f.render_widget(
        Paragraph::new(shown)
            .style(bg.fg(fg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_main_list(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let banner = if matches!(
        app.shell_view,
        ShellView::Logs | ShellView::Inspect | ShellView::Messages | ShellView::Help
    ) {
        None
    } else {
        app.status_banner()
    };
    let (title_area, banner_area, content_area) = if banner.is_some() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        (chunks[0], Some(chunks[1]), chunks[2])
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        (chunks[0], None, chunks[1])
    };

    match app.shell_view {
        ShellView::Dashboard => {
            draw_shell_title(f, app, "Dashboard", usize::MAX, title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_dashboard(f, app, content_area);
        }
        ShellView::Stacks => {
            draw_shell_title(f, app, "Stacks", app.stacks.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_stacks_table(f, app, content_area);
        }
        ShellView::Containers => {
            draw_shell_title(f, app, "Containers", app.containers.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_containers_table(f, app, content_area);
        }
        ShellView::Images => {
            draw_shell_title(f, app, "Images", app.images_visible_len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_images_table(f, app, content_area);
        }
        ShellView::Volumes => {
            draw_shell_title(f, app, "Volumes", app.volumes_visible_len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_volumes_table(f, app, content_area);
        }
        ShellView::Networks => {
            draw_shell_title(f, app, "Networks", app.networks.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_networks_table(f, app, content_area);
        }
        ShellView::Templates => {
            match app.templates_state.kind {
                TemplatesKind::Stacks => {
                    draw_shell_title(f, app, "Templates: Stacks", app.templates_state.templates.len(), title_area);
                }
                TemplatesKind::Networks => {
                    draw_shell_title(
                        f,
                        app,
                        "Templates: Networks",
                        app.templates_state.net_templates.len(),
                        title_area,
                    );
                }
            }
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_templates_table(f, app, content_area);
        }
        ShellView::Registries => {
            draw_shell_title(
                f,
                app,
                "Registries",
                app.registries_cfg.registries.len(),
                title_area,
            );
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            draw_shell_registries_table(f, app, content_area);
        }
        ShellView::Logs => {
            draw_shell_title(f, app, "Logs", app.logs_total_lines(), title_area);
            draw_shell_logs_view(f, app, content_area);
        }
        ShellView::Inspect => {
            draw_shell_title(f, app, "Inspect", app.inspect.lines.len(), title_area);
            draw_shell_inspect_view(f, app, content_area);
        }
        ShellView::Help => {
            draw_shell_title(f, app, "Help", 0, title_area);
            draw_shell_help_view(f, app, content_area);
        }
        ShellView::Messages => {
            draw_shell_title(f, app, "Messages", app.session_msgs.len(), title_area);
            draw_shell_messages_view(f, app, content_area);
        }
    }
}

fn draw_rate_limit_banner(
    f: &mut ratatui::Frame,
    app: &App,
    banner: Option<String>,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    let text = banner.unwrap_or_default();
    let style = bg
        .patch(app.theme.text_info.to_style())
        .add_modifier(Modifier::BOLD);
    let content = truncate_end(&text, area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(content)
            .style(style)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_main_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.shell_view {
        ShellView::Dashboard => {}
        ShellView::Stacks => draw_shell_stack_details(f, app, area),
        ShellView::Containers => draw_shell_container_details(f, app, area),
        ShellView::Images => draw_shell_image_details(f, app, area),
        ShellView::Volumes => draw_shell_volume_details(f, app, area),
        ShellView::Networks => draw_shell_network_details(f, app, area),
        ShellView::Templates => draw_shell_template_details(f, app, area),
        ShellView::Registries => draw_shell_registry_details(f, app, area),
        ShellView::Logs => draw_shell_logs_meta(f, app, area),
        ShellView::Inspect => draw_shell_inspect_meta(f, app, area),
        ShellView::Help => draw_shell_help_meta(f, app, area),
        ShellView::Messages => draw_shell_messages_meta(f, app, area),
    }
}

fn draw_shell_footer(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = app.theme.footer.to_style();
    f.render_widget(Block::default().style(bg), area);

    let version = format!("v{} ", env!("CARGO_PKG_VERSION"));
    let hint = match app.shell_view {
        ShellView::Dashboard => {
            " F1 help  b sidebar  ^p layout  ^s start  ^o stop  ^r restart  ^d rm  :q quit"
        }
        ShellView::Stacks => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Containers => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Images | ShellView::Volumes | ShellView::Networks => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Templates => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Registries => {
            " F1 help  b sidebar  ^p layout  ^y test  :q quit"
        }
        ShellView::Logs => {
            " F1 help  / search  : cmd  n/N match  m regex  l numbers  q back  :q quit"
        }
        ShellView::Inspect => {
            " F1 help  / search  : cmd  n/N match  m regex  Enter expand  q back  :q quit"
        }
        ShellView::Help => " F1 help  Up/Down scroll  PageUp/PageDown  q back  :q quit",
        ShellView::Messages => {
            " F1 help  Up/Down select  Left/Right hscroll  PgUp/PgDn  ^c copy  ^g toggle  q back  :q quit"
        }
    };

    let w = area.width.max(1) as usize;
    let right_len = version.chars().count();
    let line = if w <= right_len {
        truncate_end(&version, w)
    } else {
        let left_max = w.saturating_sub(right_len + 1);
        let left = truncate_end(hint, left_max);
        let left_len = left.chars().count();
        let gap = w.saturating_sub(right_len + left_len);
        format!("{left}{}{}", " ".repeat(gap), version)
    };
    let line = Line::from(vec![Span::styled(
        line,
        bg.fg(theme::parse_color(&app.theme.footer.fg)),
    )]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_cmdline(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = app.theme.cmdline.to_style();
    f.render_widget(Block::default().style(bg), area);

    let (mode, prefix, input, cursor, show_cursor): (&str, &str, String, usize, bool) =
        if app.shell_cmdline.mode {
            if let Some(confirm) = &app.shell_cmdline.confirm {
                ("CONFIRM", ":", format!("{} (y/n)", confirm.label), 0, false)
            } else {
                (
                    "COMMAND",
                    ":",
                    app.shell_cmdline.input.clone(),
                    app.shell_cmdline.cursor,
                    true,
                )
            }
        } else {
            match app.shell_view {
                ShellView::Logs => match app.logs.mode {
                    LogsMode::Normal => ("CONTAINR", "", String::new(), 0, false),
                    LogsMode::Search => ("SEARCH", "/", app.logs.input.clone(), app.logs.input_cursor, true),
                    LogsMode::Command => (
                        "COMMAND",
                        ":",
                        app.logs.command.clone(),
                        app.logs.command_cursor,
                        true,
                    ),
                },
                ShellView::Inspect => match app.inspect.mode {
                    InspectMode::Normal => ("CONTAINR", "", String::new(), 0, false),
                    InspectMode::Search => (
                        "SEARCH",
                        "/",
                        app.inspect.input.clone(),
                        app.inspect.input_cursor,
                        true,
                    ),
                    InspectMode::Command => (
                        "COMMAND",
                        ":",
                        app.inspect.input.clone(),
                        app.inspect.input_cursor,
                        true,
                    ),
                },
                _ => ("CONTAINR", "", String::new(), 0, false),
            }
        };

    let w = area.width.max(1) as usize;
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(
        format!(" {mode} "),
        app.theme.cmdline_label.to_style(),
    ));

    if !prefix.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

        let fixed_len = format!(" {mode} ").chars().count() + 1 + prefix.chars().count();
        let avail = w.saturating_sub(fixed_len).max(1);
        if show_cursor {
            let input_w = avail.saturating_sub(1).max(1);
            let (before, at, after) = input_window_with_cursor(&input, cursor, input_w);
            spans.push(Span::styled(before, bg));
            spans.push(Span::styled(
                at,
                app.theme.cmdline_cursor.to_style(),
            ));
            spans.push(Span::styled(after, bg));
        } else {
            spans.push(Span::styled(
                truncate_end(&input, avail),
                app.theme.cmdline_inactive.to_style(),
            ));
        }
    } else {
        spans.push(Span::styled(
            "  (press : for commands)",
            app.theme.text_faint.to_style(),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn shell_row_highlight(app: &App) -> Style {
    // Keep selection color consistent across all lists (sidebar/table).
    // Focus is indicated elsewhere (list title / details background).
    // Do not force foreground color so marked rows (yellow) stay visible when selected.
    app.theme.list_selected.to_style()
}

fn shell_header_style(app: &App) -> Style {
    app.theme.table_header.to_style()
}

fn draw_shell_containers_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Reuse existing container row computation logic, but render without outer borders.
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);

    app.ensure_view();
    if app.containers.is_empty() {
        let msg = if app.loading {
            let spinner = loading_spinner(app.loading_since);
            format!("Loading... {spinner}")
        } else if app.last_error.is_some() {
            "Failed to load (see status)".to_string()
        } else {
            "No containers".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(
                    bg.patch(app.theme.text_dim.to_style()),
                )
                .wrap(Wrap { trim: true }),
            area.inner(ratatui::layout::Margin {
                vertical: 0,
                horizontal: 1,
            }),
        );
        return;
    }

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let header = Row::new(vec![
        Cell::from("NAME"),
        Cell::from("IMAGE"),
        Cell::from("UPD"),
        Cell::from("CPU"),
        Cell::from("MEM"),
        Cell::from("STATUS"),
        Cell::from("IP"),
    ])
        .style(shell_header_style(app));

    let mut rows: Vec<Row> = Vec::new();

    let make_container_row = |c: &ContainerRow, name_prefix: &str| -> Row {
        let stopped = is_container_stopped(&c.status);
        let marked = app.is_marked(&c.id);
        let row_style = if marked {
            app.theme.marked.to_style()
        } else if stopped {
            app.theme
                .text_faint
                .to_style()
                .add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };

        let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
        let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
        let ip = app
            .ip_cache
            .get(&c.id)
            .map(|(ip, _)| ip.as_str())
            .unwrap_or("-");
        let status = if let Some(marker) = app.action_inflight.get(&c.id) {
            action_status_prefix(marker.action).to_string()
        } else if let Some(err) = app.container_action_error.get(&c.id) {
            action_error_label(err).to_string()
        } else {
            c.status.clone()
        };
        let status_style = if app.action_inflight.contains_key(&c.id) {
            bg.patch(app.theme.text_warn.to_style())
        } else if let Some(err) = app.container_action_error.get(&c.id) {
            match err.kind {
                ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
            }
        } else {
            row_style
        };

        let name = format!("{name_prefix}{}", c.name);
        let (upd_text, upd_style) = image_update_indicator(
            app,
            image_update_view_for_ref(app, &c.image).1,
            bg,
        );
        Row::new(vec![
            Cell::from(truncate_end(&name, 22)).style(row_style),
            Cell::from(truncate_end(&c.image, 40)).style(row_style),
            Cell::from(upd_text).style(upd_style),
            Cell::from(cpu).style(row_style),
            Cell::from(mem).style(row_style),
            Cell::from(status).style(status_style),
            Cell::from(truncate_end(ip, 15)).style(row_style),
        ])
        .style(row_style)
    };

    if app.list_mode == ListMode::Tree {
        for e in &app.view {
            match e {
                ViewEntry::StackHeader {
                    name,
                    total,
                    running,
                    expanded,
                } => {
                    let st = if *running == 0 {
                        app.theme
                            .text_faint
                            .to_style()
                            .add_modifier(Modifier::BOLD)
                    } else if *running == *total {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD)
                    };
                    let glyph = if *expanded { "▾" } else { "▸" };
                    let (upd_text, upd_style) =
                        image_update_indicator(app, image_update_view_for_stack(app, name), bg);
                    rows.push(
                        Row::new(vec![
                            Cell::from(format!("{glyph} {name}")).style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
                            Cell::from(upd_text).style(upd_style),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                        ])
                        .style(st),
                    );
                }
                ViewEntry::UngroupedHeader { total, running } => {
                    let st = app.theme.text.to_style().add_modifier(Modifier::BOLD);
                    rows.push(
                        Row::new(vec![
                            Cell::from("Ungrouped").style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                        ])
                        .style(st),
                    );
                }
                ViewEntry::Container { id, indent, .. } => {
                    if let Some(idx) = app.container_idx_by_id.get(id).copied() {
                        if let Some(c) = app.containers.get(idx) {
                            let prefix = "  ".repeat(*indent);
                            rows.push(make_container_row(c, &prefix));
                        }
                    }
                }
            }
        }
    } else {
        for c in &app.containers {
            rows.push(make_container_row(c, ""));
        }
    }

    // Keep the same column widths as before; only remove the visual separators.
    let widths = [
        Constraint::Length(22),
        Constraint::Min(20),
        Constraint::Length(3),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(22),
        Constraint::Length(15),
    ];

    let mut state = TableState::default();
    state.select(Some(app.selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(rows, widths)
        .header(header)
        .style(bg)
        .column_spacing(1)
        .row_highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_images_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    const REF_TEXT_MAX: usize = 62;
    const ID_TEXT_MAX: usize = 50;
    const USED_W: usize = 3;
    const SIZE_W: usize = 10;
    const REF_MIN_W: usize = 24;
    const ID_MIN_W: usize = 10;

    let size_cell = |s: &str| -> String {
        // SIZE values are ASCII (e.g. "294MB", "2.06GB"), so fixed-width padding is fine.
        if s.chars().count() >= SIZE_W {
            truncate_end(s, SIZE_W)
        } else {
            format!("{:>width$}", s, width = SIZE_W)
        }
    };

    // Keep columns compact: size REF/ID to the actual visible content (capped),
    // but always reserve space for USED/SIZE.
    let mut max_ref = 0usize;
    let mut max_id = 0usize;
    let mut rows: Vec<Row> = Vec::new();
    for img in app
        .images
        .iter()
        .filter(|img| !app.images_unused_only || !app.image_referenced(img))
    {
        let reference_full = img.name();
        let reference = truncate_end(&reference_full, REF_TEXT_MAX);
        let id = truncate_end(&img.id, ID_TEXT_MAX);
        let key = App::image_row_key(img);
        let marked = app.is_image_marked(&key);
        let row_style = if marked {
            app.theme.marked.to_style()
        } else {
            Style::default()
        };
        let is_removing = app.image_action_inflight.contains_key(&key);
        let err = app.image_action_error.get(&key);
        let used = app
            .image_referenced_count_by_id
            .get(&img.id)
            .copied()
            .unwrap_or(0)
            > 0;
        let used_cell = if used {
            if app.ascii_only {
                "Y"
            } else {
                "✓"
            }
        } else {
            ""
        };
        let size = if is_removing {
            Cell::from(size_cell("removing")).style(bg.patch(app.theme.text_warn.to_style()))
        } else if let Some(err) = err {
            let style = match err.kind {
                ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
            };
            Cell::from(size_cell(action_error_label(err))).style(style)
        } else {
            Cell::from(size_cell(&img.size))
        };
        max_ref = max_ref.max(reference.chars().count());
        max_id = max_id.max(id.chars().count());
        rows.push(
            Row::new(vec![
                Cell::from(reference),
                Cell::from(id),
                Cell::from(used_cell).style(bg.patch(app.theme.text_ok.to_style())),
                size,
            ])
            .style(row_style),
        );
    }
    let inner_w = inner.width.max(1) as usize;
    let spacing = 3; // 4 columns => 3 spaces
    let fixed = USED_W + SIZE_W + spacing;
    let avail = inner_w.saturating_sub(fixed);

    let mut ref_w = max_ref.clamp(REF_MIN_W, REF_TEXT_MAX).min(avail);
    let mut id_w = max_id.clamp(ID_MIN_W, ID_TEXT_MAX).min(avail.saturating_sub(ref_w));
    if ref_w + id_w < avail {
        let extra = avail - (ref_w + id_w);
        let add_ref = extra.min(REF_TEXT_MAX.saturating_sub(ref_w));
        ref_w += add_ref;
        let extra = extra - add_ref;
        id_w = (id_w + extra).min(ID_TEXT_MAX);
    }
    if avail > 0 {
        if ref_w == 0 {
            ref_w = 1.min(avail);
        }
        if id_w == 0 && avail > ref_w {
            id_w = 1;
        }
    }

    let mut state = TableState::default();
    state.select(Some(app.images_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Length(ref_w as u16),
            Constraint::Length(id_w as u16),
            Constraint::Length(USED_W as u16),
            Constraint::Length(SIZE_W as u16),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("REF"),
            Cell::from("ID"),
            Cell::from("USED"),
            Cell::from(size_cell("SIZE")),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_volumes_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let used_cell = |used: usize, bg: Style, app: &App| -> Cell<'static> {
        if used == 0 {
            Cell::from("")
        } else if app.ascii_only {
            Cell::from("Y").style(bg.patch(app.theme.text_ok.to_style()))
        } else {
            Cell::from("✓").style(bg.patch(app.theme.text_ok.to_style()))
        }
    };

    let rows: Vec<Row> = app
        .volumes
        .iter()
        .filter(|v| !app.volumes_unused_only || !app.volume_referenced(v))
        .map(|v| {
            let used = app
                .volume_referenced_count_by_name
                .get(&v.name)
                .copied()
                .unwrap_or(0);
            let marked = app.is_volume_marked(&v.name);
            let st = if marked {
                app.theme.marked.to_style()
            } else {
                Style::default()
            };
            let is_removing = app.volume_action_inflight.contains_key(&v.name);
            let err = app.volume_action_error.get(&v.name);
            let used_cell = if is_removing {
                Cell::from("removing").style(bg.patch(app.theme.text_warn.to_style()))
            } else if let Some(err) = err {
                let style = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                Cell::from(action_error_label(err)).style(style)
            } else {
                used_cell(used, bg, app)
            };
            Row::new(vec![
                Cell::from(v.name.clone()),
                Cell::from(v.driver.clone()),
                used_cell,
            ])
            .style(st)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(app.volumes_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(10),
            Constraint::Length(3),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("DRIVER"),
            Cell::from("USED"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_networks_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let used_cell = |used: bool, bg: Style, app: &App| -> Cell<'static> {
        if !used {
            Cell::from("")
        } else if app.ascii_only {
            Cell::from("Y").style(bg.patch(app.theme.text_ok.to_style()))
        } else {
            Cell::from("✓").style(bg.patch(app.theme.text_ok.to_style()))
        }
    };

    let rows: Vec<Row> = app
        .networks
        .iter()
        .map(|n| {
            let marked = app.is_network_marked(&n.id);
            let st = if marked {
                app.theme.marked.to_style()
            } else if App::is_system_network(n) {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let used = app
                .network_referenced_count_by_id
                .get(&n.id)
                .copied()
                .unwrap_or(0)
                > 0;
            let is_removing = app.network_action_inflight.contains_key(&n.id);
            let err = app.network_action_error.get(&n.id);
            let scope_cell = if is_removing {
                Cell::from("removing").style(bg.patch(app.theme.text_warn.to_style()))
            } else if let Some(err) = err {
                let style = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                Cell::from(action_error_label(err)).style(style)
            } else {
                Cell::from(n.scope.clone())
            };
            Row::new(vec![
                Cell::from(n.name.clone()),
                Cell::from(n.id.clone()),
                Cell::from(n.driver.clone()),
                used_cell(used, bg, app),
                scope_cell,
            ])
            .style(st)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.networks_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            // Keep NAME compact so ID can expand.
            Constraint::Length(16),
            Constraint::Min(16),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("ID"),
            Cell::from("DRIVER"),
            Cell::from("USED"),
            Cell::from("SCOPE"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

struct DetailRow {
    key: &'static str,
    value: String,
    style: Style,
}

fn render_detail_table(
    f: &mut ratatui::Frame,
    app: &App,
    area: ratatui::layout::Rect,
    mut rows: Vec<DetailRow>,
    scroll: usize,
) -> usize {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let table_w = inner.width.max(1) as usize;
    let key_w = 12usize.min(table_w.saturating_sub(1).max(1));
    let val_w = table_w.saturating_sub(key_w + 1).max(1);
    let key_style = bg.patch(app.theme.text_dim.to_style());

    let mut out_lines: Vec<Line<'static>> = Vec::new();
    for row in rows.drain(..) {
        let wrap = matches!(row.key, "Last error" | "Used by");
        let wrapped = if wrap {
            wrap_text(&row.value, val_w)
        } else {
            vec![truncate_end(&row.value, val_w)]
        };
        for (idx, line) in wrapped.into_iter().enumerate() {
            let key = if idx == 0 { row.key } else { "" };
            let key = pad_right(key, key_w);
            out_lines.push(Line::from(vec![
                Span::styled(key, key_style),
                Span::styled(line, row.style),
            ]));
        }
    }

    let max_scroll = out_lines.len().saturating_sub(inner.height.max(1) as usize);
    let scroll = scroll.min(max_scroll);
    let scroll_u16 = scroll.min(u16::MAX as usize) as u16;
    let para = Paragraph::new(out_lines)
        .style(bg)
        .wrap(Wrap { trim: false })
        .scroll((scroll_u16, 0));
    f.render_widget(para, inner);
    scroll
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let next = if line.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", line, word)
        };
        if next.chars().count() > width {
            if !line.is_empty() {
                out.push(truncate_end(&line, width));
            }
            line = word.to_string();
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        out.push(truncate_end(&line, width));
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return truncate_end(text, width);
    }
    let mut out = text.to_string();
    out.push_str(&" ".repeat(width - len + 1));
    out
}

fn format_bytes_short(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0usize;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes}B")
    } else if v >= 10.0 {
        format!("{:.0}{}", v, UNITS[u])
    } else {
        format!("{:.1}{}", v, UNITS[u])
    }
}

fn bar_spans_threshold(
    width: usize,
    ratio: f32,
    ascii_only: bool,
    filled_style: Style,
    empty_style: Style,
) -> Vec<Span<'static>> {
    let width = width.max(1);
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = ((width as f32) * ratio).round() as usize;
    let filled = filled.min(width);
    let (on, off) = if ascii_only { ('#', '.') } else { ('█', '░') };
    let mut out: Vec<Span<'static>> = Vec::new();
    if filled > 0 {
        let mut s = String::with_capacity(filled);
        s.extend(std::iter::repeat(on).take(filled));
        out.push(Span::styled(s, filled_style));
    }
    if width > filled {
        let mut s = String::with_capacity(width - filled);
        s.extend(std::iter::repeat(off).take(width - filled));
        out.push(Span::styled(s, empty_style));
    }
    out
}

fn bar_spans_gradient(
    width: usize,
    ratio: f32,
    ascii_only: bool,
    ok: Style,
    warn: Style,
    err: Style,
    empty_style: Style,
) -> Vec<Span<'static>> {
    let width = width.max(1);
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = ((width as f32) * ratio).round() as usize;
    let filled = filled.min(width);
    let (on, off) = if ascii_only { ('#', '.') } else { ('█', '░') };
    let mut out: Vec<Span<'static>> = Vec::new();

    let mut cur_style: Option<Style> = None;
    let mut cur_buf = String::new();
    for i in 0..filled {
        let pos_ratio = (i + 1) as f32 / (width as f32);
        let st = if pos_ratio >= 0.85 {
            err
        } else if pos_ratio >= 0.70 {
            warn
        } else {
            ok
        };
        if cur_style.map(|c| c == st).unwrap_or(false) {
            cur_buf.push(on);
        } else {
            if !cur_buf.is_empty() {
                out.push(Span::styled(cur_buf, cur_style.unwrap_or(ok)));
                cur_buf = String::new();
            }
            cur_style = Some(st);
            cur_buf.push(on);
        }
    }
    if !cur_buf.is_empty() {
        out.push(Span::styled(cur_buf, cur_style.unwrap_or(ok)));
    }
    if width > filled {
        let mut s = String::with_capacity(width - filled);
        s.extend(std::iter::repeat(off).take(width - filled));
        out.push(Span::styled(s, empty_style));
    }
    out
}

fn draw_shell_dashboard(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    if app.servers.is_empty() && app.current_target.trim().is_empty() {
        let msg = "No server configured. Use :server add to get started.";
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // health strip
            Constraint::Length(1), // spacer
            Constraint::Length(7), // summary table
            Constraint::Length(1), // spacer
            Constraint::Length(7), // metrics table
            Constraint::Min(1),    // notes
        ])
        .split(inner);

    let ok = bg.patch(app.theme.text_ok.to_style());
    let warn = bg.patch(app.theme.text_warn.to_style());
    let err = bg.patch(app.theme.text_error.to_style());
    let dim = bg.patch(app.theme.text_dim.to_style());
    let faint = bg.patch(app.theme.text_faint.to_style());

    let ssh_ok = app.conn_error.is_none();
    let dash_ok = app.dashboard.error.is_none() && app.dashboard.snap.is_some();
    let snap = app.dashboard.snap.as_ref();

    let engine_ok = dash_ok && snap.is_some_and(|s| !s.engine.trim().is_empty() && s.engine != "-");
    let disk_ratio = snap
        .and_then(|s| {
            if s.disk_total_bytes == 0 {
                None
            } else {
                Some((s.disk_used_bytes as f32) / (s.disk_total_bytes as f32))
            }
        })
        .unwrap_or(0.0);
    let mem_ratio = snap
        .and_then(|s| {
            if s.mem_total_bytes == 0 {
                None
            } else {
                Some((s.mem_used_bytes as f32) / (s.mem_total_bytes as f32))
            }
        })
        .unwrap_or(0.0);

    let disk_total = snap.map(|s| s.disk_total_bytes).unwrap_or(0);
    let mem_total = snap.map(|s| s.mem_total_bytes).unwrap_or(0);
    let disk_style = if !dash_ok || disk_total == 0 {
        warn
    } else if disk_ratio >= 0.9 {
        err
    } else if disk_ratio >= 0.8 {
        warn
    } else {
        ok
    };
    let mem_style = if !dash_ok || mem_total == 0 {
        warn
    } else if mem_ratio >= 0.9 {
        err
    } else if mem_ratio >= 0.8 {
        warn
    } else {
        ok
    };

    let badge = |label: &str, st: Style| -> Span<'static> {
        // Keep it readable and consistent with the mock: "[ SSH OK ]".
        Span::styled(format!("[ {label} ]"), st)
    };

    let mut strip: Vec<Span<'static>> = Vec::new();
    strip.push(badge(
        if ssh_ok { "SSH OK" } else { "SSH ERR" },
        if ssh_ok { ok } else { err },
    ));
    strip.push(Span::styled(" ", dim));
    strip.push(badge(
        if engine_ok { "ENGINE OK" } else { "ENGINE ?" },
        if engine_ok { ok } else { warn },
    ));
    strip.push(Span::styled(" ", dim));
    strip.push(badge(
        if disk_style == ok {
            "DISK OK"
        } else if disk_style == err {
            "DISK ERR"
        } else {
            "DISK WARN"
        },
        disk_style,
    ));
    strip.push(Span::styled(" ", dim));
    strip.push(badge(
        if mem_style == ok {
            "MEM OK"
        } else if mem_style == err {
            "MEM ERR"
        } else {
            "MEM WARN"
        },
        mem_style,
    ));
    let unseen_err = app.unseen_error_count();
    if unseen_err > 0 {
        strip.push(Span::styled(" ", dim));
        strip.push(badge(&format!("ERR {unseen_err}"), err));
    }
    f.render_widget(
        Paragraph::new(Line::from(strip)).style(bg).wrap(Wrap { trim: false }),
        chunks[0],
    );

    // Spacer line for readability.
    f.render_widget(Paragraph::new(" ").style(bg), chunks[1]);

    // Summary.
    let (os, kernel, arch, uptime, engine, ts, load1, load5, load15, cores) = if let Some(s) = snap
    {
        (
            s.os.as_str(),
            s.kernel.as_str(),
            s.arch.as_str(),
            s.uptime.as_str(),
            s.engine.as_str(),
            format_session_ts(s.collected_at),
            s.load1,
            s.load5,
            s.load15,
            s.cpu_cores,
        )
    } else if app.dashboard.loading {
        ("Loading...", "-", "-", "-", "-", "-".to_string(), 0.0, 0.0, 0.0, 1)
    } else {
        ("-", "-", "-", "-", "-", "-".to_string(), 0.0, 0.0, 0.0, 1)
    };

    let server = current_server_label(app);
    // Container counts derived from current list (ps -a).
    let mut running = 0usize;
    let mut exited = 0usize;
    let mut paused = 0usize;
    let mut dead = 0usize;
    for c in &app.containers {
        let s = c.status.trim();
        if s.starts_with("Up") || s.starts_with("Restarting") {
            running += 1;
        } else if s.starts_with("Exited") {
            exited += 1;
        } else if s.starts_with("Paused") {
            paused += 1;
        } else if s.starts_with("Dead") {
            dead += 1;
        } else {
            exited += 1;
        }
    }
    let total = app.containers.len();

    let table_w = inner.width.max(1) as usize;
    let key_w = 12usize.min(table_w.saturating_sub(1).max(1));
    let val_w = table_w.saturating_sub(key_w + 1).max(1);
    let k = dim;
    let v = bg.patch(app.theme.text.to_style());
    let summary_rows: Vec<Row> = vec![
        Row::new(vec![
            Cell::from(Span::styled("Server", k)),
            Cell::from(Span::styled(truncate_end(&server, val_w), v)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Host", k)),
            Cell::from(Span::styled(
                truncate_end(&format!("{os} ({kernel} {arch})"), val_w),
                v,
            )),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Uptime", k)),
            Cell::from(Span::styled(truncate_end(uptime, val_w), v)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Engine", k)),
            Cell::from(Span::styled(truncate_end(engine, val_w), v)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Containers", k)),
            Cell::from(Span::styled(
                truncate_end(
                    &format!(
                        "running {running}/{total}  exited {exited}  paused {paused}  dead {dead}"
                    ),
                    val_w,
                ),
                v,
            )),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Updated", k)),
            Cell::from(Span::styled(truncate_end(&ts, val_w), faint)),
        ]),
    ];
    let summary = Table::new(
        summary_rows,
        [Constraint::Length(key_w as u16), Constraint::Min(1)],
    )
    .style(bg)
    .column_spacing(1);
    f.render_widget(summary, chunks[2]);

    f.render_widget(Paragraph::new(" ").style(bg), chunks[3]);

    // CPU (normalize load1 by cores as a coarse signal).
    let load_ratio = if cores == 0 {
        0.0
    } else {
        (load1 / (cores as f32)).clamp(0.0, 1.0)
    };
    // Metrics table (label | value | bar).
    let (mem_used, mem_total2, disk_used, disk_total2) = snap
        .map(|s| (s.mem_used_bytes, s.mem_total_bytes, s.disk_used_bytes, s.disk_total_bytes))
        .unwrap_or((0, 0, 0, 0));
    let mem_ratio2 = if mem_total2 == 0 {
        0.0
    } else {
        (mem_used as f32) / (mem_total2 as f32)
    };
    let disk_ratio2 = if disk_total2 == 0 {
        0.0
    } else {
        (disk_used as f32) / (disk_total2 as f32)
    };

    let metrics_w = inner.width.max(1) as usize;
    let m_key_w = key_w;
    let m_val_w = 20usize.min(metrics_w.saturating_sub(m_key_w + 2).max(10));
    let m_bar_w = metrics_w.saturating_sub(m_key_w + m_val_w + 2).max(10);
    let mk = dim;
    let mv = v;
    let bar_empty = bg.patch(app.theme.text_faint.to_style());
    let bar_ok = bg.patch(app.theme.text_ok.to_style());
    let bar_warn = bg.patch(app.theme.text_warn.to_style());
    let bar_err = bg.patch(app.theme.text_error.to_style());

    let metric_row =
        |name: &str, val: String, bar: Vec<Span<'static>>, extra: Option<String>| -> Row<'static> {
        let mut val = truncate_end(&val, m_val_w);
        if let Some(extra) = extra {
            if !extra.trim().is_empty() {
                let extra = format!(" {extra}");
                val = truncate_end(&(val + &extra), m_val_w);
            }
        }
        let name = truncate_end(name, m_key_w);
        Row::new(vec![
            Cell::from(Span::styled(name, mk)),
            Cell::from(Span::styled(val, mv)),
            Cell::from(Line::from(bar)),
        ])
    };

    let cpu_val = format!("{load1:.2}/{load5:.2}/{load15:.2}");
    let mem_val = format!(
        "{}/{} {:>3.0}%",
        format_bytes_short(mem_used),
        format_bytes_short(mem_total2),
        mem_ratio2 * 100.0
    );
    let dsk_val = format!(
        "{}/{} {:>3.0}%",
        format_bytes_short(disk_used),
        format_bytes_short(disk_total2),
        disk_ratio2 * 100.0
    );

    let cpu_fill = if load_ratio >= 0.85 {
        bar_err
    } else if load_ratio >= 0.70 {
        bar_warn
    } else {
        bar_ok
    };
    let cpu_bar = bar_spans_threshold(m_bar_w, load_ratio, app.ascii_only, cpu_fill, bar_empty);
    let mem_fill = if mem_ratio2 >= 0.85 {
        bar_err
    } else if mem_ratio2 >= 0.70 {
        bar_warn
    } else {
        bar_ok
    };
    let mem_bar = bar_spans_threshold(m_bar_w, mem_ratio2, app.ascii_only, mem_fill, bar_empty);

    let mut metric_rows: Vec<Row> = vec![
        metric_row("CPU", cpu_val, cpu_bar, Some(format!("{cores}c"))),
        metric_row("MEM", mem_val, mem_bar, None),
    ];
    if let Some(s) = snap {
        for (idx, disk) in s.disks.iter().enumerate() {
            let total = disk.total_bytes.max(1);
            let ratio = (disk.used_bytes as f32) / (total as f32);
            let val = format!(
                "{}/{} {:>3.0}%",
                format_bytes_short(disk.used_bytes),
                format_bytes_short(disk.total_bytes),
                ratio * 100.0
            );
            let label = if idx == 0 { "DSK" } else { "" };
            let dsk_bar = bar_spans_gradient(
                m_bar_w,
                ratio,
                app.ascii_only,
                bar_ok,
                bar_warn,
                bar_err,
                bar_empty,
            );
            metric_rows.push(metric_row(&label, val, dsk_bar, None));
        }
        for (idx, nic) in s.nics.iter().take(3).enumerate() {
            let label = if idx == 0 {
                format!("NIC ({})", nic.name)
            } else {
                format!("({})", nic.name)
            };
            let val = nic.addr.clone();
            metric_rows.push(metric_row(&label, val, Vec::new(), None));
        }
    } else {
        let dsk_bar = bar_spans_gradient(
            m_bar_w,
            disk_ratio2,
            app.ascii_only,
            bar_ok,
            bar_warn,
            bar_err,
            bar_empty,
        );
        metric_rows.push(metric_row("DSK", dsk_val, dsk_bar, None));
    }
    let metrics = Table::new(
        metric_rows,
        [
            Constraint::Length(m_key_w as u16),
            Constraint::Length(m_val_w as u16),
            Constraint::Min(1),
        ],
    )
    .style(bg)
    .column_spacing(1);
    f.render_widget(metrics, chunks[4]);

    if let Some(err) = &app.dashboard.error {
        let msg = truncate_end(err, inner.width.max(1) as usize);
        f.render_widget(
            Paragraph::new(format!("Dashboard error: {msg}"))
                .style(bg.patch(app.theme.text_warn.to_style()))
                .wrap(Wrap { trim: true }),
            chunks[5],
        );
    }
}

fn draw_shell_stack_templates_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if let Some(err) = &app.templates_state.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if app.templates_state.templates.is_empty() {
        let msg = format!("No templates in {}", app.stack_templates_dir().display());
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let now = Instant::now();
    let rows: Vec<Row> = app
        .templates_state
        .templates
        .iter()
        .map(|t| {
            let (state, state_style) = if let Some(m) =
                app.templates_state.template_deploy_inflight.get(&t.name)
            {
                let secs = now.duration_since(m.started).as_secs();
                (
                    format!("deploy {secs}s"),
                    Style::default().patch(app.theme.text_warn.to_style()),
                )
            } else if let Some(err) = app.template_action_error.get(&t.name) {
                let st = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                (action_error_label(err).to_string(), st)
            } else if let Some(id) = t.template_id.as_ref() {
                if let Some(list) = app.template_deploys.get(id) {
                    if list.is_empty() {
                        (String::new(), Style::default())
                    } else {
                        ("deployed".to_string(), Style::default())
                    }
                } else {
                    (String::new(), Style::default())
                }
            } else {
                (String::new(), Style::default())
            };
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_compose { "yes" } else { "no" }),
                Cell::from(state).style(state_style),
                Cell::from(t.desc.clone()),
            ])
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.templates_state.templates_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(16),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("COMPOSE"),
            Cell::from("STATE"),
            Cell::from("DESC"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_templates_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => draw_shell_stack_templates_table(f, app, area),
        TemplatesKind::Networks => draw_shell_net_templates_table(f, app, area),
    }
}

fn registry_auth_label(auth: &config::RegistryAuth) -> &'static str {
    match auth {
        config::RegistryAuth::Anonymous => "anonymous",
        config::RegistryAuth::Basic => "basic",
        config::RegistryAuth::BearerToken => "bearer",
        config::RegistryAuth::GithubPat => "github",
    }
}

fn draw_shell_registries_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if app.registries_cfg.registries.is_empty() {
        let msg = "No registries configured (edit via :registry add).".to_string();
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows: Vec<Row> = app
        .registries_cfg
        .registries
        .iter()
        .map(|r| {
            let host = r.host.clone();
            let auth = registry_auth_label(&r.auth).to_string();
            let user = r.username.clone().unwrap_or_else(|| "-".to_string());
            let secret = if r.secret.as_ref().map(|s| s.trim()).unwrap_or("").is_empty() {
                "-"
            } else {
                "yes"
            };
            Row::new(vec![
                Cell::from(host),
                Cell::from(auth),
                Cell::from(user),
                Cell::from(secret),
            ])
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.registries_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(7),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("HOST"),
            Cell::from("AUTH"),
            Cell::from("USER"),
            Cell::from("SECRET"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_net_templates_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if let Some(err) = &app.templates_state.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if app.templates_state.net_templates.is_empty() {
        let msg = format!("No network templates in {}", app.net_templates_dir().display());
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let now = Instant::now();
    let rows: Vec<Row> = app
        .templates_state
        .net_templates
        .iter()
        .map(|t| {
            let (state, state_style) = if let Some(m) =
                app.templates_state.net_template_deploy_inflight.get(&t.name)
            {
                let secs = now.duration_since(m.started).as_secs();
                (
                    format!("deploy {secs}s"),
                    Style::default().patch(app.theme.text_warn.to_style()),
                )
            } else if let Some(err) = app.net_template_action_error.get(&t.name) {
                let st = match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                };
                (action_error_label(err).to_string(), st)
            } else {
                (String::new(), Style::default())
            };
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_cfg { "yes" } else { "no" }),
                Cell::from(state).style(state_style),
                Cell::from(t.desc.clone()),
            ])
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.templates_state.net_templates_selected
            .min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("CFG"),
            Cell::from("STATE"),
            Cell::from("DESC"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_stacks_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if app.stacks.is_empty() {
        f.render_widget(
            Paragraph::new("No stacks found (no compose/stack labels).")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows: Vec<Row> = app
        .stacks
        .iter()
        .map(|s| {
            let row_style = if s.running == 0 {
                bg.patch(app.theme.text_dim.to_style())
            } else {
                Style::default()
            };
            let (upd_text, upd_style) = image_update_indicator(
                app,
                image_update_view_for_stack(app, &s.name),
                bg,
            );
            let mut state = String::new();
            let mut state_style = row_style;
            for c in app
                .containers
                .iter()
                .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(s.name.as_str()))
            {
                if let Some(marker) = app.action_inflight.get(&c.id) {
                    state = action_status_prefix(marker.action).to_string();
                    state_style = bg.patch(app.theme.text_warn.to_style());
                    break;
                }
                if let Some(err) = app.container_action_error.get(&c.id) {
                    state = action_error_label(err).to_string();
                    state_style = match err.kind {
                        ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                        ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                    };
                    break;
                }
            }

            let name_cell = if state.is_empty() {
                Cell::from(s.name.clone())
            } else {
                Cell::from(Line::from(vec![
                    Span::raw(s.name.clone()),
                    Span::styled(format!(" ({state})"), state_style),
                ]))
            };
            let row = Row::new(vec![
                name_cell,
                Cell::from(upd_text).style(upd_style),
                Cell::from(s.total.to_string()),
                Cell::from(s.running.to_string()),
            ]);
            row.style(row_style)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.stacks_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Min(26),
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("UPD"),
            Cell::from("TOTAL"),
            Cell::from("RUN"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_container_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(c) = app.selected_container().cloned() else {
        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("Select a container to see details.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let mut scroll = app.container_details_scroll;
    if app.container_details_id.as_deref() != Some(&c.id) {
        app.container_details_id = Some(c.id.clone());
        scroll = 0;
    }
    let val = bg;
    let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
    let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
    let ip = app
        .ip_cache
        .get(&c.id)
        .map(|(ip, _)| ip.clone())
        .unwrap_or_else(|| "-".to_string());
    let (status_value, status_style) = if let Some(marker) = app.action_inflight.get(&c.id) {
        (
            action_status_prefix(marker.action).to_string(),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.container_action_error.get(&c.id) {
        let style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (action_error_label(err).to_string(), style)
    } else {
        (c.status.clone(), val)
    };
    let (update_text, update_style) = image_update_indicator(
        app,
        image_update_view_for_ref(app, &c.image).1,
        bg,
    );
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: c.name.clone(),
            style: val,
        },
        DetailRow {
            key: "ID",
            value: c.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Image",
            value: c.image.clone(),
            style: val,
        },
        DetailRow {
            key: "Update",
            value: update_text,
            style: update_style,
        },
        DetailRow {
            key: "Status",
            value: status_value,
            style: status_style,
        },
        DetailRow {
            key: "CPU / MEM",
            value: format!("{cpu} / {mem}"),
            style: val,
        },
        DetailRow {
            key: "IP",
            value: ip,
            style: val,
        },
        DetailRow {
            key: "Ports",
            value: c.ports.clone(),
            style: val,
        },
    ];
    if let Some(err) = app.container_action_error.get(&c.id) {
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.container_details_scroll = scroll;
}

fn draw_shell_stack_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);

    let Some(stack) = app.selected_stack_entry() else {
        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("Select a stack to see details.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    let mut containers: Vec<ContainerRow> = app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack.name.as_str()))
        .cloned()
        .collect();
    containers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let mut networks: Vec<NetworkRow> = app
        .networks
        .iter()
        .filter(|n| stack_name_from_labels(&n.labels).as_deref() == Some(stack.name.as_str()))
        .cloned()
        .collect();
    networks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    if containers.is_empty() && networks.is_empty() {
        f.render_widget(
            Paragraph::new("No stack resources found.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let focus = if app.shell_focus == ShellFocus::Details {
        app.stack_details_focus
    } else {
        StackDetailsFocus::Containers
    };
    let containers_focused = focus == StackDetailsFocus::Containers;
    let networks_focused = focus == StackDetailsFocus::Networks;

    if networks.is_empty() {
        draw_stack_containers_table(f, app, inner, &containers, true);
        return;
    }

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(65),
            Constraint::Length(1),
            Constraint::Percentage(35),
        ])
        .split(inner);
    draw_stack_containers_table(f, app, parts[0], &containers, containers_focused);
    draw_shell_hr(f, app, parts[1]);
    draw_stack_networks_table(f, app, parts[2], &networks, networks_focused);
}

fn draw_stack_containers_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    containers: &[ContainerRow],
    focused: bool,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if containers.is_empty() {
        f.render_widget(
            Paragraph::new("No containers in this stack.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let inner_height = inner.height.max(1) as usize;
    let header_rows = 1usize;
    let view_height = inner_height.saturating_sub(header_rows).max(1);
    let scroll = app
        .stacks_details_scroll
        .min(containers.len().saturating_sub(1));
    let rows: Vec<Row> = containers
        .iter()
        .skip(scroll)
        .take(view_height)
        .map(|c| {
            let status = if let Some(marker) = app.action_inflight.get(&c.id) {
                action_status_prefix(marker.action).to_string()
            } else if let Some(err) = app.container_action_error.get(&c.id) {
                action_error_label(err).to_string()
            } else {
                c.status.clone()
            };
            let status_style = if app.action_inflight.contains_key(&c.id) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.container_action_error.get(&c.id) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                bg
            };
            Row::new(vec![
                Cell::from(c.name.clone()),
                Cell::from(c.image.clone()),
                Cell::from(status).style(status_style),
                Cell::from(c.ports.clone()),
            ])
        })
        .collect();
    let header_style = if focused {
        shell_header_style(app)
    } else {
        bg.patch(app.theme.text_dim.to_style())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(26),
            Constraint::Length(28),
            Constraint::Length(14),
            Constraint::Min(12),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("CONTAINER"),
            Cell::from("IMAGE"),
            Cell::from("STATUS"),
            Cell::from("PORTS"),
        ])
        .style(header_style),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_widget(table, inner);
}

fn draw_stack_networks_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    networks: &[NetworkRow],
    focused: bool,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if networks.is_empty() {
        f.render_widget(
            Paragraph::new("No networks in this stack.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let inner_height = inner.height.max(1) as usize;
    let header_rows = 1usize;
    let view_height = inner_height.saturating_sub(header_rows).max(1);
    let scroll = app
        .stacks_networks_scroll
        .min(networks.len().saturating_sub(1));
    let rows: Vec<Row> = networks
        .iter()
        .skip(scroll)
        .take(view_height)
        .map(|n| {
            Row::new(vec![
                Cell::from(n.name.clone()),
                Cell::from(n.driver.clone()),
                Cell::from(n.scope.clone()),
            ])
        })
        .collect();
    let header_style = if focused {
        shell_header_style(app)
    } else {
        bg.patch(app.theme.text_dim.to_style())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![Cell::from("NETWORK"), Cell::from("DRIVER"), Cell::from("SCOPE")])
            .style(header_style),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_widget(table, inner);
}

fn draw_shell_image_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(img) = app.selected_image().cloned() else {
        return;
    };
    let mut scroll = app.image_details_scroll;
    if app.image_details_id.as_deref() != Some(&img.id) {
        app.image_details_id = Some(img.id.clone());
        scroll = 0;
    }
    let used_by = app
        .image_containers_by_id
        .get(&img.id)
        .cloned()
        .unwrap_or_default();
    let used_by = if used_by.is_empty() {
        "-".to_string()
    } else {
        used_by.join(", ")
    };
    let val = bg;
    let key = App::image_row_key(&img);
    let mut rows = vec![
        DetailRow {
            key: "Ref",
            value: img.name(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.image_action_inflight.contains_key(&key) {
                "removing".to_string()
            } else if let Some(err) = app.image_action_error.get(&key) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.image_action_inflight.contains_key(&key) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.image_action_error.get(&key) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "ID",
            value: img.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Size",
            value: img.size.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.image_action_error.get(&key) {
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.image_details_scroll = scroll;
}

fn draw_shell_volume_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(v) = app.selected_volume().cloned() else {
        return;
    };
    let mut scroll = app.volume_details_scroll;
    if app.volume_details_id.as_deref() != Some(&v.name) {
        app.volume_details_id = Some(v.name.clone());
        scroll = 0;
    }
    let used_by = app
        .volume_containers_by_name
        .get(&v.name)
        .map(|xs| xs.join(", "))
        .unwrap_or_else(|| "-".to_string());
    let val = bg;
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: v.name.clone(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.volume_action_inflight.contains_key(&v.name) {
                "removing".to_string()
            } else if let Some(err) = app.volume_action_error.get(&v.name) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.volume_action_inflight.contains_key(&v.name) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.volume_action_error.get(&v.name) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "Driver",
            value: v.driver.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.volume_action_error.get(&v.name) {
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v_style,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.volume_details_scroll = scroll;
}

fn draw_shell_network_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(n) = app.selected_network().cloned() else {
        return;
    };
    let mut scroll = app.network_details_scroll;
    if app.network_details_id.as_deref() != Some(&n.id) {
        app.network_details_id = Some(n.id.clone());
        scroll = 0;
    }
    let is_system = App::is_system_network(&n);
    let used_by = app
        .network_containers_by_id
        .get(&n.id)
        .cloned()
        .unwrap_or_default();
    let used_by = if used_by.is_empty() {
        "-".to_string()
    } else {
        used_by.join(", ")
    };
    let val = bg;
    let type_style = if is_system {
        bg.patch(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        bg.patch(Style::default().fg(Color::White))
    };
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: n.name.clone(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.network_action_inflight.contains_key(&n.id) {
                "removing".to_string()
            } else if let Some(err) = app.network_action_error.get(&n.id) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.network_action_inflight.contains_key(&n.id) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.network_action_error.get(&n.id) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "Type",
            value: if is_system { "System" } else { "User" }.to_string(),
            style: type_style,
        },
        DetailRow {
            key: "ID",
            value: n.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Driver",
            value: n.driver.clone(),
            style: val,
        },
        DetailRow {
            key: "Scope",
            value: n.scope.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.network_action_error.get(&n.id) {
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v_style,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.network_details_scroll = scroll;
}

fn draw_shell_stack_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);
    let status_area = parts[0];
    let content_area = parts[1];

    if let Some(err) = &app.templates_state.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_template().cloned() else {
        f.render_widget(
            Paragraph::new("No template selected.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    if !t.has_compose {
        f.render_widget(
            Paragraph::new("compose.yaml not found in template directory.")
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let content =
        fs::read_to_string(&t.compose_path).unwrap_or_else(|e| format!("read failed: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let lnw = lines.len().max(1).to_string().len();
    let view_h = content_area.height.max(1) as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.templates_state.templates_details_scroll = app.templates_state.templates_details_scroll.min(max_scroll);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len().max(1));
    let ln_style = bg.patch(app.theme.text_faint.to_style());

    for (i, l) in lines.iter().enumerate() {
        let ln = format!("{:>lnw$} ", i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(ln, ln_style));
        spans.extend(yaml_highlight_line(l, bg, &app.theme));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(format!("{:>lnw$} ", 1), ln_style)));
    }

    let (mut status_text, status_style) = if let Some(m) =
        app.templates_state.template_deploy_inflight.get(&t.name)
    {
        let secs = m.started.elapsed().as_secs();
        (
            format!("Status: deploying ({secs}s)"),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.template_action_error.get(&t.name) {
        let st = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (format!("Status: {}", action_error_label(err)), st)
    } else {
        ("Status: -".to_string(), bg.patch(app.theme.text_dim.to_style()))
    };
    let deploy_list = t
        .template_id
        .as_ref()
        .and_then(|id| app.template_deploys.get(id));
    let mut servers: Vec<String> = deploy_list
        .map(|list| list.iter().map(|info| info.server_name.clone()).collect())
        .unwrap_or_default();
    servers.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    servers.dedup();
    let servers_text = if servers.is_empty() {
        "-".to_string()
    } else {
        servers.join(", ")
    };
    if status_text == "Status: -" && servers_text != "-" {
        status_text = "Status: deployed".to_string();
    }
    let info_style = bg.patch(app.theme.text_dim.to_style());
    let status_lines = Text::from(vec![
        Line::from(Span::styled(status_text, status_style)),
        Line::from(Span::styled(format!("Servers: {servers_text}"), info_style)),
    ]);
    f.render_widget(
        Paragraph::new(status_lines).wrap(Wrap { trim: true }),
        status_area,
    );
    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_state.templates_details_scroll.min(u16::MAX as usize) as u16,
            0,
        )),
        content_area,
    );
}

fn draw_shell_template_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => draw_shell_stack_template_details(f, app, area),
        TemplatesKind::Networks => draw_shell_net_template_details(f, app, area),
    }
}

fn draw_shell_registry_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let Some(r) = app.registries_cfg.registries.get(app.registries_selected).cloned() else {
        return;
    };
    let mut scroll = app.registries_details_scroll;
    let host = r.host.trim().to_ascii_lowercase();
    let resolved = app.registry_auths.get(&host);
    let secret_status = if r.secret.as_ref().map(|s| s.trim()).unwrap_or("").is_empty() {
        "missing"
    } else if resolved.and_then(|a| a.secret.as_ref()).is_some() {
        "loaded"
    } else {
        "unavailable"
    };
    let username = r.username.clone().unwrap_or_else(|| "-".to_string());
    let test_repo = r.test_repo.clone().unwrap_or_else(|| "-".to_string());
    let (test_time, test_result) = if let Some(entry) = app.registry_tests.get(&host) {
        let ts = OffsetDateTime::from_unix_timestamp(entry.checked_at)
            .map(format_action_ts)
            .unwrap_or_else(|_| entry.checked_at.to_string());
        let status = if entry.ok { "ok" } else { "error" };
        let result = if entry.message.trim().is_empty() {
            status.to_string()
        } else {
            format!("{status}: {}", entry.message)
        };
        (ts, truncate_end(&result, 120))
    } else {
        ("-".to_string(), "-".to_string())
    };
    let val = bg;
    let rows = vec![
        DetailRow {
            key: "Host",
            value: r.host,
            style: val,
        },
        DetailRow {
            key: "Auth",
            value: registry_auth_label(&r.auth).to_string(),
            style: val,
        },
        DetailRow {
            key: "Username",
            value: username,
            style: val,
        },
        DetailRow {
            key: "Secret",
            value: secret_status.to_string(),
            style: val,
        },
        DetailRow {
            key: "Test repo",
            value: test_repo,
            style: val,
        },
        DetailRow {
            key: "Last test",
            value: test_time,
            style: val,
        },
        DetailRow {
            key: "Test result",
            value: test_result,
            style: val,
        },
        DetailRow {
            key: "Identity",
            value: app.registries_cfg.age_identity.clone(),
            style: val,
        },
    ];
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.registries_details_scroll = scroll;
}

fn draw_shell_net_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    let status_area = parts[0];
    let content_area = parts[1];

    if let Some(err) = &app.templates_state.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_net_template().cloned() else {
        f.render_widget(
            Paragraph::new("No network template selected.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    if !t.has_cfg {
        f.render_widget(
            Paragraph::new("network.json not found in template directory.")
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let content = fs::read_to_string(&t.cfg_path).unwrap_or_else(|e| format!("read failed: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let lnw = lines.len().max(1).to_string().len();
    let view_h = content_area.height.max(1) as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.templates_state.net_templates_details_scroll = app.templates_state.net_templates_details_scroll.min(max_scroll);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len().max(1));
    let ln_style = bg.patch(app.theme.text_faint.to_style());

    for (i, l) in lines.iter().enumerate() {
        let ln = format!("{:>lnw$} ", i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(ln, ln_style));
        spans.extend(json_highlight_line(l, bg, &app.theme));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(format!("{:>lnw$} ", 1), ln_style)));
    }

    let (status_text, status_style) = if let Some(m) =
        app.templates_state.net_template_deploy_inflight.get(&t.name)
    {
        let secs = m.started.elapsed().as_secs();
        (
            format!("Status: deploying ({secs}s)"),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.net_template_action_error.get(&t.name) {
        let st = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (format!("Status: {}", action_error_label(err)), st)
    } else {
        ("Status: -".to_string(), bg.patch(app.theme.text_dim.to_style()))
    };
    f.render_widget(
        Paragraph::new(status_text)
            .style(status_style)
            .wrap(Wrap { trim: true }),
        status_area,
    );
    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_state.net_templates_details_scroll
                .min(u16::MAX as usize) as u16,
            0,
        )),
        content_area,
    );
}

fn yaml_highlight_line(line: &str, base: Style, theme: &theme::ThemeSpec) -> Vec<Span<'static>> {
    // Very small YAML-ish highlighter:
    // - comments: dim
    // - mapping keys: light blue
    let normal = base.patch(theme.syntax_text.to_style());
    let comment = base.patch(theme.syntax_comment.to_style());
    let key_style = base.patch(theme.syntax_key.to_style());

    let (code, comment_part) = split_yaml_comment(line);
    let mut spans: Vec<Span<'static>> = Vec::new();

    if code.trim().is_empty() {
        if !code.is_empty() {
            spans.push(Span::styled(code.to_string(), normal));
        }
    } else if let Some((prefix, key, rest)) = split_yaml_key(code) {
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix.to_string(), normal));
        }
        spans.push(Span::styled(key.to_string(), key_style));
        if !rest.is_empty() {
            spans.push(Span::styled(rest.to_string(), normal));
        }
    } else {
        spans.push(Span::styled(code.to_string(), normal));
    }

    if let Some(c) = comment_part {
        spans.push(Span::styled(c.to_string(), comment));
    }
    spans
}

fn json_highlight_line(line: &str, base: Style, theme: &theme::ThemeSpec) -> Vec<Span<'static>> {
    // Minimal JSON-ish highlighter:
    // - keys ("...":) in light blue
    let normal = base.patch(theme.syntax_text.to_style());
    let key_style = base.patch(theme.syntax_key.to_style());

    let mut spans: Vec<Span<'static>> = Vec::new();
    let Some(start) = line.find('"') else {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    };
    let rest = &line[start + 1..];
    let Some(end_rel) = rest.find('"') else {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    };
    let end = start + 1 + end_rel;
    let after = &line[end + 1..];
    // Only treat it as a key if a ':' follows (allow whitespace).
    let after_trim = after.trim_start();
    if !after_trim.starts_with(':') {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    }

    let prefix = &line[..start];
    let key = &line[start..=end];
    let rest = &line[end + 1..];
    if !prefix.is_empty() {
        spans.push(Span::styled(prefix.to_string(), normal));
    }
    spans.push(Span::styled(key.to_string(), key_style));
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), normal));
    }
    spans
}

fn split_yaml_comment(line: &str) -> (&str, Option<&str>) {
    // Find a '#' that is not inside single/double quotes.
    let mut in_s = false;
    let mut in_d = false;
    let mut prev_bs = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => {
                in_s = !in_s;
                prev_bs = false;
            }
            '"' if !in_s && !prev_bs => {
                in_d = !in_d;
                prev_bs = false;
            }
            '\\' if in_d => {
                prev_bs = !prev_bs;
            }
            '#' if !in_s && !in_d => {
                return (&line[..i], Some(&line[i..]));
            }
            _ => prev_bs = false,
        }
    }
    (line, None)
}

fn split_yaml_key(line: &str) -> Option<(&str, &str, &str)> {
    // Attempts to split "<prefix><key>:<rest>" where key is outside quotes.
    let mut in_s = false;
    let mut in_d = false;
    let mut prev_bs = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => {
                in_s = !in_s;
                prev_bs = false;
            }
            '"' if !in_s && !prev_bs => {
                in_d = !in_d;
                prev_bs = false;
            }
            '\\' if in_d => {
                prev_bs = !prev_bs;
            }
            ':' if !in_s && !in_d => {
                let (left, _right) = line.split_at(i);
                // Walk back to find key token (support "- key:" too).
                let bytes = left.as_bytes();
                let mut j = bytes.len();
                while j > 0 && bytes[j - 1].is_ascii_whitespace() {
                    j -= 1;
                }
                let key_end = j;
                while j > 0 {
                    let b = bytes[j - 1];
                    if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                        j -= 1;
                    } else {
                        break;
                    }
                }
                let key_start = j;
                if key_start == key_end {
                    return None;
                }
                let prefix = &left[..key_start];
                let key = &left[key_start..key_end];
                let rest = &line[key_end..];
                return Some((prefix, key, rest));
            }
            _ => prev_bs = false,
        }
    }
    None
}

fn draw_shell_logs_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Reuse the underlying log renderer, but in a borderless main view.
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let content = cols[0];
    let vbar_area = cols[1];

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(content);
    let list_area = rows[0];
    let hbar_area = rows[1];

    let effective_query = match app.logs.mode {
        LogsMode::Search => app.logs.input.trim(),
        LogsMode::Normal | LogsMode::Command => app.logs.query.trim(),
    };

    let view_height = list_area.height.max(1) as usize;
    let total_lines = app.logs_total_lines();
    let max_scroll = total_lines.saturating_sub(view_height);
    let cursor = if total_lines == 0 {
        0usize
    } else {
        app.logs.cursor.min(total_lines.saturating_sub(1))
    };
    let mut scroll_top = app.logs.scroll_top.min(max_scroll);
    if cursor < scroll_top {
        scroll_top = cursor;
    } else if cursor >= scroll_top.saturating_add(view_height) {
        scroll_top = cursor
            .saturating_add(1)
            .saturating_sub(view_height)
            .min(max_scroll);
    }
    app.logs.scroll_top = scroll_top;

    if app.logs.loading || app.logs.error.is_some() || app.logs.text.is_none() {
        let msg = if app.logs.loading {
            "Loading…".to_string()
        } else if let Some(e) = &app.logs.error {
            format!("error: {e}")
        } else {
            "No logs loaded.".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(
                    bg.patch(app.theme.text_dim.to_style()),
                )
                .wrap(Wrap { trim: true }),
            list_area,
        );
        return;
    }

    let Some(txt) = &app.logs.text else {
        return;
    };
    let total = total_lines.max(1);
    let digits = total.to_string().len().max(1);
    let start = scroll_top;
    let end = (start + view_height).min(total_lines);
    let prefix_w = if app.logs.show_line_numbers {
        digits.saturating_add(1)
    } else {
        0
    };
    let avail_w = list_area.width.max(1) as usize;
    let body_w = avail_w.saturating_sub(prefix_w).max(1);
    let max_hscroll = app.logs.max_width.saturating_sub(body_w);
    app.logs.hscroll = app.logs.hscroll.min(max_hscroll);

    let q = effective_query;
    let sel = app.logs_selection_range();
    let mut items: Vec<ListItem> = Vec::with_capacity(end.saturating_sub(start));
    for (idx, line) in txt.lines().enumerate().take(end).skip(start) {
        let visible = slice_window(line, app.logs.hscroll, body_w);
        let mut l = if app.logs.use_regex {
            let matcher = if q.is_empty() || app.logs.regex_error.is_some() {
                None
            } else {
                app.logs.regex.as_ref()
            };
            highlight_log_line_regex(&visible, matcher)
        } else {
            highlight_log_line_literal(&visible, q)
        };
        if app.logs.show_line_numbers {
            let prefix = format!("{:>width$} ", idx + 1, width = digits);
            l.spans.insert(
                0,
                Span::styled(
                    prefix,
                    bg.patch(app.theme.text_dim.to_style()),
                ),
            );
        }
        let selected = sel.map(|(a, b)| idx >= a && idx <= b).unwrap_or(false);
        let item_style = if selected {
            app.theme.marked.to_style()
        } else {
            Style::default()
        };
        items.push(ListItem::new(l).style(item_style));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }
    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(start)));
    f.render_stateful_widget(list, list_area, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        scroll_top,
        max_scroll,
        total_lines,
        view_height,
        app.ascii_only,
        &app.theme,
    );
    draw_shell_scrollbar_h(
        f,
        hbar_area,
        app.logs.hscroll,
        max_hscroll,
        app.logs.max_width,
        body_w,
        app.ascii_only,
        &app.theme,
    );
}

fn draw_shell_logs_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let q = app.logs.query.trim();
    let matches = if q.is_empty() {
        "Matches: -".to_string()
    } else if app.logs.use_regex && app.logs.regex_error.is_some() {
        "Regex: invalid".to_string()
    } else {
        format!("Matches: {}", app.logs.match_lines.len())
    };
    let re = if app.logs.use_regex {
        "regex:on"
    } else {
        "regex:off"
    };
    let pos = format!(
        "Line: {}/{}",
        app.logs.cursor.saturating_add(1),
        app.logs_total_lines().max(1)
    );
    let line = Line::from(vec![
        Span::styled(matches, Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("Query: ", Style::default().fg(Color::Gray)),
        Span::styled(
            if q.is_empty() { "-" } else { q },
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled(re, Style::default().fg(Color::Gray)),
        Span::raw("   "),
        Span::styled(pos, Style::default().fg(Color::Gray)),
    ]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_inspect_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Reuse inspect tree lines computed in app.inspect.lines.
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let content = cols[0];
    let vbar_area = cols[1];

    let view_height = content.height.max(1) as usize;
    let total_lines = app.inspect.lines.len();
    let max_scroll = total_lines.saturating_sub(view_height);
    let cursor = app.inspect.selected.min(total_lines.saturating_sub(1));
    let mut scroll_top = app.inspect.scroll_top.min(max_scroll);
    if cursor < scroll_top {
        scroll_top = cursor;
    } else if cursor >= scroll_top.saturating_add(view_height) {
        scroll_top = cursor
            .saturating_add(1)
            .saturating_sub(view_height)
            .min(max_scroll);
    }
    app.inspect.scroll_top = scroll_top;

    let start = scroll_top;
    let end = (start + view_height).min(total_lines);
    let avail_w = content.width.max(1) as usize;

    // Clamp horizontal scroll so it does not "virtually" exceed the content width.
    let mut max_len: usize = 0;
    for l in &app.inspect.lines {
        let label_len = l.label.chars().count();
        let summary_len = l.summary.chars().count();
        let line_len = l.depth.saturating_mul(2)
            + 2
            + label_len
            + if summary_len > 0 { 2 + summary_len } else { 0 };
        max_len = max_len.max(line_len);
    }
    let max_hscroll = max_len.saturating_sub(avail_w);
    app.inspect.scroll = app.inspect.scroll.min(max_hscroll);

    let q = app.inspect.query.trim();
    let mut items: Vec<ListItem> = Vec::with_capacity(end.saturating_sub(start));
    for l in app.inspect.lines.iter().take(end).skip(start) {
        let indent = "  ".repeat(l.depth);
        let glyph = if l.expandable {
            if l.expanded { "▾ " } else { "▸ " }
        } else {
            "  "
        };
        let mut text = format!("{indent}{glyph}{}", l.label);
        if !l.summary.is_empty() {
            text.push_str(": ");
            text.push_str(&l.summary);
        }
        let visible = slice_window(&text, app.inspect.scroll, avail_w);
        let line = if app.inspect.mode == InspectMode::Search && !q.is_empty() {
            highlight_log_line_literal(&visible, q)
        } else {
            if l.matches {
                highlight_log_line_literal(&visible, q)
            } else {
                Line::from(visible)
            }
        };
        items.push(ListItem::new(line));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }

    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(start)));
    f.render_stateful_widget(list, content, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        scroll_top,
        max_scroll,
        total_lines,
        view_height,
        app.ascii_only,
        &app.theme,
    );
}

fn draw_shell_inspect_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let (cur, total) = current_match_pos(app);
    let matches = if app.inspect.query.trim().is_empty() {
        "Matches: -".to_string()
    } else {
        format!("Matches: {cur}/{total}")
    };
    let q = app.inspect.query.trim();
    let path = app
        .inspect.lines
        .get(app.inspect.selected)
        .map(|l| l.path.clone())
        .unwrap_or_else(|| "-".to_string());
    let line = Line::from(vec![
        Span::styled(matches, Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("Query: ", Style::default().fg(Color::Gray)),
        Span::styled(
            if q.is_empty() { "-" } else { q },
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled("Path: ", Style::default().fg(Color::Gray)),
        Span::styled(
            truncate_end(&path, inner.width.max(1) as usize / 2),
            Style::default().fg(Color::White),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_help_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let lines = shell_help_lines(&app.theme);
    let total = lines.len().max(1);
    let view_h = inner.height.max(1) as usize;
    let max_scroll = total.saturating_sub(view_h);
    let top = if app.shell_help.scroll == usize::MAX {
        max_scroll
    } else {
        app.shell_help.scroll.min(max_scroll)
    };
    app.shell_help.scroll = top;
    let shown: Vec<Line> = lines.into_iter().skip(top).take(view_h).collect();
    f.render_widget(
        Paragraph::new(shown).style(bg).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_shell_help_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let hint = "Use Up/Down/PageUp/PageDown to scroll. Press q to return.";
    f.render_widget(
        Paragraph::new(hint)
            .alignment(Alignment::Center)
            .style(bg.patch(app.theme.text_dim.to_style()))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn format_session_ts(at: OffsetDateTime) -> String {
    use std::sync::OnceLock;
    static FMT: OnceLock<Vec<time::format_description::FormatItem<'static>>> = OnceLock::new();
    let fmt = FMT.get_or_init(|| {
        time::format_description::parse("[hour]:[minute]:[second]")
            .unwrap_or_else(|_| Vec::new())
    });
    at.format(fmt)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

fn format_action_ts(at: OffsetDateTime) -> String {
    use std::sync::OnceLock;
    static FMT: OnceLock<Vec<time::format_description::FormatItem<'static>>> = OnceLock::new();
    let fmt = FMT.get_or_init(|| {
        time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
            .unwrap_or_else(|_| Vec::new())
    });
    at.format(fmt)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

fn draw_shell_messages_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let list_area = cols[0];
    let vbar_area = cols[1];

    let total_msgs = app.session_msgs.len();
    let total = total_msgs.max(1);
    let view_h = list_area.height.max(1) as usize;
    let max_scroll = total.saturating_sub(view_h);
    let w = list_area.width.max(1) as usize;
    let cursor = if total_msgs == 0 {
        0usize
    } else if app.shell_msgs.scroll == usize::MAX {
        total_msgs.saturating_sub(1)
    } else {
        app.shell_msgs.scroll.min(total_msgs.saturating_sub(1))
    };
    if total_msgs > 0 {
        app.shell_msgs.scroll = cursor;
    }
    let top = cursor.saturating_sub(view_h / 2).min(max_scroll);

    // Clamp horizontal scroll to the selected message width.
    if let Some(m) = app.session_msgs.get(cursor) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let fixed_len = format!("{ts} {lvl} ").chars().count();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let max_h = m.text.chars().count().saturating_sub(msg_w);
        app.shell_msgs.hscroll = app.shell_msgs.hscroll.min(max_h);
    } else {
        app.shell_msgs.hscroll = 0;
    }

    let mut items: Vec<ListItem> = Vec::new();
    for m in app.session_msgs.iter().skip(top).take(view_h) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let lvl_style = match m.level {
            MsgLevel::Info => bg.patch(app.theme.text_dim.to_style()),
            MsgLevel::Warn => bg.patch(app.theme.text_warn.to_style()),
            MsgLevel::Error => bg.patch(app.theme.text_error.to_style()),
        };
        let ts = format_session_ts(m.at);
        let ts_style = bg.patch(app.theme.text_faint.to_style());
        let fixed = format!("{ts} {lvl} ");
        let fixed_len = fixed.chars().count();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let msg = window_hscroll(&m.text, app.shell_msgs.hscroll, msg_w);

        let line = Line::from(vec![
            Span::styled(ts, ts_style),
            Span::raw(" "),
            Span::styled(lvl.to_string(), lvl_style),
            Span::raw(" "),
            Span::styled(msg, bg),
        ]);
        items.push(ListItem::new(line));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }
    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(top)));
    f.render_stateful_widget(list, list_area, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        top,
        max_scroll,
        total,
        view_h,
        app.ascii_only,
        &app.theme,
    );
}

fn window_hscroll(s: &str, start: usize, max: usize) -> String {
    let max = max.max(1);
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max <= 3 {
        let start = start.min(chars.len().saturating_sub(1));
        return chars.into_iter().skip(start).take(max).collect();
    }

    let mut start = start.min(chars.len().saturating_sub(1));
    let show_prefix = start > 0;
    // Reserve space for ellipsis markers.
    let mut avail = max;
    if show_prefix {
        avail = avail.saturating_sub(3);
    }

    let remaining = chars.len().saturating_sub(start);
    let show_suffix = remaining > avail;
    if show_suffix {
        avail = avail.saturating_sub(3);
    }
    if avail == 0 {
        // Fallback: show as much as possible.
        avail = 1;
    }

    // Clamp start so we can fill the window.
    if chars.len() > avail {
        start = start.min(chars.len().saturating_sub(avail));
    } else {
        start = 0;
    }

    let mid: String = chars.iter().copied().skip(start).take(avail).collect();
    let mut out = String::new();
    if show_prefix {
        out.push_str("...");
    }
    out.push_str(&mid);
    if show_suffix {
        out.push_str("...");
    }
    truncate_end(&out, max)
}

fn draw_shell_messages_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let hint = "Up/Down select  Left/Right hscroll  PageUp/PageDown  Home/End  ^c copy  q back";
    f.render_widget(
        Paragraph::new(hint)
            .style(bg.patch(app.theme.text_dim.to_style()))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn shell_escape_sh_arg(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
    {
        return text.to_string();
    }
    let escaped = text.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

fn draw_shell_scrollbar_v(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_top: usize,
    max_scroll: usize,
    total_lines: usize,
    view_height: usize,
    ascii_only: bool,
    theme: &theme::ThemeSpec,
) {
    if area.height == 0 || total_lines == 0 {
        return;
    }
    let mapped_pos = if max_scroll == 0 || total_lines <= 1 {
        0
    } else {
        (scroll_top.min(max_scroll) * (total_lines - 1)) / max_scroll
    };
    let track = if ascii_only { "|" } else { "│" };
    let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(track))
        .thumb_symbol(track)
        .track_style(theme.scroll_track.to_style())
        .thumb_style(theme.scroll_thumb.to_style());
    let mut sb_state = ScrollbarState::new(total_lines)
        .position(mapped_pos)
        .viewport_content_length(view_height.max(1));
    f.render_stateful_widget(sb, area, &mut sb_state);
}

fn draw_shell_scrollbar_h(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_left: usize,
    max_scroll: usize,
    content_width: usize,
    view_width: usize,
    ascii_only: bool,
    theme: &theme::ThemeSpec,
) {
    if area.height == 0 || area.width == 0 || content_width == 0 {
        return;
    }
    let mapped_pos = if max_scroll == 0 || content_width <= 1 {
        0
    } else {
        (scroll_left.min(max_scroll) * (content_width - 1)) / max_scroll
    };
    let track = if ascii_only { "-" } else { "─" };
    let sb = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(track))
        .thumb_symbol(track)
        .track_style(theme.scroll_track.to_style())
        .thumb_style(theme.scroll_thumb.to_style());
    let mut sb_state = ScrollbarState::new(content_width)
        .position(mapped_pos)
        .viewport_content_length(view_width.max(1));
    f.render_stateful_widget(sb, area, &mut sb_state);
}
fn is_container_stopped(status: &str) -> bool {
    let s = status.trim();
    // docker ps STATUS values: "Up ...", "Exited (...) ...", "Created", "Dead"
    !(s.starts_with("Up") || s.starts_with("Restarting"))
}

fn loading_spinner(since: Option<Instant>) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let Some(since) = since else {
        return FRAMES[0];
    };
    let idx = (since.elapsed().as_millis() / 120) as usize % FRAMES.len();
    FRAMES[idx]
}

fn truncate_end(s: &str, max: usize) -> String {
    let max = max.max(1);
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s.chars().take(max).collect();
    }
    let mut out: String = s.chars().take(max - 3).collect();
    out.push_str("...");
    out
}

fn truncate_start(s: &str, max: usize) -> String {
    let max = max.max(1);
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s
            .chars()
            .rev()
            .take(max)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    let tail: String = s
        .chars()
        .rev()
        .take(max - 3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

fn spinner_char(started: Instant, ascii_only: bool) -> char {
    let ms = started.elapsed().as_millis() as u64;
    if ascii_only {
        let frames = ['|', '/', '-', '\\'];
        frames[((ms / 150) % frames.len() as u64) as usize]
    } else {
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        frames[((ms / 120) % frames.len() as u64) as usize]
    }
}
fn stack_name_from_labels(labels: &str) -> Option<String> {
    // docker ps --format exposes labels as a comma-separated "k=v" list.
    // Compose stacks typically set:
    // - com.docker.compose.project=<stack>
    // Swarm stacks often set:
    // - com.docker.stack.namespace=<stack>
    for part in labels.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        if k == "com.docker.compose.project" || k == "com.docker.stack.namespace" {
            return Some(v.to_string());
        }
    }
    None
}

fn action_status_prefix(action: ContainerAction) -> &'static str {
    match action {
        ContainerAction::Start => "Starting...",
        ContainerAction::Stop => "Stopping...",
        ContainerAction::Restart => "Restarting...",
        ContainerAction::Remove => "Removing...",
    }
}
fn slice_window(s: &str, start: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut it = s.chars();
    for _ in 0..start {
        if it.next().is_none() {
            return String::new();
        }
    }
    it.take(width).collect()
}
fn highlight_log_line_regex(line: &str, matcher: Option<&Regex>) -> Line<'static> {
    let Some(re) = matcher else {
        return Line::from(line.to_string());
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(line) {
        let start = m.start();
        let end = m.end();
        if end <= start {
            continue;
        }
        if start > last {
            spans.push(Span::raw(line[last..start].to_string()));
        }
        spans.push(Span::styled(
            line[start..end].to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        last = end;
    }
    if spans.is_empty() {
        return Line::from(line.to_string());
    }
    if last < line.len() {
        spans.push(Span::raw(line[last..].to_string()));
    }
    Line::from(spans)
}

fn highlight_log_line_literal(line: &str, query: &str) -> Line<'static> {
    let q = query.trim();
    if q.is_empty() {
        return Line::from(line.to_string());
    }

    let line_lc = line.to_ascii_lowercase();
    let q_lc = q.to_ascii_lowercase();
    if q_lc.is_empty() {
        return Line::from(line.to_string());
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut start = 0usize;
    while let Some(pos) = line_lc[start..].find(&q_lc) {
        let abs = start + pos;
        if abs > start {
            spans.push(Span::raw(line[start..abs].to_string()));
        }
        let end = abs + q_lc.len();
        spans.push(Span::styled(
            line[abs..end].to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        start = end;
    }
    if spans.is_empty() {
        return Line::from(line.to_string());
    }
    if start < line.len() {
        spans.push(Span::raw(line[start..].to_string()));
    }
    Line::from(spans)
}
fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    // macOS
    if let Ok(()) = pipe_to_cmd("pbcopy", &[], text) {
        return Ok(());
    }
    // Wayland
    if let Ok(()) = pipe_to_cmd("wl-copy", &[], text) {
        return Ok(());
    }
    // X11
    if let Ok(()) = pipe_to_cmd("xclip", &["-selection", "clipboard"], text) {
        return Ok(());
    }

    anyhow::bail!("no clipboard tool found (tried pbcopy, wl-copy, xclip)")
}

fn pipe_to_cmd(cmd: &str, args: &[&str], input: &str) -> anyhow::Result<()> {
    let mut child = StdCommand::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn {}", cmd))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin.write_all(input.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("{} exited with {}", cmd, status);
    }
    Ok(())
}

fn build_inspect_lines(
    root: Option<&Value>,
    expanded: &HashSet<String>,
    match_set: &HashSet<String>,
    query: &str,
) -> Vec<InspectLine> {
    let Some(root) = root else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    let mut buf = String::new();
    build_inspect_lines_inner(
        root,
        expanded,
        match_set,
        "",
        0,
        "$".to_string(),
        &q,
        &mut out,
        &mut buf,
    );
    out
}

fn build_inspect_lines_inner(
    value: &Value,
    expanded: &HashSet<String>,
    match_set: &HashSet<String>,
    path: &str,
    depth: usize,
    label: String,
    query: &str,
    out: &mut Vec<InspectLine>,
    scratch: &mut String,
) {
    let expanded_here = expanded.contains(path);
    let (summary, expandable) = summarize(value);

    scratch.clear();
    let _ = write!(scratch, "{} {} {}", path, label, summary);
    let hay = scratch.to_lowercase();
    let matches = !query.is_empty() && (match_set.contains(path) || hay.contains(query));

    out.push(InspectLine {
        path: path.to_string(),
        depth,
        label,
        summary,
        expandable,
        expanded: expanded_here,
        matches,
    });

    if !(expandable && expanded_here) {
        return;
    }

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child = &map[key];
                let child_path = join_pointer(path, key);
                build_inspect_lines_inner(
                    child,
                    expanded,
                    match_set,
                    &child_path,
                    depth + 1,
                    key.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &idx.to_string());
                build_inspect_lines_inner(
                    child,
                    expanded,
                    match_set,
                    &child_path,
                    depth + 1,
                    idx.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        _ => {}
    }
}

fn summarize(value: &Value) -> (String, bool) {
    match value {
        Value::Null => ("null".to_string(), false),
        Value::Bool(b) => (b.to_string(), false),
        Value::Number(n) => (n.to_string(), false),
        Value::String(s) => (format!("{:?}", s), false),
        Value::Array(arr) => (format!("[{}]", arr.len()), true),
        Value::Object(map) => (format!("{{{}}}", map.len()), true),
    }
}

fn collect_expandable_paths(root: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_expandable_paths_inner(root, "", &mut out);
    out
}

fn collect_expandable_paths_inner(value: &Value, path: &str, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            out.insert(path.to_string());
            for (k, v) in map {
                let p = join_pointer(path, k);
                collect_expandable_paths_inner(v, &p, out);
            }
        }
        Value::Array(arr) => {
            out.insert(path.to_string());
            for (idx, v) in arr.iter().enumerate() {
                let p = join_pointer(path, &idx.to_string());
                collect_expandable_paths_inner(v, &p, out);
            }
        }
        _ => {}
    }
}

fn join_pointer(parent: &str, token: &str) -> String {
    if parent.is_empty() {
        format!("/{}", escape_pointer_token(token))
    } else {
        format!("{}/{}", parent, escape_pointer_token(token))
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn collect_match_paths(root: Option<&Value>, query: &str) -> Vec<String> {
    let Some(root) = root else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut scratch = String::new();
    collect_match_paths_inner(root, "", "$", &q, &mut out, &mut scratch);
    out
}

fn collect_match_paths_inner(
    value: &Value,
    path: &str,
    label: &str,
    query: &str,
    out: &mut Vec<String>,
    scratch: &mut String,
) {
    let (summary, _expandable) = summarize(value);
    scratch.clear();
    let _ = write!(scratch, "{} {} {}", path, label, summary);
    if scratch.to_lowercase().contains(query) {
        out.push(path.to_string());
    }

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = join_pointer(path, key);
                collect_match_paths_inner(&map[key], &child_path, key, query, out, scratch);
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &idx.to_string());
                collect_match_paths_inner(
                    child,
                    &child_path,
                    &idx.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        _ => {}
    }
}

fn ancestors_of_pointer(pointer: &str) -> Vec<String> {
    if pointer.is_empty() {
        return vec!["".to_string()];
    }
    let mut out = vec!["".to_string()];
    let mut current = String::new();
    for token in pointer.split('/').skip(1) {
        current.push('/');
        current.push_str(token);
        out.push(current.clone());
    }
    out
}

fn collect_path_rank(root: Option<&Value>) -> HashMap<String, usize> {
    let Some(root) = root else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    let mut idx = 0usize;
    collect_path_rank_inner(root, "", &mut idx, &mut out);
    out
}

fn collect_path_rank_inner(
    value: &Value,
    path: &str,
    idx: &mut usize,
    out: &mut HashMap<String, usize>,
) {
    out.insert(path.to_string(), *idx);
    *idx += 1;

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = join_pointer(path, key);
                collect_path_rank_inner(&map[key], &child_path, idx, out);
            }
        }
        Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &i.to_string());
                collect_path_rank_inner(child, &child_path, idx, out);
            }
        }
        _ => {}
    }
}

fn current_match_pos(app: &App) -> (usize, usize) {
    let total = app.inspect.match_paths.len();
    if total == 0 {
        return (0, 0);
    }
    let path = app
        .inspect.lines
        .get(app.inspect.selected)
        .map(|l| l.path.as_str())
        .unwrap_or("");
    let idx = app
        .inspect.match_paths
        .iter()
        .position(|p| p == path)
        .map(|i| i + 1)
        .unwrap_or(0);
    (idx, total)
}
