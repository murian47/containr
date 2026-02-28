//! Sidebar command (`:sidebar ...`).

use super::common::{subcommand, warn_usage};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellView};

const USAGE: &str = ":sidebar toggle|compact";

pub(in crate::ui) fn handle_sidebar(app: &mut App, args: &[&str]) -> bool {
    let sub = subcommand(args, "toggle");
    match sub.to_ascii_lowercase().as_str() {
        "toggle" => {
            app.shell_sidebar_hidden = !app.shell_sidebar_hidden;
            if app.shell_sidebar_hidden && app.shell_focus == ShellFocus::Sidebar {
                app.shell_focus = ShellFocus::List;
            }
            if app.shell_view == ShellView::Dashboard {
                app.dashboard.suppress_image_frames = 2;
                app.reset_dashboard_image();
            }
        }
        "compact" => app.shell_sidebar_collapsed = !app.shell_sidebar_collapsed,
        _ => warn_usage(app, USAGE),
    }
    true
}
