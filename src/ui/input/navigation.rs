//! Top-level key dispatch for shell mode.

use super::cmdline::handle_cmdline_mode;
use super::context::InputCtx;
use super::global::{
    handle_always_bindings, handle_dock_navigation, handle_global_keys, handle_scoped_bindings,
};
use super::modes::handle_view_input_modes;
use super::views::handle_view_navigation;
use crate::ui::core::requests::{ActionRequest, Connection};
use crate::ui::core::types::InspectTarget;
use crate::ui::state::app::App;
use crossterm::event::{KeyCode, KeyEvent};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_shell_key_impl(
    app: &mut App,
    key: KeyEvent,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
    refresh_interval_tx: &watch::Sender<Duration>,
    refresh_pause_tx: &watch::Sender<bool>,
    image_update_limit_tx: &watch::Sender<usize>,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let ctx = InputCtx {
        conn_tx,
        refresh_tx,
        dash_refresh_tx,
        dash_all_refresh_tx,
        dash_all_enabled_tx,
        refresh_interval_tx,
        refresh_pause_tx,
        image_update_limit_tx,
        inspect_req_tx,
        logs_req_tx,
        action_req_tx,
    };

    if handle_always_bindings(app, key, &ctx) {
        return;
    }

    if app.refresh_paused
        && key.modifiers.is_empty()
        && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
    {
        app.refresh_now(
            ctx.refresh_tx,
            ctx.dash_refresh_tx,
            ctx.dash_all_refresh_tx,
            ctx.refresh_pause_tx,
        );
        return;
    }

    if handle_cmdline_mode(app, key, &ctx) {
        return;
    }

    if handle_view_input_modes(app, key, &ctx) {
        return;
    }

    if handle_scoped_bindings(app, key, &ctx) {
        return;
    }

    if handle_dock_navigation(app, key) {
        return;
    }

    if handle_global_keys(app, key) {
        return;
    }

    handle_view_navigation(app, key, &ctx);
}
