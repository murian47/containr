//! Dashboard command handler (`:dashboard ...`).

use super::super::{App, Connection};
use tokio::sync::{mpsc, watch};

pub fn handle_dashboard(
    app: &mut App,
    args: &[&str],
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
) -> bool {
    let mode = args.first().copied().unwrap_or("toggle");
    match mode {
        "all" => {
            app.switch_server_all(dash_all_enabled_tx, dash_all_refresh_tx);
        }
        "single" | "server" => {
            if !app.servers.is_empty() {
                let idx = app.server_selected.min(app.servers.len().saturating_sub(1));
                app.switch_server(
                    idx,
                    conn_tx,
                    refresh_tx,
                    dash_refresh_tx,
                    dash_all_enabled_tx,
                );
            }
        }
        "simulate-error" => {
            let target = args.get(1).copied();
            let mut applied = false;
            for host in &mut app.dashboard_all.hosts {
                if target.is_none() || target == Some(host.name.as_str()) {
                    host.error = Some("simulated error".to_string());
                    host.loading = false;
                    applied = true;
                    if target.is_some() {
                        break;
                    }
                }
            }
            if !applied {
                app.set_warn("dashboard simulate-error: no matching host");
            }
        }
        "toggle" => {
            if app.server_all_selected {
                if !app.servers.is_empty() {
                    let idx = app.server_selected.min(app.servers.len().saturating_sub(1));
                    app.switch_server(
                        idx,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_enabled_tx,
                    );
                }
            } else {
                app.switch_server_all(dash_all_enabled_tx, dash_all_refresh_tx);
            }
        }
        _ => {
            app.set_warn("usage: :dashboard (all|single|toggle|simulate-error [name])");
        }
    }
    true
}
