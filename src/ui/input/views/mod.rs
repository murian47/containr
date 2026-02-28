mod main_views;
mod overlays;
mod registries;
mod sidebar;
mod templates;

use super::context::InputCtx;
use crate::ui::core::view::shell_module_shortcut;
use crate::ui::render::sidebar::shell_sidebar_select_item;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellSidebarItem, ShellView};
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_view_navigation(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) {
    if key.modifiers.is_empty()
        && let KeyCode::Char(mut ch) = key.code
    {
        for (i, hint) in app.shell_server_shortcuts.iter().copied().enumerate() {
            if hint == '\0' {
                continue;
            }
            if hint.is_ascii_alphabetic() {
                ch = ch.to_ascii_uppercase();
            }
            if ch == hint {
                app.switch_server(
                    i,
                    ctx.conn_tx,
                    ctx.refresh_tx,
                    ctx.dash_refresh_tx,
                    ctx.dash_all_enabled_tx,
                );
                return;
            }
        }
        if !matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
            let ch_lc = ch.to_ascii_lowercase();
            for v in [
                ShellView::Dashboard,
                ShellView::Stacks,
                ShellView::Containers,
                ShellView::Images,
                ShellView::Volumes,
                ShellView::Networks,
                ShellView::Templates,
                ShellView::Registries,
            ] {
                if ch_lc == shell_module_shortcut(v) {
                    app.set_main_view(v);
                    shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                    return;
                }
            }
        }
    }

    if app.shell_focus == ShellFocus::Sidebar {
        sidebar::handle_sidebar_navigation(app, key, ctx);
        return;
    }

    match app.shell_view {
        ShellView::Dashboard => {}
        ShellView::Stacks
        | ShellView::Containers
        | ShellView::Images
        | ShellView::Volumes
        | ShellView::Networks => main_views::handle_main_view_navigation(app, key),
        ShellView::Templates => templates::handle_templates_navigation(app, key),
        ShellView::Registries => registries::handle_registries_navigation(app, key),
        ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages => {
            overlays::handle_overlay_navigation(app, key)
        }
        ShellView::ThemeSelector => {}
    }
}
