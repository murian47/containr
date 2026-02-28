//! Network commands (`:network ...` / `:net ...`).

use super::common::{force_or_confirm, subcommand, warn_usage};
use crate::ui::core::requests::ActionRequest;
use crate::ui::state::app::App;
use tokio::sync::mpsc;

const USAGE: &str = ":network rm";

pub(in crate::ui) fn handle_network(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    let sub = subcommand(args, "");
    match sub {
        "rm" | "remove" | "delete" => {
            // Avoid prompting when only system networks are selected/marked.
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
                return true;
            }
            force_or_confirm(app, force, "network rm", cmdline_full, |app| {
                crate::ui::state::actions::exec_network_remove(app, action_req_tx);
            });
        }
        _ => warn_usage(app, USAGE),
    }
    true
}
