//! Layout command (`:layout ...`).

use super::common::{subcommand, warn_usage};
use super::super::{App, ShellSplitMode, ShellView};

const USAGE: &str = ":layout [horizontal|vertical|toggle]";

pub fn handle_layout(app: &mut App, args: &[&str]) -> bool {
    let sub = subcommand(args, "toggle");
    let target_view = if matches!(
        app.shell_view,
        ShellView::Inspect
            | ShellView::Logs
            | ShellView::Help
            | ShellView::Messages
    ) {
        app.shell_last_main_view
    } else {
        app.shell_view
    };

    match sub.to_ascii_lowercase().as_str() {
        "h" | "hor" | "horizontal" => app.shell_split_mode = ShellSplitMode::Horizontal,
        "v" | "ver" | "vertical" => app.shell_split_mode = ShellSplitMode::Vertical,
        "toggle" => {
            app.shell_split_mode = match app.shell_split_mode {
                ShellSplitMode::Horizontal => ShellSplitMode::Vertical,
                ShellSplitMode::Vertical => ShellSplitMode::Horizontal,
            }
        }
        _ => {
            warn_usage(app, USAGE);
            return true;
        }
    }
    app.set_view_split_mode(target_view, app.shell_split_mode);
    app.persist_config();
    app.set_info(format!(
        "layout: {}",
        match app.shell_split_mode {
            ShellSplitMode::Horizontal => "horizontal",
            ShellSplitMode::Vertical => "vertical",
        }
    ));
    true
}
