use super::super::context::InputCtx;
use crate::ui::render::sidebar::{
    shell_move_sidebar, shell_sidebar_items, shell_sidebar_select_item,
};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellSidebarItem, ShellView};
use crate::ui::ui_actions;
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_sidebar_navigation(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) {
    match key.code {
        KeyCode::Up => shell_move_sidebar(app, -1),
        KeyCode::Down => shell_move_sidebar(app, 1),
        KeyCode::Enter => {
            let items = shell_sidebar_items(app);
            let Some(it) = items.get(app.shell_sidebar_selected).copied() else {
                return;
            };
            match it {
                ShellSidebarItem::Server(i) => app.switch_server(
                    i,
                    ctx.conn_tx,
                    ctx.refresh_tx,
                    ctx.dash_refresh_tx,
                    ctx.dash_all_enabled_tx,
                ),
                ShellSidebarItem::Module(v) => match v {
                    ShellView::Inspect => app.enter_inspect(ctx.inspect_req_tx),
                    ShellView::Logs => app.enter_logs(ctx.logs_req_tx),
                    _ => {
                        app.set_main_view(v);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                        app.shell_focus = ShellFocus::Sidebar;
                    }
                },
                ShellSidebarItem::Action(a) => ui_actions::execute_action(
                    app,
                    a,
                    ctx.inspect_req_tx,
                    ctx.logs_req_tx,
                    ctx.action_req_tx,
                ),
                ShellSidebarItem::Separator | ShellSidebarItem::Gap => {}
            }
        }
        _ => {}
    }
}
