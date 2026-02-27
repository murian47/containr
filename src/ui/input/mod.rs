//! Key handling / input dispatch.

mod cmdline;
mod context;
mod global;
mod modes;
mod navigation;
mod views;

use crate::ui::core::requests::{ActionRequest, Connection};
use crate::ui::core::types::InspectTarget;
use crate::ui::state::app::App;
use crossterm::event::KeyEvent;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub(in crate::ui) fn handle_shell_key(
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
    navigation::handle_shell_key_impl(
        app,
        key,
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
    );
}
