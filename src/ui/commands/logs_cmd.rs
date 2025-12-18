//! Log-related commands (`:logs ...`) available from the main command line.

use super::super::{App, ShellView};
use tokio::sync::mpsc;

pub fn handle_logs(
    app: &mut App,
    args: &[&str],
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    if sub == "reload" || sub == "refresh" {
        if let Some(id) = app.logs.for_id.clone() {
            app.logs.loading = true;
            let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
        } else {
            app.set_warn("no logs target selected");
        }
        return true;
    }
    if sub == "copy" {
        app.logs_copy_selection();
        return true;
    }
    if sub.is_empty() && app.shell_view == ShellView::Logs {
        if let Some(id) = app.logs.for_id.clone() {
            app.logs.loading = true;
            let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
        } else {
            app.set_warn("no logs target selected");
        }
        return true;
    }
    app.set_warn("usage: :logs reload|copy");
    true
}

