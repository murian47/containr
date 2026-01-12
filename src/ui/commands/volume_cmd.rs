//! Volume commands (`:volume ...` / `:vol ...`).

use super::super::App;
use super::super::shell_begin_confirm;
use tokio::sync::mpsc;

pub fn handle_volume(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<super::super::ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "rm" | "remove" | "delete" => {
            if force {
                crate::ui::state::actions::exec_volume_remove(app, action_req_tx);
            } else {
                shell_begin_confirm(app, "volume rm", cmdline_full);
            }
        }
        _ => app.set_warn("usage: :volume rm"),
    }
    true
}
