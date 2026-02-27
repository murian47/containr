//! Volume commands (`:volume ...` / `:vol ...`).

use super::common::{force_or_confirm, subcommand, warn_usage};
use super::super::App;
use tokio::sync::mpsc;

const USAGE: &str = ":volume rm";

pub(in crate::ui) fn handle_volume(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<super::super::ActionRequest>,
) -> bool {
    let sub = subcommand(args, "");
    match sub {
        "rm" | "remove" | "delete" => {
            force_or_confirm(app, force, "volume rm", cmdline_full, |app| {
                crate::ui::state::actions::exec_volume_remove(app, action_req_tx);
            });
        }
        _ => warn_usage(app, USAGE),
    }
    true
}
