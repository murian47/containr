//! Layout command (`:layout ...`).

use super::super::{App, ShellSplitMode, ShellView};

pub fn handle_layout(app: &mut App, args: &[&str]) -> bool {
    let sub = args.first().copied().unwrap_or("toggle");
    let target_view = if matches!(
        app.shell_view,
        ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages
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
            app.set_warn("usage: :layout [horizontal|vertical|toggle]");
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
