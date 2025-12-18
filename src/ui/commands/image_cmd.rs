//! Image commands (`:image ...` / `:img ...`).

use super::super::App;
use super::super::shell_begin_confirm;
use tokio::sync::mpsc;

pub fn handle_image(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<super::super::ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "untag" => {
            if force {
                super::super::shell_exec_image_action(app, true, action_req_tx);
            } else {
                shell_begin_confirm(app, "image untag", cmdline_full);
            }
        }
        "rm" | "remove" | "delete" => {
            if force {
                super::super::shell_exec_image_action(app, false, action_req_tx);
            } else {
                shell_begin_confirm(app, "image rm", cmdline_full);
            }
        }
        _ => app.set_warn("usage: :image untag | :image rm"),
    }
    true
}
