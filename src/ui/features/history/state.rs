use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellView, TemplatesKind};

impl App {
    pub(in crate::ui) fn open_deploy_history_for_current(&mut self) {
        match self.shell_view {
            ShellView::Stacks => self.open_deploy_history_for_selected_stack(),
            ShellView::Templates => match self.templates_state.kind {
                TemplatesKind::Stacks => self.open_deploy_history_for_selected_template(),
                TemplatesKind::Networks => self.open_deploy_history_for_selected_net_template(),
            },
            ShellView::History => {}
            _ => self.set_warn("history is available in stacks/templates views"),
        }
    }

    pub(in crate::ui) fn history_move_up(&mut self, by: usize) {
        self.deploy_history.selected = self.deploy_history.selected.saturating_sub(by);
    }

    pub(in crate::ui) fn history_move_down(&mut self, by: usize) {
        if self.deploy_history.entries.is_empty() {
            self.deploy_history.selected = 0;
            return;
        }
        self.deploy_history.selected = self
            .deploy_history
            .selected
            .saturating_add(by)
            .min(self.deploy_history.entries.len() - 1);
    }

    fn open_deploy_history_for_selected_stack(&mut self) {
        let Some(stack) = self.selected_stack_entry().map(|s| s.name.clone()) else {
            self.set_warn("no stack selected");
            return;
        };
        let Some(template_id) = self.stack_template_id(&stack) else {
            self.set_warn(format!("stack '{stack}' has no linked template id"));
            return;
        };
        let mut entries = self
            .template_deploys
            .get(&template_id)
            .cloned()
            .unwrap_or_default();
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.open_history_view(
            format!("Stack deploy history: {stack}"),
            format!("template_id={template_id}"),
            entries,
        );
    }

    fn open_deploy_history_for_selected_template(&mut self) {
        let Some(template) = self.selected_template().cloned() else {
            self.set_warn("no template selected");
            return;
        };
        let Some(template_id) = template.template_id else {
            self.set_warn(format!("template '{}' has no template id", template.name));
            return;
        };
        let mut entries = self
            .template_deploys
            .get(&template_id)
            .cloned()
            .unwrap_or_default();
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.open_history_view(
            format!("Template deploy history: {}", template.name),
            format!("template_id={template_id}"),
            entries,
        );
    }

    fn open_deploy_history_for_selected_net_template(&mut self) {
        let Some(template) = self.selected_net_template().cloned() else {
            self.set_warn("no network template selected");
            return;
        };
        let mut entries = self
            .net_template_deploys
            .get(&template.name)
            .cloned()
            .unwrap_or_default();
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.open_history_view(
            format!("Network template deploy history: {}", template.name),
            format!("network_template={}", template.name),
            entries,
        );
    }

    fn open_history_view(
        &mut self,
        title: String,
        source: String,
        entries: Vec<crate::ui::core::types::TemplateDeployEntry>,
    ) {
        self.deploy_history.return_view = self.shell_view;
        self.deploy_history.title = title;
        self.deploy_history.source = source;
        self.deploy_history.entries = entries;
        self.deploy_history.selected = 0;
        self.deploy_history.scroll_top = 0;
        self.shell_view = ShellView::History;
        self.shell_focus = ShellFocus::List;
    }
}
