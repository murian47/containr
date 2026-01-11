use ratatui::layout::Alignment;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::ui::render::utils::truncate_end;
use crate::ui::App;

pub(crate) fn draw_rate_limit_banner(
    f: &mut ratatui::Frame,
    app: &App,
    banner: Option<String>,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    let text = banner.unwrap_or_default();
    let style = bg
        .patch(app.theme.text_info.to_style())
        .add_modifier(ratatui::style::Modifier::BOLD);
    let content = truncate_end(&text, area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(content)
            .style(style)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        area,
    );
}
