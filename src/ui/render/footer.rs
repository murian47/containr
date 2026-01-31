use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::ui::{App, ShellView};
use crate::ui::render::text::truncate_end;
use crate::ui::theme;

pub(crate) fn draw_shell_footer(
    f: &mut ratatui::Frame,
    app: &App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.footer.to_style();
    f.render_widget(Block::default().style(bg), area);

    let version = format!("v{} ", env!("CARGO_PKG_VERSION"));
    let hint = footer_hint(app.shell_view);

    let w = area.width.max(1) as usize;
    let right_len = version.chars().count();
    let line = if w <= right_len {
        truncate_end(&version, w)
    } else {
        let left_max = w.saturating_sub(right_len + 1);
        let left = truncate_end(hint, left_max);
        let left_len = left.chars().count();
        let gap = w.saturating_sub(right_len + left_len);
        format!("{left}{}{}", " ".repeat(gap), version)
    };
    let line = Line::from(vec![Span::styled(
        line,
        bg.fg(theme::parse_color(&app.theme.footer.fg)),
    )]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: false }),
        area,
    );
}

fn footer_hint(view: ShellView) -> &'static str {
    match view {
        ShellView::Dashboard => {
            " F1 help  ^b sidebar  ^p layout  ^s start  ^o stop  ^r restart  ^d rm  :q quit"
        }
        ShellView::Stacks => {
            " F1 help  ^b sidebar  ^p layout  :q quit"
        }
        ShellView::Containers => {
            " F1 help  ^b sidebar  ^p layout  :q quit"
        }
        ShellView::Images | ShellView::Volumes | ShellView::Networks => {
            " F1 help  ^b sidebar  ^p layout  :q quit"
        }
        ShellView::Templates => {
            " F1 help  ^b sidebar  ^p layout  :q quit"
        }
        ShellView::Registries => {
            " F1 help  ^b sidebar  ^p layout  ^y test  :q quit"
        }
        ShellView::Logs => {
            " F1 help  / search  : cmd  n/N match  m regex  l numbers  q back  :q quit"
        }
        ShellView::Inspect => {
            " F1 help  / search  : cmd  n/N match  m regex  Enter expand  q back  :q quit"
        }
        ShellView::Help => " F1 help  Up/Down scroll  PageUp/PageDown  q back  :q quit",
        ShellView::Messages => {
            " F1 help  Up/Down select  Left/Right hscroll  PgUp/PgDn  ^c copy  ^g toggle  q back  :q quit"
        }
        ShellView::ThemeSelector => {
            " F1 help  / search  Up/Down select  Enter apply  Esc cancel"
        }
    }
}
