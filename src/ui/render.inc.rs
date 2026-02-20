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

fn write_net_template_cfg(
    templates_dir: &PathBuf,
    name: &str,
    cfg: &str,
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
    anyhow::ensure!(
        !dir.exists(),
        "template already exists: {}",
        dir.display()
    );
    fs::create_dir_all(&dir)?;
    let cfg_path = dir.join("network.json");
    fs::write(&cfg_path, cfg)?;
    Ok(cfg_path)
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

#[derive(serde::Serialize)]
struct NetworkTemplateSpecWrite {
    #[serde(default)]
    description: Option<String>,
    name: String,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default, rename = "ipvlan_mode")]
    ipvlan_mode: Option<String>,
    #[serde(default)]
    internal: Option<bool>,
    #[serde(default)]
    attachable: Option<bool>,
    #[serde(default)]
    ipv4: Option<NetworkTemplateIpv4>,
    #[serde(default)]
    options: Option<BTreeMap<String, String>>,
    #[serde(default)]
    labels: Option<BTreeMap<String, String>>,
}

async fn export_net_template(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    source: &str,
    network_id: &str,
    templates_dir: &PathBuf,
) -> anyhow::Result<String> {
    let network_id = network_id.trim();
    anyhow::ensure!(!network_id.is_empty(), "no network selected");
    let raw = docker::fetch_network_inspect(runner, docker, network_id).await?;
    let net: NetworkInspect =
        serde_json::from_str(&raw).context("network inspect was not valid JSON")?;

    let driver = net
        .driver
        .clone()
        .unwrap_or_else(|| "bridge".to_string());
    let mut options_map: HashMap<String, String> = net.options.clone().unwrap_or_default();
    let mut parent = None;
    let mut ipvlan_mode = None;
    if driver == "ipvlan" {
        if let Some(value) = options_map.remove("parent") {
            if !value.trim().is_empty() {
                parent = Some(value);
            }
        }
        if let Some(value) = options_map.remove("ipvlan_mode") {
            if !value.trim().is_empty() {
                ipvlan_mode = Some(value);
            }
        }
    }
    let options = if options_map.is_empty() {
        None
    } else {
        let mut out = BTreeMap::new();
        for (k, v) in options_map {
            out.insert(k, v);
        }
        Some(out)
    };

    let labels = net.labels.as_ref().map(filter_labels).filter(|m| !m.is_empty());
    let mut ipv4 = None;
    if let Some(ipam) = net.ipam.as_ref() {
        if let Some(cfgs) = ipam.config.as_ref() {
            for cfg in cfgs {
                let Some(subnet) = cfg.subnet.as_ref() else {
                    continue;
                };
                if subnet.contains(':') {
                    continue;
                }
                ipv4 = Some(NetworkTemplateIpv4 {
                    subnet: Some(subnet.clone()),
                    gateway: cfg.gateway.clone(),
                    ip_range: cfg.ip_range.clone(),
                });
                break;
            }
        }
    }

    let spec = NetworkTemplateSpecWrite {
        description: Some(format!("Imported from {source}")),
        name: net.name,
        driver: Some(driver),
        parent,
        ipvlan_mode,
        internal: net.internal,
        attachable: net.attachable,
        ipv4,
        options,
        labels,
    };
    let cfg = serde_json::to_string_pretty(&spec)?;
    write_net_template_cfg(templates_dir, name, &cfg)?;
    Ok(String::new())
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
    crate::ui::render::git::maybe_autocommit_templates(app, kind, action, name)
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







struct CmdlineCompletionContext {
    tokens_before: Vec<String>,
    token_prefix: String,
    token_start: usize,
    cursor_byte: usize,
    quote_prefix: bool,
}

fn cmdline_char_to_byte_index(input: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    match input.char_indices().nth(char_idx) {
        Some((idx, _)) => idx,
        None => input.len(),
    }
}

fn cmdline_completion_context(input: &str, cursor: usize) -> CmdlineCompletionContext {
    let cursor_byte = cmdline_char_to_byte_index(input, cursor);
    let mut tokens_before: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut token_start: Option<usize> = None;
    let mut in_quotes = false;
    let mut escaped = false;

    for (idx, ch) in input[..cursor_byte].char_indices() {
        if escaped {
            token.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quotes = !in_quotes;
            if token_start.is_none() {
                token_start = Some(idx);
            }
            continue;
        }
        if !in_quotes && ch.is_whitespace() {
            if token_start.is_some() {
                tokens_before.push(std::mem::take(&mut token));
                token_start = None;
            }
            continue;
        }
        if token_start.is_none() {
            token_start = Some(idx);
        }
        token.push(ch);
    }

    let (token_prefix, token_start) = if let Some(start) = token_start {
        (token, start)
    } else {
        (String::new(), cursor_byte)
    };
    let quote_prefix = token_start < cursor_byte && input[token_start..cursor_byte].starts_with('"');

    CmdlineCompletionContext {
        tokens_before,
        token_prefix,
        token_start,
        cursor_byte,
        quote_prefix,
    }
}

fn cmdline_common_prefix_len_ci(a: &str, b: &str) -> usize {
    let mut len = 0usize;
    let mut it_a = a.chars();
    let mut it_b = b.chars();
    loop {
        let (ca, cb) = match (it_a.next(), it_b.next()) {
            (Some(a), Some(b)) => (a, b),
            _ => break,
        };
        if ca.to_ascii_lowercase() != cb.to_ascii_lowercase() {
            break;
        }
        len += 1;
    }
    len
}

fn cmdline_common_prefix_ci(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let mut len = strings[0].chars().count();
    for s in strings.iter().skip(1) {
        len = len.min(cmdline_common_prefix_len_ci(&strings[0], s));
    }
    strings[0].chars().take(len).collect()
}

fn cmdline_filter_candidates(prefix: &str, candidates: Vec<String>) -> Vec<String> {
    let prefix_lc = prefix.to_ascii_lowercase();
    let mut out: Vec<String> = candidates
        .into_iter()
        .filter(|c| c.to_ascii_lowercase().starts_with(&prefix_lc))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn cmdline_command_candidates() -> Vec<&'static str> {
    vec![
        "q",
        "help",
        "?",
        "messages",
        "msgs",
        "ack",
        "refresh",
        "theme",
        "git",
        "map",
        "unmap",
        "container",
        "ctr",
        "stack",
        "stacks",
        "stk",
        "image",
        "img",
        "volume",
        "vol",
        "network",
        "net",
        "sidebar",
        "ai",
        "inspect",
        "logs",
        "set",
        "layout",
        "templates",
        "template",
        "tpl",
        "registries",
        "registry",
        "reg",
        "nettemplate",
        "nettpl",
        "ntpl",
        "nt",
        "server",
    ]
}

fn cmdline_scope_candidates() -> Vec<String> {
    vec![
        "always",
        "global",
        "view:dashboard",
        "view:stacks",
        "view:containers",
        "view:images",
        "view:volumes",
        "view:networks",
        "view:templates",
        "view:registries",
        "view:logs",
        "view:inspect",
        "view:messages",
        "view:help",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}

fn cmdline_key_candidates() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in [
        "Enter",
        "Esc",
        "Tab",
        "Backspace",
        "Delete",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Up",
        "Down",
        "Left",
        "Right",
        "Space",
    ] {
        out.push(key.to_string());
    }
    for n in 1..=12 {
        out.push(format!("F{n}"));
        out.push(format!("C-F{n}"));
    }
    for ch in ['a', 'b', 'c', 'd', 'e', 'g', 'k', 'n', 'o', 'p', 'r', 's', 't', 'u', 'y'] {
        out.push(format!("C-{ch}"));
    }
    out
}

fn cmdline_theme_names(app: &App) -> Vec<String> {
    match theme::list_theme_names(&app.config_path) {
        Ok(mut names) => {
            if !names.iter().any(|n| n == "default") {
                names.insert(0, "default".to_string());
            }
            names
        }
        Err(_) => vec![],
    }
}

fn cmdline_server_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = app.servers.iter().map(|s| s.name.clone()).collect();
    names.sort();
    names
}

fn cmdline_template_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = match app.templates_state.kind {
        TemplatesKind::Stacks => app
            .templates_state
            .templates
            .iter()
            .map(|t| t.name.clone())
            .collect(),
        TemplatesKind::Networks => app
            .templates_state
            .net_templates
            .iter()
            .map(|t| t.name.clone())
            .collect(),
    };
    names.sort();
    names
}

fn cmdline_net_template_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = app
        .templates_state
        .net_templates
        .iter()
        .map(|t| t.name.clone())
        .collect();
    names.sort();
    names
}

fn cmdline_registry_hosts(app: &App) -> Vec<String> {
    let mut hosts: Vec<String> = app
        .registries_cfg
        .registries
        .iter()
        .map(|r| r.host.clone())
        .collect();
    hosts.sort();
    hosts
}

fn cmdline_stack_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = app.stacks.iter().map(|s| s.name.clone()).collect();
    names.sort();
    names
}

fn cmdline_normalize_cmd(tokens_before: &[String]) -> (Option<String>, usize) {
    if tokens_before.is_empty() {
        return (None, 0);
    }
    let mut first = tokens_before[0].as_str();
    if first == "!" {
        if let Some(cmd) = tokens_before.get(1) {
            return (Some(cmd.clone()), 1);
        }
        return (None, 1);
    }
    if let Some(rest) = first.strip_prefix(':') {
        first = rest;
    }
    if let Some(rest) = first.strip_prefix('!') {
        if !rest.is_empty() {
            return (Some(rest.to_string()), 0);
        }
    }
    if let Some(rest) = first.strip_suffix('!') {
        if !rest.is_empty() {
            return (Some(rest.to_string()), 0);
        }
    }
    (Some(first.to_string()), 0)
}

fn cmdline_completion_candidates(app: &App, ctx: &CmdlineCompletionContext) -> (String, Vec<String>) {
    let mut leading = String::new();
    let token_index = ctx.tokens_before.len();
    let mut token_prefix = ctx.token_prefix.clone();

    let command_position = token_index == 0
        || (token_index == 1 && ctx.tokens_before.first().is_some_and(|t| t == "!"));

    if command_position {
        if token_prefix.starts_with(':') {
            leading.push(':');
            token_prefix = token_prefix[1..].to_string();
        }
        if token_prefix.starts_with('!') {
            leading.push('!');
            token_prefix = token_prefix[1..].to_string();
        }
        if token_index == 1 {
            leading.push('!');
        }
        let candidates: Vec<String> = cmdline_command_candidates()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        return (leading, cmdline_filter_candidates(&token_prefix, candidates));
    }

    let (cmd_opt, cmd_idx) = cmdline_normalize_cmd(&ctx.tokens_before);
    let Some(cmd_raw) = cmd_opt else {
        return (String::new(), Vec::new());
    };
    let cmd = cmd_raw.to_ascii_lowercase();
    let arg_index = ctx
        .tokens_before
        .len()
        .saturating_sub(cmd_idx.saturating_add(1));
    let sub = ctx
        .tokens_before
        .get(cmd_idx + 1)
        .map(|s| s.as_str())
        .unwrap_or("");

    let candidates: Vec<String> = match cmd.as_str() {
        "theme" => {
            if arg_index == 0 {
                vec!["list", "use", "new", "edit", "rm"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "use" | "edit" | "rm") && arg_index == 1 {
                cmdline_theme_names(app)
            } else {
                Vec::new()
            }
        }
        "server" => {
            if arg_index == 0 {
                vec!["list", "use", "add", "rm", "shell"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "use" | "rm" | "shell") && arg_index == 1 {
                cmdline_server_names(app)
            } else if sub == "add" {
                if arg_index == 2 {
                    vec!["ssh", "local"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                } else if arg_index >= 3 && ctx.token_prefix.starts_with('-') {
                    vec!["-p", "-i", "--cmd"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        "container" | "ctr" => {
            if arg_index == 0 {
                vec![
                    "start",
                    "stop",
                    "restart",
                    "rm",
                    "console",
                    "tree",
                    "check",
                    "updates",
                    "recreate",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if sub == "console" && arg_index >= 1 {
                if ctx.token_prefix.starts_with('-') {
                    vec!["-u".to_string()]
                } else {
                    vec!["bash".to_string(), "sh".to_string()]
                }
            } else {
                Vec::new()
            }
        }
        "stack" | "stacks" | "stk" => {
            if arg_index == 0 {
                vec![
                    "start", "stop", "restart", "rm", "check", "updates", "running", "all",
                    "recreate",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if matches!(sub, "start" | "stop" | "restart" | "rm" | "check")
                && arg_index == 1
            {
                cmdline_stack_names(app)
            } else {
                Vec::new()
            }
        }
        "image" | "img" => {
            if arg_index == 0 {
                vec!["push", "untag", "rm", "remove", "delete"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if sub == "push" {
                if ctx.token_prefix.starts_with('-') {
                    vec!["--registry", "--repo", "--tag", "--image"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                } else if let Some(prev) = ctx.tokens_before.get(cmd_idx + arg_index) {
                    match prev.as_str() {
                        "--registry" => cmdline_registry_hosts(app),
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        "volume" | "vol" => {
            if arg_index == 0 {
                vec!["rm", "remove", "delete"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "network" | "net" => {
            if arg_index == 0 {
                vec!["rm", "remove", "delete"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "templates" => {
            if arg_index == 0 {
                vec!["kind", "toggle"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if sub == "kind" && arg_index == 1 {
                vec!["stacks", "networks", "toggle"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "template" | "tpl" => {
            if arg_index == 0 {
                vec![
                    "add",
                    "new",
                    "edit",
                    "deploy",
                    "rm",
                    "del",
                    "delete",
                    "from",
                    "from-stack",
                    "from-container",
                    "from-network",
                    "kind",
                    "toggle",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if sub == "kind" && arg_index == 1 {
                vec!["stacks", "networks", "toggle"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if sub == "deploy" && arg_index >= 1 {
                if ctx.token_prefix.starts_with('-') {
                    vec!["--pull", "--recreate", "--force-recreate"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    let mut out = cmdline_template_names(app);
                    out.extend(
                        ["--pull", "--recreate", "--force-recreate", "pull", "recreate"]
                            .into_iter()
                            .map(|s| s.to_string()),
                    );
                    out
                }
            } else if matches!(sub, "rm" | "del" | "delete") && arg_index == 1 {
                cmdline_template_names(app)
            } else if sub == "from" && arg_index == 1 {
                vec!["stack", "container", "network"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "nettemplate" | "nettpl" | "ntpl" | "nt" => {
            if arg_index == 0 {
                vec!["add", "new", "edit", "deploy", "rm", "del", "delete"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "deploy" | "rm" | "del" | "delete") && arg_index == 1 {
                cmdline_net_template_names(app)
            } else {
                Vec::new()
            }
        }
        "registries" => {
            if arg_index == 0 {
                vec!["view", "list", "identity"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "registry" | "reg" => {
            if arg_index == 0 {
                vec!["add", "rm", "remove", "del", "set", "test", "default", "list"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "rm" | "remove" | "del" | "test" | "default") && arg_index == 1 {
                cmdline_registry_hosts(app)
            } else if sub == "set" {
                if arg_index == 1 {
                    cmdline_registry_hosts(app)
                } else if arg_index == 2 {
                    vec![
                        "auth",
                        "username",
                        "secret",
                        "secret-file",
                        "test-repo",
                    ]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
                } else if arg_index == 3 {
                    if let Some(field) = ctx.tokens_before.get(cmd_idx + 2) {
                        if field == "auth" {
                            return (
                                String::new(),
                                cmdline_filter_candidates(
                                    &ctx.token_prefix,
                                    vec![
                                        "anonymous".to_string(),
                                        "basic".to_string(),
                                        "bearer".to_string(),
                                        "github".to_string(),
                                    ],
                                ),
                            );
                        }
                    }
                    Vec::new()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        "set" => {
            if arg_index == 0 {
                vec![
                    "refresh",
                    "logtail",
                    "history",
                    "git_autocommit",
                    "git_autocommit_confirm",
                    "editor",
                    "image_update_concurrency",
                    "image_update_debug",
                    "image_update_autocheck",
                    "kitty_graphics",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if matches!(
                sub,
                "git_autocommit"
                    | "git_autocommit_confirm"
                    | "image_update_debug"
                    | "image_update_autocheck"
                    | "kitty_graphics"
            ) && arg_index == 1
            {
                vec!["on", "off", "true", "false"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if sub == "editor" && arg_index == 1 {
                vec!["reset".to_string()]
            } else {
                Vec::new()
            }
        }
        "layout" => {
            if arg_index == 0 {
                vec!["horizontal", "vertical", "toggle"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "sidebar" => {
            if arg_index == 0 {
                vec!["toggle", "compact"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "logs" => {
            if arg_index == 0 {
                vec!["reload", "refresh", "copy"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "messages" | "msgs" => {
            if arg_index == 0 {
                vec!["copy", "save", "save!"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "log" => {
            if arg_index == 0 {
                vec!["dock"].into_iter().map(|s| s.to_string()).collect()
            } else {
                Vec::new()
            }
        }
        "ack" => {
            if arg_index == 0 {
                vec!["all".to_string()]
            } else {
                Vec::new()
            }
        }
        "git" => {
            if arg_index == 0 {
                vec!["templates", "themes"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if arg_index == 1 {
                vec![
                    "status",
                    "diff",
                    "log",
                    "commit",
                    "config",
                    "pull",
                    "push",
                    "init",
                    "clone",
                    "autocommit",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if sub == "config" && arg_index == 2 {
                vec!["user.name", "user.email"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "commit" | "autocommit") && arg_index == 2 {
                vec!["-m".to_string()]
            } else {
                Vec::new()
            }
        }
        "map" => {
            if arg_index == 0 {
                let mut out = vec!["list".to_string()];
                out.extend(cmdline_scope_candidates());
                out
            } else if arg_index == 1 {
                cmdline_key_candidates()
            } else {
                Vec::new()
            }
        }
        "unmap" => {
            if arg_index == 0 {
                cmdline_scope_candidates()
            } else if arg_index == 1 {
                cmdline_key_candidates()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    (String::new(), cmdline_filter_candidates(&ctx.token_prefix, candidates))
}

fn cmdline_apply_completion(app: &mut App) {
    let input = app.shell_cmdline.input.clone();
    let cursor = app.shell_cmdline.cursor;
    let ctx = cmdline_completion_context(&input, cursor);
    let (leading, mut matches) = cmdline_completion_candidates(app, &ctx);
    if matches.is_empty() {
        return;
    }

    let mut prefix = ctx.token_prefix.clone();
    if !leading.is_empty() && prefix.starts_with(':') {
        prefix = prefix.trim_start_matches(':').to_string();
    }
    if leading.contains('!') && prefix.starts_with('!') {
        prefix = prefix.trim_start_matches('!').to_string();
    }

    let single_match = matches.len() == 1;
    let replacement = if single_match {
        matches[0].clone()
    } else {
        let common = cmdline_common_prefix_ci(&matches);
        if common.len() > prefix.len() {
            common
        } else {
            String::new()
        }
    };

    if replacement.is_empty() {
        let max = 12usize;
        if matches.len() > max {
            let rest = matches.len() - max;
            matches.truncate(max);
            app.set_info(format!(
                "matches: {} ... +{rest} more",
                matches.join(" ")
            ));
        } else {
            app.set_info(format!("matches: {}", matches.join(" ")));
        }
        return;
    }

    let mut replace_text = format!("{leading}{replacement}");
    if ctx.quote_prefix {
        replace_text = format!("\"{}", replace_text);
    }

    let mut new_input = String::new();
    new_input.push_str(&input[..ctx.token_start]);
    new_input.push_str(&replace_text);
    new_input.push_str(&input[ctx.cursor_byte..]);
    app.shell_cmdline.input = new_input;
    app.shell_cmdline.cursor =
        app.shell_cmdline.input[..ctx.token_start + replace_text.len()].chars().count();

    if single_match {
        let after = &app.shell_cmdline.input[ctx.token_start + replace_text.len()..];
        if after.is_empty() {
            app.shell_cmdline.input.push(' ');
            app.shell_cmdline.cursor += 1;
        }
    } else {
        let max = 12usize;
        if matches.len() > max {
            let rest = matches.len() - max;
            matches.truncate(max);
            app.set_info(format!(
                "matches: {} ... +{rest} more",
                matches.join(" ")
            ));
        } else {
            app.set_info(format!("matches: {}", matches.join(" ")));
        }
    }
}









// moved to ui::state::actions::exec_image_action













fn handle_shell_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
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
                    commands::cmdline_cmd::execute_cmdline(
                        app,
                        &cmd,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_refresh_tx,
                        dash_all_enabled_tx,
                        refresh_interval_tx,
                        refresh_pause_tx,
                        image_update_limit_tx,
                        inspect_req_tx,
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
        app.refresh_now(
            refresh_tx,
            dash_refresh_tx,
            dash_all_refresh_tx,
            refresh_pause_tx,
        );
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
                    commands::cmdline_cmd::execute_cmdline(
                        app,
                        &cmdline,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_refresh_tx,
                        dash_all_enabled_tx,
                        refresh_interval_tx,
                        refresh_pause_tx,
                        image_update_limit_tx,
                        inspect_req_tx,
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
                commands::cmdline_cmd::execute_cmdline(
                    app,
                    &cmdline,
                    conn_tx,
                    refresh_tx,
                    dash_refresh_tx,
                    dash_all_refresh_tx,
                    dash_all_enabled_tx,
                    refresh_interval_tx,
                    refresh_pause_tx,
                    image_update_limit_tx,
                    inspect_req_tx,
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
            KeyCode::Tab => {
                cmdline_apply_completion(app);
            }
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
                        "q" | "quit" => app.back_from_full_view(),
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
                        "q" | "quit" => app.back_from_full_view(),
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

    if app.shell_view == ShellView::ThemeSelector {
        match key.code {
            KeyCode::Esc => {
                if app.theme_selector.search_mode {
                    app.theme_selector.search_mode = false;
                    app.theme_selector.search_input.clear();
                    app.theme_selector.search_cursor = 0;
                } else {
                    app.theme_selector_cancel();
                }
                return;
            }
            KeyCode::Char('q') if key.modifiers.is_empty() => {
                app.theme_selector_cancel();
                return;
            }
            KeyCode::Enter => {
                if app.theme_selector.search_mode {
                    app.theme_selector.search_mode = false;
                } else {
                    app.theme_selector_apply();
                }
                return;
            }
            KeyCode::Char('/') if key.modifiers.is_empty() => {
                app.theme_selector.search_mode = true;
                app.theme_selector.search_input.clear();
                app.theme_selector.search_cursor = 0;
                return;
            }
            _ => {}
        }

        if app.theme_selector.search_mode {
            match key.code {
                KeyCode::Backspace => {
                    backspace_at_cursor(
                        &mut app.theme_selector.search_input,
                        &mut app.theme_selector.search_cursor,
                    );
                    let query = app.theme_selector.search_input.clone();
                    app.theme_selector_search(&query);
                    return;
                }
                KeyCode::Delete => {
                    delete_at_cursor(
                        &mut app.theme_selector.search_input,
                        &mut app.theme_selector.search_cursor,
                    );
                    let query = app.theme_selector.search_input.clone();
                    app.theme_selector_search(&query);
                    return;
                }
                KeyCode::Left => {
                    app.theme_selector.search_cursor =
                        clamp_cursor_to_text(&app.theme_selector.search_input, app.theme_selector.search_cursor)
                            .saturating_sub(1);
                    return;
                }
                KeyCode::Right => {
                    let len = app.theme_selector.search_input.chars().count();
                    app.theme_selector.search_cursor =
                        clamp_cursor_to_text(&app.theme_selector.search_input, app.theme_selector.search_cursor)
                            .saturating_add(1)
                            .min(len);
                    return;
                }
                KeyCode::Home => {
                    app.theme_selector.search_cursor = 0;
                    return;
                }
                KeyCode::End => {
                    app.theme_selector.search_cursor = app.theme_selector.search_input.chars().count();
                    return;
                }
                KeyCode::Char(ch) => {
                    if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        insert_char_at_cursor(
                            &mut app.theme_selector.search_input,
                            &mut app.theme_selector.search_cursor,
                            ch,
                        );
                        let query = app.theme_selector.search_input.clone();
                        app.theme_selector_search(&query);
                        return;
                    }
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Up => {
                app.theme_selector_move(-1);
            }
            KeyCode::Down => {
                app.theme_selector_move(1);
            }
            KeyCode::PageUp => {
                app.theme_selector_page_move(-1);
            }
            KeyCode::PageDown => {
                app.theme_selector_page_move(1);
            }
            KeyCode::Home => {
                app.theme_selector_move(-(app.theme_selector.selected as i32));
            }
            KeyCode::End => {
                let last = app
                    .theme_selector
                    .names
                    .len()
                    .saturating_sub(1) as i32;
                let delta = last.saturating_sub(app.theme_selector.selected as i32);
                app.theme_selector_move(delta);
            }
            _ => {}
        }
        return;
    }

    // Custom key bindings (outside of input modes). Skip single-letter shortcuts when sidebar has focus.
    if let Some(spec) = key_spec_from_event(key) {
        if app.shell_focus != ShellFocus::Sidebar || !is_single_letter_without_modifiers(spec) {
            if let Some(hit) = lookup_scoped_binding(app, spec) {
                match hit {
                    BindingHit::Disabled => return,
                    BindingHit::Cmd(cmd) => {
                        commands::cmdline_cmd::execute_cmdline(
                            app,
                            &cmd,
                            conn_tx,
                            refresh_tx,
                            dash_refresh_tx,
                            dash_all_refresh_tx,
                            dash_all_enabled_tx,
                            refresh_interval_tx,
                            refresh_pause_tx,
                            image_update_limit_tx,
                            inspect_req_tx,
                            logs_req_tx,
                            action_req_tx,
                        );
                        return;
                    }
                }
            }
        }
    }

    if app.log_dock_enabled
        && app.shell_focus == ShellFocus::Dock
        && !matches!(
            app.shell_view,
            ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages | ShellView::ThemeSelector
        )
    {
        match key.code {
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
        }
        if !matches!(key.code, KeyCode::Tab) {
            return;
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
            app.back_from_full_view();
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
                    app.switch_server(
                        i,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_enabled_tx,
                    );
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
                ] {
                    if ch_lc == shell_module_shortcut(v) {
                        app.set_main_view(v);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
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
                        app.switch_server(
                            i,
                            conn_tx,
                            refresh_tx,
                            dash_refresh_tx,
                            dash_all_enabled_tx,
                        )
                    }
                    ShellSidebarItem::Module(v) => match v {
                        ShellView::Inspect => app.enter_inspect(inspect_req_tx),
                        ShellView::Logs => app.enter_logs(logs_req_tx),
                        _ => {
                            app.set_main_view(v);
                            shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                        }
                    },
                    ShellSidebarItem::Action(a) => {
                        actions::execute_action(app, a, inspect_req_tx, logs_req_tx, action_req_tx)
                    }
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
        ShellView::ThemeSelector => {}
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
    let mut global_loading = app.logs.loading
        || app.inspect.loading
        || !app.action_inflight.is_empty()
        || !app.image_action_inflight.is_empty()
        || !app.volume_action_inflight.is_empty()
        || !app.network_action_inflight.is_empty()
        || !app.templates_state.template_deploy_inflight.is_empty()
        || !app.templates_state.net_template_deploy_inflight.is_empty()
        || !app.stack_update_inflight.is_empty()
        || !app.image_updates_inflight.is_empty();
    if app.server_all_selected {
        global_loading = global_loading || app.dashboard_all.hosts.iter().any(|h| h.loading);
    } else {
        global_loading = global_loading || app.loading || app.dashboard.loading;
    }
    let refresh_icon = if app.ascii_only { "r" } else { "⏱" };
    let refresh_label = format!("{refresh_icon} {}s", app.refresh_secs.max(1));
    let commit_label = if commands::git_cmd::git_available() && app.git_autocommit {
        "  Commit: auto"
    } else {
        ""
    };
    let mid = format!(
        "Server: {server}  {conn} connected{err_badge}  {refresh_label}{commit_label}  View: {}{crumb}{deploy}",
        app.shell_view.title(),
    );
    let right = if global_loading {
        dot_spinner(app.ascii_only).to_string()
    } else {
        String::new()
    };

    let w = area.width.max(1) as usize;
    let mut line = String::new();
    line.push_str(left);
    line.push_str(&mid);
    let min_right = right.chars().count();
    let shown = truncate_end(&line, w.saturating_sub(min_right));
    let rem = w.saturating_sub(shown.chars().count());
    let right_shown = truncate_start(&right, rem);
    let right_len = right_shown.chars().count();
    let gap = rem.saturating_sub(right_len);

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
        if gap > 0 {
            spans.push(Span::styled(" ".repeat(gap), bg));
        }
        spans.push(Span::styled(right_shown, bg.fg(Color::Gray)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(in crate::ui) fn draw_shell_main_list(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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
            crate::ui::views::dashboard::render_dashboard(f, app, content_area);
        }
        ShellView::Stacks => {
            draw_shell_title(f, app, "Stacks", app.stacks.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            crate::ui::views::stacks::render_stacks(f, app, content_area);
        }
        ShellView::Containers => {
            draw_shell_title(f, app, "Containers", app.containers.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            crate::ui::views::containers::render_containers(f, app, content_area);
        }
        ShellView::Images => {
            draw_shell_title(f, app, "Images", app.images_visible_len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            crate::ui::views::images::render_images(f, app, content_area);
        }
        ShellView::Volumes => {
            draw_shell_title(f, app, "Volumes", app.volumes_visible_len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            crate::ui::views::volumes::render_volumes(f, app, content_area);
        }
        ShellView::Networks => {
            draw_shell_title(f, app, "Networks", app.networks.len(), title_area);
            if let Some(area) = banner_area {
                draw_rate_limit_banner(f, app, banner, area);
            }
            crate::ui::views::networks::render_networks(f, app, content_area);
        }
        ShellView::Templates => match app.templates_state.kind {
            TemplatesKind::Stacks => {
                draw_shell_title(
                    f,
                    app,
                    "Templates: Stacks",
                    app.templates_state.templates.len(),
                    title_area,
                );
                if let Some(area) = banner_area {
                    draw_rate_limit_banner(f, app, banner, area);
                }
                crate::ui::views::templates::render_templates(f, app, content_area);
            }
            TemplatesKind::Networks => {
                draw_shell_title(
                    f,
                    app,
                    "Templates: Networks",
                    app.templates_state.net_templates.len(),
                    title_area,
                );
                if let Some(area) = banner_area {
                    draw_rate_limit_banner(f, app, banner, area);
                }
                crate::ui::views::templates::render_templates(f, app, content_area);
            }
        },
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
            crate::ui::views::registries::render_registries(f, app, content_area);
        }
        ShellView::Logs => {
            draw_shell_title(f, app, "Logs", app.logs_total_lines(), title_area);
            crate::ui::views::logs::render_logs(f, app, content_area);
        }
        ShellView::Inspect => {
            draw_shell_title(f, app, "Inspect", app.inspect.lines.len(), title_area);
            crate::ui::views::inspect::render_inspect(f, app, content_area);
        }
        ShellView::Help => {
            draw_shell_title(f, app, "Help", 0, title_area);
            crate::ui::views::help::render_help(f, app, content_area);
        }
        ShellView::Messages => {
            draw_shell_title(f, app, "Messages", app.session_msgs.len(), title_area);
            crate::ui::views::messages::render_messages(f, app, content_area);
        }
        ShellView::ThemeSelector => {
            draw_shell_title(f, app, "Themes", 0, title_area);
        }
    }
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
                ShellView::ThemeSelector => {
                    if app.theme_selector.search_mode {
                        (
                            "SEARCH",
                            "/",
                            app.theme_selector.search_input.clone(),
                            app.theme_selector.search_cursor,
                            true,
                        )
                    } else {
                        ("CONTAINR", "", String::new(), 0, false)
                    }
                }
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

pub(in crate::ui) fn shell_header_style(app: &App) -> Style {
    app.theme.table_header.to_style()
}

pub(in crate::ui) fn draw_shell_containers_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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
        let status = if app.is_stack_update_container(&c.id) {
            "Updating...".to_string()
        } else if let Some(marker) = app.action_inflight.get(&c.id) {
            action_status_prefix(marker.action).to_string()
        } else if let Some(err) = app.container_action_error.get(&c.id) {
            action_error_label(err).to_string()
        } else {
            c.status.clone()
        };
        let status_style = if app.is_stack_update_container(&c.id) {
            bg.patch(app.theme.text_warn.to_style())
        } else if app.action_inflight.contains_key(&c.id) {
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
            resolve_image_update_state(app, &c.image).1,
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
                    let mut name_text = format!("{glyph} {name}");
                    if let Some(marker) = app.stack_update_inflight.get(name) {
                        let secs = marker.started.elapsed().as_secs();
                        name_text.push_str(&format!(" (Updating {secs}s)"));
                    }
                    let (upd_text, upd_style) = if app.stack_update_error.contains_key(name) {
                        (
                            "!".to_string(),
                            bg.patch(app.theme.text_error.to_style()),
                        )
                    } else {
                        image_update_indicator(app, resolve_stack_update_state(app, name), bg)
                    };
                    rows.push(
                        Row::new(vec![
                            Cell::from(name_text).style(st),
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

pub(in crate::ui) fn draw_shell_images_table(
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

pub(in crate::ui) fn draw_shell_volumes_table(
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

pub(in crate::ui) fn draw_shell_networks_table(
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


pub(in crate::ui) fn draw_shell_stack_templates_table(
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
    let mut max_state = "STATE".chars().count();
    let active_server = app.active_server.as_deref();
    let git_status_cell =
        |dirty: bool, status: GitRemoteStatus, untracked: bool| -> Cell<'static> {
        let left = if dirty { "!" } else { "✓" };
        let left_style = if dirty {
            bg.patch(app.theme.text_warn.to_style())
        } else {
            bg.patch(app.theme.text_ok.to_style())
        };
        let (right, right_style) = if untracked {
            (" ", bg)
        } else {
            match status {
                GitRemoteStatus::UpToDate => ("✓", bg.patch(app.theme.text_ok.to_style())),
                GitRemoteStatus::Ahead => ("↑", bg.patch(app.theme.text_info.to_style())),
                GitRemoteStatus::Behind => ("↓", bg.patch(app.theme.text_warn.to_style())),
                GitRemoteStatus::Diverged => ("!", bg.patch(app.theme.text_error.to_style())),
                GitRemoteStatus::Unknown => ("·", bg.patch(app.theme.text_dim.to_style())),
            }
        };
        Cell::from(Line::from(vec![
            Span::styled(left, left_style),
            Span::styled(right, right_style),
        ]))
    };
    let rows: Vec<Row> = app
        .templates_state
        .templates
        .iter()
        .map(|t| {
            let dirty = app.templates_state.dirty_templates.contains(&t.name);
            let untracked = app.templates_state.untracked_templates.contains(&t.name);
            let git_status = app
                .templates_state
                .git_remote_templates
                .get(&t.name)
                .copied()
                .unwrap_or(GitRemoteStatus::Unknown);
            let (deployed_any, deployed_on_active) = if let Some(id) = t.template_id.as_ref() {
                if let Some(list) = app.template_deploys.get(id) {
                    let any = !list.is_empty();
                    let on_active = active_server
                        .map(|srv| list.iter().any(|e| e.server_name == srv))
                        .unwrap_or(any);
                    (any, on_active)
                } else {
                    (false, false)
                }
            } else {
                (false, false)
            };
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
            } else if deployed_any {
                ("deployed".to_string(), Style::default())
            } else {
                (String::new(), Style::default())
            };
            let row_style = if deployed_on_active || app.templates_state.template_deploy_inflight.contains_key(&t.name) {
                Style::default()
            } else {
                bg.patch(app.theme.text_dim.to_style()).add_modifier(Modifier::DIM)
            };
            max_state = max_state.max(state.chars().count());
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_compose { "yes" } else { "no" }),
                Cell::from(state).style(state_style),
                git_status_cell(dirty, git_status, untracked),
                Cell::from(t.desc.clone()),
            ])
            .style(row_style)
        })
        .collect();
    let state_w = max_state.clamp(10, 22) as u16;

    let mut state = TableState::default();
    state.select(Some(
        app.templates_state.templates_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(state_w),
            Constraint::Length(3),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("COMPOSE"),
            Cell::from("STATE"),
            Cell::from("GIT"),
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

pub(in crate::ui) fn draw_shell_templates_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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

pub(in crate::ui) fn draw_shell_registries_table(
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
            let is_default = app
                .registries_cfg
                .default_registry
                .as_ref()
                .map(|h| h.eq_ignore_ascii_case(&host))
                .unwrap_or(false);
            let def = if is_default {
                Cell::from(Span::styled(
                    "✓",
                    bg.patch(app.theme.text_ok.to_style()),
                ))
            } else {
                Cell::from("")
            };
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
                def,
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
            Constraint::Length(7),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("HOST"),
            Cell::from("AUTH"),
            Cell::from("USER"),
            Cell::from("SECRET"),
            Cell::from("DEFAULT"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

pub(in crate::ui) fn draw_shell_net_templates_table(
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
    let mut max_state = "STATE".chars().count();
    let active_server = app.active_server.as_deref();
    let git_status_cell =
        |dirty: bool, status: GitRemoteStatus, untracked: bool| -> Cell<'static> {
        let left = if dirty { "!" } else { "✓" };
        let left_style = if dirty {
            bg.patch(app.theme.text_warn.to_style())
        } else {
            bg.patch(app.theme.text_ok.to_style())
        };
        let (right, right_style) = if untracked {
            (" ", bg)
        } else {
            match status {
                GitRemoteStatus::UpToDate => ("✓", bg.patch(app.theme.text_ok.to_style())),
                GitRemoteStatus::Ahead => ("↑", bg.patch(app.theme.text_info.to_style())),
                GitRemoteStatus::Behind => ("↓", bg.patch(app.theme.text_warn.to_style())),
                GitRemoteStatus::Diverged => ("!", bg.patch(app.theme.text_error.to_style())),
                GitRemoteStatus::Unknown => ("·", bg.patch(app.theme.text_dim.to_style())),
            }
        };
        Cell::from(Line::from(vec![
            Span::styled(left, left_style),
            Span::styled(right, right_style),
        ]))
    };
    let rows: Vec<Row> = app
        .templates_state
        .net_templates
        .iter()
        .map(|t| {
            let dirty = app.templates_state.dirty_net_templates.contains(&t.name);
            let untracked = app
                .templates_state
                .untracked_net_templates
                .contains(&t.name);
            let git_status = app
                .templates_state
                .git_remote_net_templates
                .get(&t.name)
                .copied()
                .unwrap_or(GitRemoteStatus::Unknown);
            let (deployed_any, deployed_on_active) =
                if let Some(list) = app.net_template_deploys.get(&t.name) {
                    let any = !list.is_empty();
                    let on_active = active_server
                        .map(|srv| list.iter().any(|e| e.server_name == srv))
                        .unwrap_or(any);
                    (any, on_active)
                } else {
                    (false, false)
                };
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
            } else if deployed_any {
                ("deployed".to_string(), Style::default())
            } else {
                (String::new(), Style::default())
            };
            let row_style = if deployed_on_active
                || app
                    .templates_state
                    .net_template_deploy_inflight
                    .contains_key(&t.name)
            {
                Style::default()
            } else {
                bg.patch(app.theme.text_dim.to_style()).add_modifier(Modifier::DIM)
            };
            max_state = max_state.max(state.chars().count());
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_cfg { "yes" } else { "no" }),
                Cell::from(state).style(state_style),
                git_status_cell(dirty, git_status, untracked),
                Cell::from(t.desc.clone()),
            ])
            .style(row_style)
        })
        .collect();
    let state_w = max_state.clamp(10, 22) as u16;

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
            Constraint::Length(state_w),
            Constraint::Length(3),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("CFG"),
            Cell::from("STATE"),
            Cell::from("GIT"),
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

pub(in crate::ui) fn draw_shell_logs_view(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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

pub(in crate::ui) fn draw_shell_inspect_view(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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

pub(in crate::ui) fn draw_shell_help_view(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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

pub(in crate::ui) fn draw_shell_messages_view(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.overlay.to_style();
    f.render_widget(Block::default().style(bg), area);
    draw_shell_messages_list(f, app, area, bg);
}

pub(in crate::ui) fn draw_shell_messages_dock(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Dock {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);
    draw_shell_messages_list(f, app, area, bg);
}

fn draw_shell_messages_list(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    bg: Style,
) {
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

    let lnw = if app.logs.show_line_numbers {
        total_msgs.max(1).to_string().len()
    } else {
        0
    };

    // Clamp horizontal scroll to the selected message width.
    if let Some(m) = app.session_msgs.get(cursor) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let num_w = if app.logs.show_line_numbers {
            lnw + 1
        } else {
            0
        };
        let fixed_len = num_w + format!("{ts} {lvl} ").chars().count();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let max_h = m.text.chars().count().saturating_sub(msg_w);
        app.shell_msgs.hscroll = app.shell_msgs.hscroll.min(max_h);
    } else {
        app.shell_msgs.hscroll = 0;
    }

    let mut items: Vec<ListItem> = Vec::new();
    for (idx, m) in app
        .session_msgs
        .iter()
        .enumerate()
        .skip(top)
        .take(view_h)
    {
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
        let mut spans: Vec<Span<'static>> = Vec::new();
        if app.logs.show_line_numbers {
            let ln = format!("{:>lnw$} ", idx + 1);
            spans.push(Span::styled(ln, ts_style));
        }
        spans.push(Span::styled(ts, ts_style));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(lvl.to_string(), lvl_style));
        spans.push(Span::raw(" "));
        let fixed_len = spans.iter().map(|s| s.content.chars().count()).sum::<usize>();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let msg = window_hscroll(&m.text, app.shell_msgs.hscroll, msg_w);
        spans.push(Span::styled(msg, bg));
        items.push(ListItem::new(Line::from(spans)));
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
