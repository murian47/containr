//! Sidebar command (`:sidebar ...`).

use super::super::{App, ShellFocus};

pub fn handle_sidebar(app: &mut App, args: &[&str]) -> bool {
    let sub = args.first().copied().unwrap_or("toggle");
    match sub.to_ascii_lowercase().as_str() {
        "toggle" => {
            app.shell_sidebar_hidden = !app.shell_sidebar_hidden;
            if app.shell_sidebar_hidden && app.shell_focus == ShellFocus::Sidebar {
                app.shell_focus = ShellFocus::List;
            }
        }
        "compact" => app.shell_sidebar_collapsed = !app.shell_sidebar_collapsed,
        _ => app.set_warn("usage: :sidebar toggle|compact"),
    }
    true
}
