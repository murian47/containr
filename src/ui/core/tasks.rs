use tokio::task::JoinHandle;

pub(in crate::ui) struct BackgroundTasks {
    pub(in crate::ui) fetch_task: JoinHandle<()>,
    pub(in crate::ui) dash_task: JoinHandle<()>,
    pub(in crate::ui) dash_all_task: JoinHandle<()>,
    pub(in crate::ui) inspect_task: JoinHandle<()>,
    pub(in crate::ui) action_task: JoinHandle<()>,
    pub(in crate::ui) image_update_task: JoinHandle<()>,
    pub(in crate::ui) logs_task: JoinHandle<()>,
    pub(in crate::ui) ip_task: JoinHandle<()>,
    pub(in crate::ui) usage_task: JoinHandle<()>,
}

impl BackgroundTasks {
    pub(in crate::ui) fn abort_all(self) {
        self.fetch_task.abort();
        self.dash_task.abort();
        self.dash_all_task.abort();
        self.inspect_task.abort();
        self.action_task.abort();
        self.image_update_task.abort();
        self.logs_task.abort();
        self.ip_task.abort();
        self.usage_task.abort();
    }
}
