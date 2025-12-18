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
    items.push(ShellSidebarItem::Module(ShellView::Containers));
    items.push(ShellSidebarItem::Module(ShellView::Images));
    items.push(ShellSidebarItem::Module(ShellView::Volumes));
    items.push(ShellSidebarItem::Module(ShellView::Networks));
    items.push(ShellSidebarItem::Module(ShellView::Inspect));
    items.push(ShellSidebarItem::Module(ShellView::Logs));
    items.push(ShellSidebarItem::Gap);
    items.push(ShellSidebarItem::Module(ShellView::Templates));
    // Help is accessible via :? / :help (not a module entry).

    let actions: Vec<ShellAction> = match app.shell_view {
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
        ShellView::Containers => ActiveView::Containers,
        ShellView::Images => ActiveView::Images,
        ShellView::Volumes => ActiveView::Volumes,
        ShellView::Networks => ActiveView::Networks,
        ShellView::Templates => app.active_view,
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
        app.shell_view = if app.shell_view == ShellView::Help {
            app.shell_help.return_view
        } else if app.shell_view == ShellView::Messages {
            app.shell_msgs.return_view
        } else {
            app.shell_last_main_view
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
    let _ = conn_tx.send(Connection {
        runner,
        docker: DockerCfg {
            docker_cmd: s.docker_cmd,
        },
    });

    // Persist last_server only; no secrets stored.
    app.persist_config();
    let _ = refresh_tx.send(());

    shell_set_main_view(app, ShellView::Containers);
    shell_sidebar_select_item(app, ShellSidebarItem::Server(idx));
}

fn shell_refresh(app: &mut App, refresh_tx: &mpsc::UnboundedSender<()>) {
    app.start_loading(true);
    let _ = refresh_tx.send(());
}

impl App {
    fn persist_config(&mut self) {
        let cfg = ContainrConfig {
            version: 9,
            last_server: self.active_server.clone(),
            refresh_secs: self.refresh_secs.max(1),
            logs_tail: self.logs.tail.max(1),
            cmd_history_max: self.cmd_history_max_effective(),
            cmd_history: self.shell_cmdline.history.entries.clone(),
            active_theme: self.theme_name.clone(),
            templates_dir: self.templates_state.dir.to_string_lossy().to_string(),
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
        };
        if let Err(e) = config::save(&self.config_path, &cfg) {
            self.set_error(format!("failed to save config: {:#}", e));
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
    format!("~/.config/containr/apps/{name}")
}

fn deploy_remote_net_dir_for(name: &str) -> String {
    format!("~/.config/containr/networks/{name}")
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

fn parse_kv_args(
    mut it: impl Iterator<Item = String>,
) -> (Option<u16>, Option<String>, Option<String>, Vec<String>) {
    // Supports: -p <port>  -i <identity>  --cmd <docker_cmd>
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut docker_cmd: Option<String> = None;
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
                    docker_cmd = Some(v);
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
    let docker_cmd = current_docker_cmd_from_app(app);
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
    refresh_interval_tx: &watch::Sender<Duration>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return;
    }
    let cmdline = cmdline.trim_start_matches(':').trim();
    let cmdline_full = cmdline.to_string();

    let mut it = cmdline.split_whitespace();
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
                ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages => {}
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
                shell_refresh(app, refresh_tx);
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
        let _ = commands::set_cmd::handle_set(app, &args, refresh_interval_tx, logs_req_tx);
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

fn shell_execute_action(
    app: &mut App,
    a: ShellAction,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    match a {
        ShellAction::Start => {
            shell_exec_container_action(app, ContainerAction::Start, action_req_tx)
        }
        ShellAction::Stop => shell_exec_container_action(app, ContainerAction::Stop, action_req_tx),
        ShellAction::Restart => {
            shell_exec_container_action(app, ContainerAction::Restart, action_req_tx)
        }
        ShellAction::Delete => {
            shell_begin_confirm(app, "container rm", "container rm");
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
                        shell_deploy_template(app, &name, action_req_tx);
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
    }
}

fn shell_deploy_template(
    app: &mut App,
    name: &str,
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
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let _ = action_req_tx.send(ActionRequest::TemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_compose: tpl.compose_path.clone(),
    });
    app.templates_state.template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.set_info(format!("deploying template {name}"));
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
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
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
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
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
    refresh_interval_tx: &watch::Sender<Duration>,
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
                        refresh_interval_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
            }
        }
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
                        refresh_interval_tx,
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
                    refresh_interval_tx,
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
                    if let Some(path) = cmdline.strip_prefix("save").map(str::trim) {
                        if path.is_empty() {
                            app.set_warn("usage: save <file>");
                        } else {
                            match app.logs.text.as_deref() {
                                None => app.set_warn("no logs loaded"),
                                Some(text) => match write_text_file(path, text) {
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
                    if let Some(path) = cmd.strip_prefix("save").map(str::trim) {
                        if path.is_empty() {
                            app.inspect.error = Some("usage: save <file>".to_string());
                        } else {
                            match app.inspect.value.as_ref() {
                                None => {
                                    app.inspect.error = Some("no inspect data loaded".to_string())
                                }
                                Some(v) => match serde_json::to_string_pretty(v) {
                                    Ok(s) => match write_text_file(path, &s) {
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

    // Custom key bindings (outside of input modes).
    if let Some(spec) = key_spec_from_event(key) {
        if let Some(hit) = lookup_scoped_binding(app, spec) {
            match hit {
                BindingHit::Disabled => return,
                BindingHit::Cmd(cmd) => {
                    shell_execute_cmdline(
                        app,
                        &cmd,
                        conn_tx,
                        refresh_tx,
                        refresh_interval_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
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
                    shell_switch_server(app, i, conn_tx, refresh_tx);
                    return;
                }
            }
            // Modules (disabled in full-screen views like Logs/Inspect to avoid conflicts with
            // in-view navigation keys like n/N, j/k, etc.).
            if !matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
                let ch_lc = ch.to_ascii_lowercase();
                for v in [
                    ShellView::Containers,
                    ShellView::Images,
                    ShellView::Volumes,
                    ShellView::Networks,
                    ShellView::Templates,
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
                    ShellSidebarItem::Server(i) => shell_switch_server(app, i, conn_tx, refresh_tx),
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
        ShellView::Containers | ShellView::Images | ShellView::Volumes | ShellView::Networks => {
            // Ensure active_view matches (used by the existing selection/mark logic).
            app.active_view = match app.shell_view {
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
                    ActiveView::Containers => app.selected = 0,
                    ActiveView::Images => app.images_selected = 0,
                    ActiveView::Volumes => app.volumes_selected = 0,
                    ActiveView::Networks => app.networks_selected = 0,
                },
                KeyCode::End => match app.active_view {
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
    let mid = format!(
        "Server: {server}  {conn} connected{err_badge}  ⟳ {}s  View: {}{crumb}{deploy}",
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
                rendered.push(ListItem::new(Line::from(Span::styled(
                    "─".repeat(inner_w),
                    app.theme.divider.to_style(),
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

    let is_full = matches!(app.shell_view, ShellView::Logs | ShellView::Inspect);
    let is_split_view = matches!(
        app.shell_view,
        ShellView::Containers
            | ShellView::Images
            | ShellView::Volumes
            | ShellView::Networks
            | ShellView::Templates
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
    let left = format!(" {title} ({count})");
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let title_area = chunks[0];
    let content_area = chunks[1];

    match app.shell_view {
        ShellView::Containers => {
            draw_shell_title(f, app, "Containers", app.containers.len(), title_area);
            draw_shell_containers_table(f, app, content_area);
        }
        ShellView::Images => {
            draw_shell_title(f, app, "Images", app.images_visible_len(), title_area);
            draw_shell_images_table(f, app, content_area);
        }
        ShellView::Volumes => {
            draw_shell_title(f, app, "Volumes", app.volumes_visible_len(), title_area);
            draw_shell_volumes_table(f, app, content_area);
        }
        ShellView::Networks => {
            draw_shell_title(f, app, "Networks", app.networks.len(), title_area);
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
            draw_shell_templates_table(f, app, content_area);
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

fn draw_shell_main_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.shell_view {
        ShellView::Containers => draw_shell_container_details(f, app, area),
        ShellView::Images => draw_shell_image_details(f, app, area),
        ShellView::Volumes => draw_shell_volume_details(f, app, area),
        ShellView::Networks => draw_shell_network_details(f, app, area),
        ShellView::Templates => draw_shell_template_details(f, app, area),
        ShellView::Logs => draw_shell_logs_meta(f, app, area),
        ShellView::Inspect => draw_shell_inspect_meta(f, app, area),
        ShellView::Help => draw_shell_help_meta(f, app, area),
        ShellView::Messages => draw_shell_messages_meta(f, app, area),
    }
}

fn draw_shell_footer(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = app.theme.footer.to_style();
    f.render_widget(Block::default().style(bg), area);

    let hint = match app.shell_view {
        ShellView::Containers => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Images | ShellView::Volumes | ShellView::Networks => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Templates => {
            " F1 help  b sidebar  ^p layout  :q quit"
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
    let line = Line::from(vec![Span::styled(
        truncate_end(hint, w),
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
        let status_style = if let Some(err) = app.container_action_error.get(&c.id) {
            match err.kind {
                ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
            }
        } else {
            row_style
        };

        let name = format!("{name_prefix}{}", c.name);
        Row::new(vec![
            Cell::from(truncate_end(&name, 22)).style(row_style),
            Cell::from(truncate_end(&c.image, 40)).style(row_style),
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
                    rows.push(
                        Row::new(vec![
                            Cell::from(format!("{glyph} {name}")).style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
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
    // instead of stretching REF to fill the entire view.
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
                size,
            ])
            .style(row_style),
        );
    }
    let ref_w = max_ref.clamp(REF_MIN_W, REF_TEXT_MAX);
    let id_w = max_id.clamp(ID_MIN_W, ID_TEXT_MAX);

    let mut state = TableState::default();
    state.select(Some(app.images_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Length(ref_w as u16),
            Constraint::Length(id_w as u16),
            Constraint::Length(SIZE_W as u16),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("REF"),
            Cell::from("ID"),
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
            } else if used == 0 {
                Cell::from("unused".to_string())
            } else {
                Cell::from(format!("{used} ctr"))
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
            Constraint::Length(10),
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
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("ID"),
            Cell::from("DRIVER"),
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
            Constraint::Length(10),
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

fn draw_shell_container_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
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
    let Some(c) = app.selected_container() else {
        f.render_widget(
            Paragraph::new("Select a container to see details.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let key = bg.patch(app.theme.text_dim.to_style());
    let val = bg;
    let kv = |k: &str, v: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("{k}: "), key),
            Span::styled(v, val),
        ])
    };
    let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
    let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
    let ip = app
        .ip_cache
        .get(&c.id)
        .map(|(ip, _)| ip.clone())
        .unwrap_or_else(|| "-".to_string());
    let mut lines = vec![
        kv("Name", c.name.clone()),
        kv("ID", c.id.clone()),
        kv("Image", c.image.clone()),
        kv("Status", c.status.clone()),
        kv("CPU / MEM", format!("{cpu} / {mem}")),
        kv("IP", ip),
        kv("Ports", c.ports.clone()),
    ];
    if let Some(err) = app.container_action_error.get(&c.id) {
        let k = bg.patch(app.theme.text_dim.to_style());
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("Last error [{}]: ", action_error_details(err)), k),
            Span::styled(err.message.clone(), v),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_image_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
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
    let Some(img) = app.selected_image() else {
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ref: ", Style::default().fg(Color::Gray)),
            Span::raw(img.name()),
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(img.id.clone()),
        ]),
        Line::from(vec![
            Span::styled("Size: ", Style::default().fg(Color::Gray)),
            Span::raw(img.size.clone()),
        ]),
    ];
    let key = App::image_row_key(img);
    if let Some(err) = app.image_action_error.get(&key) {
        let k = bg.patch(app.theme.text_dim.to_style());
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("Last error [{}]: ", action_error_details(err)), k),
            Span::styled(err.message.clone(), v),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_volume_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
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
    let Some(v) = app.selected_volume() else {
        return;
    };
    let used_by = app
        .volume_containers_by_name
        .get(&v.name)
        .map(|xs| xs.join(", "))
        .unwrap_or_else(|| "-".to_string());
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Gray)),
            Span::raw(v.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Driver: ", Style::default().fg(Color::Gray)),
            Span::raw(v.driver.clone()),
        ]),
        Line::from(vec![
            Span::styled("Used by: ", Style::default().fg(Color::Gray)),
            Span::raw(used_by),
        ]),
    ];
    if let Some(err) = app.volume_action_error.get(&v.name) {
        let k = bg.patch(app.theme.text_dim.to_style());
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("Last error [{}]: ", action_error_details(err)), k),
            Span::styled(err.message.clone(), v_style),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_network_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
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
    let Some(n) = app.selected_network() else {
        return;
    };
    let is_system = App::is_system_network(n);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Gray)),
            Span::raw(n.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Gray)),
            if is_system {
                Span::styled(
                    "System",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    "User",
                    Style::default()
                        .fg(Color::White)
                        ,
                )
            },
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(n.id.clone()),
        ]),
        Line::from(vec![
            Span::styled("Driver: ", Style::default().fg(Color::Gray)),
            Span::raw(n.driver.clone()),
        ]),
        Line::from(vec![
            Span::styled("Scope: ", Style::default().fg(Color::Gray)),
            Span::raw(n.scope.clone()),
        ]),
    ];
    if let Some(err) = app.network_action_error.get(&n.id) {
        let k = bg.patch(app.theme.text_dim.to_style());
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("Last error [{}]: ", action_error_details(err)), k),
            Span::styled(err.message.clone(), v_style),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
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

    if let Some(err) = &app.templates_state.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_template() else {
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
    let view_h = inner.height.max(1) as usize;
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

    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_state.templates_details_scroll.min(u16::MAX as usize) as u16,
            0,
        )),
        inner,
    );
}

fn draw_shell_template_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => draw_shell_stack_template_details(f, app, area),
        TemplatesKind::Networks => draw_shell_net_template_details(f, app, area),
    }
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

    if let Some(err) = &app.templates_state.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(bg.patch(app.theme.text_error.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_net_template() else {
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
    let view_h = inner.height.max(1) as usize;
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

    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_state.net_templates_details_scroll
                .min(u16::MAX as usize) as u16,
            0,
        )),
        inner,
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

fn shell_help_lines(theme: &theme::ThemeSpec) -> Vec<Line<'static>> {
    let h = |title: &str| -> Line<'static> {
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let item = |scope: &str, syntax: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("{scope:<10} "), theme.text_dim.to_style()),
            Span::styled(format!("{syntax:<22} "), Style::default().fg(Color::White)),
            Span::styled(
                desc.to_string(),
                theme.text.to_style(),
            ),
        ])
    };

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(h("General"));
    out.push(item("Always", "F1", "Open help"));
    out.push(item("Global", ":q", "Quit (prompts y/n)"));
    out.push(item("Global", ":q!", "Quit immediately (! auto-confirms)"));
    out.push(item("Global", ":! <cmd>", "Run command with auto-confirm (! modifier)"));
    out.push(item(
        "Note",
        "confirm",
        "Destructive commands prompt y/n; add ! to auto-confirm",
    ));
    out.push(item("Global", ":?", "Open help"));
    out.push(item("Global", ":help", "Open help"));
    out.push(item("Global", ":messages", "Toggle messages view (session log)"));
    out.push(item("Global", ":ack [all]", "Clear per-item action error markers"));
    out.push(item("Global", ":refresh", "Trigger immediate refresh"));
    out.push(item(
        "Global",
        ":sidebar (toggle|compact)",
        "Show/hide sidebar or compact it",
    ));
    out.push(item(
        "Global",
        ":layout [horizontal|vertical|toggle]",
        "Set list/details split for current module",
    ));
    out.push(item(
        "Note",
        "aliases",
        ":ctr, :tpl, :img, :vol, :net (logs has no alias)",
    ));
    out.push(item(
        "Global",
        ":set refresh <sec>",
        "Set refresh interval (1..3600), saved to config",
    ));
    out.push(item(
        "Global",
        ":set logtail <n>",
        "Set docker logs --tail (1..200000), saved to config",
    ));
    out.push(item(
        "Global",
        ":set history <n>",
        "Set command history size (1..5000), saved to config",
    ));
    out.push(Line::from(""));

    out.push(h("Keymap"));
    out.push(item("Note", "^x", "Means Ctrl-x (caret notation)"));
    out.push(item(
        "Keymap",
        "Scopes",
        "always, global, view:<name> (e.g. view:logs)",
    ));
    out.push(item(
        "Keymap",
        "Precedence",
        "always -> view:<current> -> global",
    ));
    out.push(item(
        "Keymap",
        "Disable",
        ":unmap inserts a disable marker that overrides defaults",
    ));
    out.push(item(
        "Global",
        ":map [scope] <KEY> <CMD...>",
        "Bind (e.g. :map always F1 :help, :map view:logs ^l :logs reload)",
    ));
    out.push(item(
        "Global",
        ":unmap [scope] <KEY>",
        "Disable binding or remove override (restore defaults)",
    ));
    out.push(item(
        "Global",
        ":map list",
        "List effective bindings (* = configured/overridden)",
    ));
    out.push(item(
        "Keymap",
        "Safety",
        "Destructive commands cannot be mapped to plain single letters",
    ));
    out.push(Line::from(""));

    out.push(h("Theme"));
    out.push(item("Global", ":theme list", "List available themes"));
    out.push(item("Global", ":theme use <name>", "Switch active theme (persisted)"));
    out.push(item("Global", ":theme new <name>", "Create a new theme from default and open $EDITOR"));
    out.push(item("Global", ":theme edit [name]", "Edit theme file via $EDITOR (creates if missing)"));
    out.push(item("Global", ":theme rm[!] <name>", "Delete theme (! skips confirmation)"));
    out.push(Line::from(""));

    out.push(h("Servers"));
    out.push(item("Global", ":server list", "List configured servers"));
    out.push(item("Global", ":server use <name>", "Switch active server"));
    out.push(item("Global", ":server rm <name>", "Remove server"));
    out.push(item(
        "Global",
        ":server add <name> ssh <target> [-p <port>] [-i <identity>] [--cmd <docker|podman>]",
        "Add SSH server entry",
    ));
    out.push(item(
        "Global",
        ":server add <name> local [--cmd <docker|podman>]",
        "Add local engine entry",
    ));
    out.push(Line::from(""));

    out.push(h("Templates"));
    out.push(item(
        "Templates",
        ":templates kind (stacks|networks|toggle)",
        "Switch between stack templates and network templates",
    ));
    out.push(item("Templates", "^t", "Toggle stacks/networks (default binding)"));
    out.push(item(
        "Templates",
        ":template/:tpl add <name>",
        "Create a new template",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl edit [name]",
        "Edit selected template (or by name)",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl rm [name]",
        "Delete selected template (or by name)",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl deploy [name]",
        "Deploy selected template (or by name) to active server",
    ));
    out.push(Line::from(""));
    out.push(item(
        "Templates",
        ":nettemplate/:nt deploy[!] [name]",
        "Create network on active server (! = recreate if already exists)",
    ));
    out.push(Line::from(""));

    out.push(h("Containers"));
    out.push(item(
        "Containers",
        ":container/:ctr (start|stop|restart|rm)",
        "Run action for selection/marks/stack",
    ));
    out.push(item(
        "Containers",
        ":container/:ctr console [-u USER] [bash|sh|SHELL]",
        "Open console for selected running container (default user: root)",
    ));
    out.push(item("Containers", ":container/:ctr tree", "Toggle stack (tree) view"));
    out.push(Line::from(""));

    out.push(h("Images"));
    out.push(item(
        "Images",
        ":image/:img untag",
        "Remove tag from selected/marked image",
    ));
    out.push(item("Images", ":image/:img rm", "Remove selected/marked image"));
    out.push(Line::from(""));

    out.push(h("Volumes"));
    out.push(item("Volumes", ":volume/:vol rm", "Remove selected/marked volume"));
    out.push(Line::from(""));

    out.push(h("Networks"));
    out.push(item(
        "Networks",
        ":network/:net rm",
        "Remove selected/marked network",
    ));
    out.push(item("Networks", "^d", "Remove (default binding)"));
    out.push(Line::from(""));

    out.push(h("Logs"));
    out.push(item("Logs", "^l", "Reload logs (default binding)"));
    out.push(item("Logs", "^c", "Copy selected lines to clipboard"));
    out.push(item("Logs", "m", "Toggle regex search"));
    out.push(item("Logs", "/", "Enter search mode"));
    out.push(item("Logs", ":", "Enter command mode"));
    out.push(item("Logs", "n/N", "Next/previous match"));
    out.push(item("Logs", "j/k", "Down/up"));
    out.push(item("Logs", "j <n>", "Jump to line n (1-based)"));
    out.push(item("Logs", "save <file>", "Save full logs to a file"));
    out.push(item("Logs", "set number", "Enable line numbers"));
    out.push(item("Logs", "set nonumber", "Disable line numbers"));
    out.push(item("Logs", "set regex", "Enable regex search"));
    out.push(item("Logs", "set noregex", "Disable regex search"));
    out.push(Line::from(""));

    out.push(h("Inspect"));
    out.push(item("Inspect", "/", "Enter search mode"));
    out.push(item("Inspect", ":", "Enter command mode"));
    out.push(item("Inspect", "Enter", "Expand/collapse selected node"));
    out.push(item("Inspect", "n/N", "Next/previous match"));
    out.push(item("Inspect", "expand", "Expand all"));
    out.push(item("Inspect", "collapse", "Collapse all"));
    out.push(item("Inspect", "save <file>", "Save full inspect JSON to a file"));
    out.push(item("Inspect", "y", "Copy selected value (pretty)"));
    out.push(item("Inspect", "p", "Copy selected JSON pointer path"));
    out
}

fn write_text_file(path: &str, text: &str) -> anyhow::Result<PathBuf> {
    let path = path.trim();
    anyhow::ensure!(!path.is_empty(), "missing file path");

    let path = expand_user_path(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&path, text)?;
    Ok(path)
}

fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
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
