//! Log-related commands (`:logs ...`) available from the main command line.

use super::common::{subcommand, warn_usage};
use super::super::{App, ShellView};
use tokio::sync::mpsc;

const USAGE: &str = ":logs reload|copy";

fn request_reload(app: &mut App, logs_req_tx: &mpsc::UnboundedSender<(String, usize)>) {
    if let Some(id) = app.logs.for_id.clone() {
        app.logs.loading = true;
        let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
    } else {
        app.set_warn("no logs target selected");
    }
}

pub fn handle_logs(
    app: &mut App,
    args: &[&str],
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
) -> bool {
    let sub = subcommand(args, "");
    if sub == "reload" || sub == "refresh" {
        request_reload(app, logs_req_tx);
        return true;
    }
    if sub == "copy" {
        app.logs_copy_selection();
        return true;
    }
    if sub.is_empty() && app.shell_view == ShellView::Logs {
        request_reload(app, logs_req_tx);
        return true;
    }
    warn_usage(app, USAGE);
    true
}
