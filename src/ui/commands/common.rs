//! Shared helpers for command handlers.

use crate::ui::state::app::App;
use crate::ui::state::shell_types::shell_begin_confirm;

pub(super) fn subcommand<'a>(args: &'a [&'a str], default: &'a str) -> &'a str {
    args.first().copied().unwrap_or(default)
}

pub(super) fn warn_usage(app: &mut App, usage: &str) {
    app.set_warn(format!("usage: {usage}"));
}

pub(super) fn force_or_confirm<F>(
    app: &mut App,
    force: bool,
    label: &str,
    cmdline_full: String,
    force_exec: F,
) where
    F: FnOnce(&mut App),
{
    if force {
        force_exec(app);
    } else {
        shell_begin_confirm(app, label, cmdline_full);
    }
}
