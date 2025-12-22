//! Settings commands (`:set ...`).

use super::super::{App, ShellView};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

fn parse_toggle(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

pub fn handle_set(
    app: &mut App,
    args: &[&str],
    refresh_interval_tx: &watch::Sender<Duration>,
    image_update_limit_tx: &watch::Sender<usize>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    let rest = &args.get(1..).unwrap_or(&[]);

    match sub {
        "refresh" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set refresh <seconds>");
                return true;
            };
            match v.parse::<u64>() {
                Ok(secs) if secs >= 1 && secs <= 3600 => {
                    app.refresh_secs = secs;
                    let _ = refresh_interval_tx.send(Duration::from_secs(secs));
                    app.persist_config();
                }
                _ => app.set_warn("refresh must be 1..3600"),
            }
            true
        }
        "logtail" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set logtail <lines>");
                return true;
            };
            match v.parse::<usize>() {
                Ok(n) if (1..=200_000).contains(&n) => {
                    app.logs.tail = n;
                    app.persist_config();
                    if app.shell_view == ShellView::Logs {
                        if let Some(id) = app.logs.for_id.clone() {
                            app.logs.loading = true;
                            let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
                        }
                    }
                }
                _ => app.set_warn("logtail must be 1..200000"),
            }
            true
        }
        "history" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set history <entries>");
                return true;
            };
            match v.parse::<usize>() {
                Ok(n) if (1..=5000).contains(&n) => {
                    app.cmd_history_max = n;
                    let entries = app.shell_cmdline.history.entries.clone();
                    app.set_cmd_history_entries(entries);
                    app.persist_config();
                }
                _ => app.set_warn("history must be 1..5000"),
            }
            true
        }
        "git_autocommit" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set git_autocommit <on|off>");
                return true;
            };
            match parse_toggle(v) {
                Some(flag) => {
                    app.git_autocommit = flag;
                    app.persist_config();
                }
                None => app.set_warn("git_autocommit must be on/off"),
            }
            true
        }
        "git_autocommit_confirm" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set git_autocommit_confirm <on|off>");
                return true;
            };
            match parse_toggle(v) {
                Some(flag) => {
                    app.git_autocommit_confirm = flag;
                    app.persist_config();
                }
                None => app.set_warn("git_autocommit_confirm must be on/off"),
            }
            true
        }
        "editor" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set editor <command> | :set editor reset");
                return true;
            };
            if matches!(v, "reset" | "default" | "unset" | "clear") {
                app.editor_cmd.clear();
                app.persist_config();
                return true;
            }
            let cmd = rest.join(" ").trim().to_string();
            if cmd.is_empty() {
                app.set_warn("editor command must not be empty");
                return true;
            }
            app.editor_cmd = cmd;
            app.persist_config();
            true
        }
        "image_update_concurrency" => {
            let Some(v) = rest.first().copied() else {
                app.set_warn("usage: :set image_update_concurrency <n>");
                return true;
            };
            match v.parse::<usize>() {
                Ok(n) if (1..=32).contains(&n) => {
                    app.image_update_concurrency = n;
                    let _ = image_update_limit_tx.send(n);
                    app.persist_config();
                }
                _ => app.set_warn("image_update_concurrency must be 1..32"),
            }
            true
        }
        _ => {
            app.set_warn(
                "usage: :set refresh <seconds> | :set logtail <lines> | :set history <entries> | :set editor <command> | :set git_autocommit <on|off> | :set git_autocommit_confirm <on|off> | :set image_update_concurrency <n>",
            );
            true
        }
    }
}
