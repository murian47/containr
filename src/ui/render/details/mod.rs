mod containers;
mod meta;
mod registries;
mod resources;
mod stacks;
mod templates;

use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellView};
use ratatui::style::Style;

pub(in crate::ui) fn draw_shell_main_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    match app.shell_view {
        ShellView::Dashboard => {}
        ShellView::Stacks => stacks::draw_shell_stack_details(f, app, area),
        ShellView::Containers => containers::draw_shell_container_details(f, app, area),
        ShellView::Images => resources::draw_shell_image_details(f, app, area),
        ShellView::Volumes => resources::draw_shell_volume_details(f, app, area),
        ShellView::Networks => resources::draw_shell_network_details(f, app, area),
        ShellView::Templates => templates::draw_shell_template_details(f, app, area),
        ShellView::Registries => registries::draw_shell_registry_details(f, app, area),
        ShellView::Logs => meta::draw_shell_logs_meta(f, app, area),
        ShellView::Inspect => meta::draw_shell_inspect_meta(f, app, area),
        ShellView::Help => meta::draw_shell_help_meta(f, app, area),
        ShellView::Messages => meta::draw_shell_messages_meta(f, app, area),
        ShellView::ThemeSelector => {}
    }
}

pub(super) fn panel_bg(app: &App) -> Style {
    if app.shell_focus == ShellFocus::Details {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    }
}
