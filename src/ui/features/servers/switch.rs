use tokio::sync::{mpsc, watch};

use crate::runner::Runner;
use crate::ssh::Ssh;

use crate::docker::DockerCfg;
use crate::ui::core::requests::Connection;
use crate::ui::core::types::DashboardHostState;
use crate::ui::render::sidebar::shell_sidebar_select_item;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellSidebarItem, ShellView};

impl App {
    pub(in crate::ui) fn switch_server(
        &mut self,
        idx: usize,
        conn_tx: &watch::Sender<Connection>,
        refresh_tx: &mpsc::UnboundedSender<()>,
        dash_refresh_tx: &mpsc::UnboundedSender<()>,
        dash_all_enabled_tx: &watch::Sender<bool>,
    ) {
        let Some(s) = self.servers.get(idx).cloned() else {
            return;
        };
        self.server_selected = idx;
        self.server_all_selected = false;
        self.active_server = Some(s.name.clone());
        self.clear_all_marks();
        self.action_inflight.clear();
        self.image_action_inflight.clear();
        self.volume_action_inflight.clear();
        self.network_action_inflight.clear();
        self.stack_update_inflight.clear();
        self.stack_update_error.clear();
        self.stack_update_containers.clear();

        let runner = if s.target == "local" {
            Runner::Local
        } else {
            Runner::Ssh(Ssh {
                target: s.target.clone(),
                identity: s.identity.clone(),
                port: s.port,
            })
        };
        self.current_target = runner.key();
        self.clear_conn_error();
        self.start_loading(true);
        self.dashboard.loading = true;
        self.dashboard.error = None;
        self.dashboard.snap = None;
        self.reset_dashboard_image();
        self.dashboard.last_disk_count = self
            .dashboard
            .snap
            .as_ref()
            .map(|s| s.disks.len())
            .unwrap_or(0);
        let _ = conn_tx.send(Connection {
            runner,
            docker: DockerCfg {
                docker_cmd: s.docker_cmd,
            },
        });
        let _ = dash_all_enabled_tx.send(false);

        // Persist last_server only; no secrets stored.
        self.persist_config();
        let _ = refresh_tx.send(());
        let _ = dash_refresh_tx.send(());

        self.set_main_view(ShellView::Dashboard);
        shell_sidebar_select_item(self, ShellSidebarItem::Server(idx));
    }

    pub(in crate::ui) fn switch_server_all(
        &mut self,
        dash_all_enabled_tx: &watch::Sender<bool>,
        dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    ) {
        if self.servers.len() <= 1 {
            return;
        }
        self.server_all_selected = true;
        self.active_server = None;
        self.current_target.clear();
        self.clear_conn_error();
        self.dashboard.loading = false;
        let mut hosts: Vec<DashboardHostState> = Vec::new();
        for s in &self.servers {
            if let Some(existing) = self.dashboard_all.hosts.iter().find(|h| h.name == s.name) {
                let mut h = existing.clone();
                h.loading = true;
                h.error = None;
                hosts.push(h);
            } else {
                hosts.push(DashboardHostState {
                    name: s.name.clone(),
                    loading: true,
                    error: None,
                    snap: None,
                    latency_ms: None,
                });
            }
        }
        self.dashboard_all.hosts = hosts;
        let _ = dash_all_enabled_tx.send(true);
        let _ = dash_all_refresh_tx.send(());
        self.set_main_view(ShellView::Dashboard);
    }
}
