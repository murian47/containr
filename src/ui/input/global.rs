use super::context::InputCtx;
use crate::ui::core::view::shell_cycle_focus;
use crate::ui::core::key_types::{
    BindingHit, KeyScope, key_spec_from_event, lookup_binding, lookup_scoped_binding,
};
use crate::ui::core::keymap::is_single_letter_without_modifiers;
use crate::ui::core::types::LogsMode;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellView};
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_scoped_bindings(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) -> bool {
    let Some(spec) = key_spec_from_event(key) else {
        return false;
    };
    if app.shell_focus == ShellFocus::Sidebar && is_single_letter_without_modifiers(spec) {
        return false;
    }
    let Some(hit) = lookup_scoped_binding(app, spec) else {
        return false;
    };
    match hit {
        BindingHit::Disabled => true,
        BindingHit::Cmd(cmd) => {
            ctx.execute_cmdline(app, &cmd);
            true
        }
    }
}

pub(super) fn handle_dock_navigation(app: &mut App, key: KeyEvent) -> bool {
    if !app.log_dock_enabled
        || app.shell_focus != ShellFocus::Dock
        || matches!(
            app.shell_view,
            ShellView::Logs
                | ShellView::Inspect
                | ShellView::Help
                | ShellView::Messages
                | ShellView::ThemeSelector
        )
    {
        return false;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(1),
        KeyCode::PageUp => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(10),
        KeyCode::PageDown => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(10),
        KeyCode::Left => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_sub(4),
        KeyCode::Right => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_add(4),
        KeyCode::Home => app.shell_msgs.scroll = 0,
        KeyCode::End => app.shell_msgs.scroll = usize::MAX,
        KeyCode::Tab => return false,
        _ => {}
    }
    true
}

pub(super) fn handle_global_keys(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Tab => {
            shell_cycle_focus(app);
            true
        }
        KeyCode::Char(':') if key.modifiers.is_empty() => {
            match app.shell_view {
                ShellView::Logs => {
                    app.logs.mode = LogsMode::Command;
                    app.logs.command.clear();
                    app.logs.command_cursor = 0;
                    app.logs_rebuild_matches();
                }
                ShellView::Inspect => app.inspect_enter_command(),
                _ => {
                    app.shell_cmdline.mode = true;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.confirm = None;
                }
            }
            true
        }
        KeyCode::Char('q') if key.modifiers.is_empty() => {
            app.back_from_full_view();
            true
        }
        _ => false,
    }
}

pub(super) fn handle_always_bindings(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) -> bool {
    let Some(spec) = key_spec_from_event(key) else {
        return false;
    };
    let Some(hit) = lookup_binding(app, KeyScope::Always, spec) else {
        return false;
    };
    match hit {
        BindingHit::Disabled => true,
        BindingHit::Cmd(cmd) => {
            ctx.execute_cmdline(app, &cmd);
            true
        }
    }
}
