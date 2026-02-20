//! Server commands (`:server ...`).

use super::super::{
    App, Connection, ShellFocus, ShellInteractive, ShellView, build_server_shortcuts,
    ensure_unique_server_name, find_server_by_name, parse_kv_args, shell_begin_confirm,
    shell_escape_sh_arg,
};
use crate::config::ServerEntry;
use std::fmt::Write as _;
use tokio::sync::{mpsc, watch};

pub fn handle_server(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "list" => {
            if app.servers.is_empty() {
                app.set_info("no servers configured");
            } else {
                let lines: Vec<String> = app
                    .servers
                    .iter()
                    .map(|s| format!("server '{}' -> {} (cmd={})", s.name, s.target, s.docker_cmd))
                    .collect();
                for line in lines {
                    app.set_info(line);
                }
            }
            app.shell_msgs.return_view = app.shell_view;
            app.shell_view = ShellView::Messages;
            app.shell_focus = ShellFocus::List;
            app.shell_msgs.scroll = usize::MAX;
            true
        }
        "shell" => {
            let name = args.get(1).copied();
            let mut target: Option<String> = None;
            let mut port: Option<u16> = None;
            let mut identity: Option<String> = None;

            if let Some(name) = name {
                let Some(idx) = find_server_by_name(&app.servers, name) else {
                    app.set_warn(format!("unknown server: {name}"));
                    return true;
                };
                if let Some(s) = app.servers.get(idx) {
                    target = Some(s.target.clone());
                    port = s.port;
                    identity = s.identity.clone();
                }
            } else if let Some(active) = &app.active_server {
                if let Some(idx) = find_server_by_name(&app.servers, active) {
                    if let Some(s) = app.servers.get(idx) {
                        target = Some(s.target.clone());
                        port = s.port;
                        identity = s.identity.clone();
                    }
                }
            }

            let target = target.unwrap_or_else(|| app.current_target.clone());
            if target.is_empty() {
                app.set_warn("no active server");
                return true;
            }

            if target == "local" {
                let shell = std::env::var("SHELL")
                    .ok()
                    .and_then(|s| if s.trim().is_empty() { None } else { Some(s) })
                    .unwrap_or_else(|| "sh".to_string());
                app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand {
                    cmd: shell_escape_sh_arg(&shell),
                });
                return true;
            }

            let mut cmd = String::from("ssh -t");
            if let Some(p) = port {
                let _ = write!(cmd, " -p {p}");
            }
            if let Some(identity) = identity {
                if !identity.trim().is_empty() {
                    let _ = write!(cmd, " -i {}", shell_escape_sh_arg(identity.trim()));
                }
            }
            let _ = write!(cmd, " {}", shell_escape_sh_arg(target.trim()));
            app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
            true
        }
        "use" => {
            let Some(name) = args.get(1).copied() else {
                app.set_warn("usage: :server use <name>");
                return true;
            };
            let Some(idx) = find_server_by_name(&app.servers, name) else {
                app.set_warn(format!("unknown server: {name}"));
                return true;
            };
            app.switch_server(
                idx,
                conn_tx,
                refresh_tx,
                dash_refresh_tx,
                dash_all_enabled_tx,
            );
            true
        }
        "rm" => {
            let Some(name) = args.get(1).copied() else {
                app.set_warn("usage: :server rm <name>");
                return true;
            };
            if !force {
                shell_begin_confirm(app, format!("server rm {name}"), cmdline_full);
                return true;
            }
            let Some(idx) = find_server_by_name(&app.servers, name) else {
                app.set_warn(format!("unknown server: {name}"));
                return true;
            };
            let removed_active = app.active_server.as_deref() == Some(name);
            app.servers.remove(idx);
            app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
            if removed_active {
                app.active_server = None;
                app.server_selected = 0;
                if !app.servers.is_empty() {
                    app.switch_server(
                        0,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_enabled_tx,
                    );
                } else {
                    app.persist_config();
                }
            } else {
                app.server_selected = app.server_selected.min(app.servers.len().saturating_sub(1));
                app.persist_config();
            }
            true
        }
        "add" => {
            let Some(name) = args.get(1).copied() else {
                app.set_warn("usage: :server add <name> (ssh <target> | local) [opts]");
                return true;
            };
            let Some(name) = ensure_unique_server_name(&app.servers, name) else {
                app.set_warn("server name already exists");
                return true;
            };
            let Some(kind) = args.get(2).copied() else {
                app.set_warn("usage: :server add <name> (ssh <target> | local) [opts]");
                return true;
            };

            let mut rest: Vec<String> = args.iter().skip(3).map(|s| (*s).to_string()).collect();
            let (port, identity, docker_cmd, tail) = parse_kv_args(rest.drain(..).into_iter());
            let docker_cmd = docker_cmd.unwrap_or_default();

            match kind {
                "ssh" => {
                    let target = tail.get(0).cloned().unwrap_or_default();
                    if target.trim().is_empty() {
                        app.set_warn("usage: :server add <name> ssh <target> [opts]");
                        return true;
                    }
                    app.servers.push(ServerEntry {
                        name,
                        target,
                        port,
                        identity,
                        docker_cmd,
                    });
                }
                "local" => {
                    app.servers.push(ServerEntry {
                        name,
                        target: "local".to_string(),
                        port: None,
                        identity: None,
                        docker_cmd,
                    });
                }
                _ => {
                    app.set_error(
                        "usage: :server add <name> (ssh <target> | local) [opts]".to_string(),
                    );
                    return true;
                }
            }
            app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
            app.persist_config();
            true
        }
        _ => {
            app.set_error("usage: :server (list|use|add|rm|shell) ...".to_string());
            true
        }
    }
}
