use crate::ui::commands;
use crate::ui::core::requests::{ActionRequest, Connection};
use crate::ui::core::types::InspectTarget;
use crate::ui::state::app::App;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub(super) struct InputCtx<'a> {
    pub(super) conn_tx: &'a watch::Sender<Connection>,
    pub(super) refresh_tx: &'a mpsc::UnboundedSender<()>,
    pub(super) dash_refresh_tx: &'a mpsc::UnboundedSender<()>,
    pub(super) dash_all_refresh_tx: &'a mpsc::UnboundedSender<()>,
    pub(super) dash_all_enabled_tx: &'a watch::Sender<bool>,
    pub(super) refresh_interval_tx: &'a watch::Sender<Duration>,
    pub(super) refresh_pause_tx: &'a watch::Sender<bool>,
    pub(super) image_update_limit_tx: &'a watch::Sender<usize>,
    pub(super) inspect_req_tx: &'a mpsc::UnboundedSender<InspectTarget>,
    pub(super) logs_req_tx: &'a mpsc::UnboundedSender<(String, usize)>,
    pub(super) action_req_tx: &'a mpsc::UnboundedSender<ActionRequest>,
}

impl<'a> InputCtx<'a> {
    pub(super) fn execute_cmdline(&self, app: &mut App, cmdline: &str) {
        commands::cmdline_cmd::execute_cmdline(
            app,
            cmdline,
            self.conn_tx,
            self.refresh_tx,
            self.dash_refresh_tx,
            self.dash_all_refresh_tx,
            self.dash_all_enabled_tx,
            self.refresh_interval_tx,
            self.refresh_pause_tx,
            self.image_update_limit_tx,
            self.inspect_req_tx,
            self.logs_req_tx,
            self.action_req_tx,
        );
    }
}
