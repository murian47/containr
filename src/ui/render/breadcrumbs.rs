use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::{App, ShellView, TemplatesKind};

pub(crate) fn shell_breadcrumbs(app: &App) -> String {
    match app.shell_view {
        ShellView::Dashboard => String::new(),
        ShellView::Stacks => app
            .selected_stack_entry()
            .map(|s| format!("/{}", s.name))
            .unwrap_or_default(),
        ShellView::Containers => {
            if let Some((name, ..)) = app.selected_stack() {
                return format!("/{name}");
            }
            if let Some(c) = app.selected_container() {
                if let Some(stack) = stack_name_from_labels(&c.labels) {
                    format!("/{stack}/{}", c.name)
                } else {
                    format!("/{}", c.name)
                }
            } else {
                String::new()
            }
        }
        ShellView::Images => app
            .selected_image()
            .map(|i| format!("/{}", i.name()))
            .unwrap_or_default(),
        ShellView::Volumes => app
            .selected_volume()
            .map(|v| format!("/{}", v.name))
            .unwrap_or_default(),
        ShellView::Networks => app
            .selected_network()
            .map(|n| format!("/{}", n.name))
            .unwrap_or_default(),
        ShellView::Templates => match app.templates_state.kind {
            TemplatesKind::Stacks => app
                .selected_template()
                .map(|t| format!("/{}", t.name))
                .unwrap_or_default(),
            TemplatesKind::Networks => app
                .selected_net_template()
                .map(|t| format!("/{}", t.name))
                .unwrap_or_default(),
        },
        ShellView::TemplateAi => {
            let name = app.template_ai.target_name.trim();
            if name.is_empty() {
                String::new()
            } else {
                format!("/{name}")
            }
        }
        ShellView::Registries => app
            .registries_cfg
            .registries
            .get(app.registries_selected)
            .map(|r| format!("/{}", r.host))
            .unwrap_or_default(),
        ShellView::Inspect => app
            .inspect.target
            .as_ref()
            .map(|t| format!("/{}", t.label))
            .unwrap_or_default(),
        ShellView::Logs => app
            .logs.for_id
            .as_ref()
            .and_then(|_| app.selected_container().map(|c| c.name.clone()))
            .map(|n| format!("/{n}"))
            .unwrap_or_default(),
        ShellView::Help => String::new(),
        ShellView::Messages => String::new(),
        ShellView::ThemeSelector => String::new(),
    }
}
