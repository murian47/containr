//! Network commands (`:network ...` / `:net ...`).

use super::super::shell_begin_confirm;
use super::super::App;
use tokio::sync::mpsc;

pub fn handle_network(
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
                super::super::shell_exec_network_remove(app, action_req_tx);
            } else {
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
                shell_begin_confirm(app, "network rm", cmdline_full);
            }
        }
        _ => app.set_warn("usage: :network rm"),
    }
    true
}

