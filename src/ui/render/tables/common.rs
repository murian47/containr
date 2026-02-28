use crate::ui::state::app::App;
use ratatui::style::Style;

pub(in crate::ui) fn shell_header_style(app: &App) -> Style {
    app.theme.table_header.to_style()
}
