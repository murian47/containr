use crate::ui::render::scroll::draw_shell_scrollbar_v;
use crate::ui::render::text::truncate_end;
use crate::ui::render::utils::shell_row_highlight;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellFocus;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

pub(super) fn draw_theme_selector_sidebar(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = if app.shell_focus == ShellFocus::Sidebar {
        app.theme.panel_focused.to_style()
    } else {
        app.theme.panel.to_style()
    };
    f.render_widget(Block::default().style(bg), area);

    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    if inner.height == 0 {
        return;
    }
    let list_area = inner;
    let visible = list_area.height.max(1) as usize;

    if app.theme_selector.page_size != visible {
        app.theme_selector.page_size = visible;
        if app.theme_selector.center_on_open {
            app.theme_selector_adjust_scroll(true);
            app.theme_selector.center_on_open = false;
        } else {
            app.theme_selector_adjust_scroll(false);
        }
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(list_area);
    let list_area = cols[0];
    let vbar_area = cols[1];

    let list_w = list_area.width.max(1) as usize;
    let mut items: Vec<ListItem> = Vec::new();
    for (idx, name) in app.theme_selector.names.iter().enumerate() {
        let selected = idx == app.theme_selector.selected;
        let st = if selected {
            shell_row_highlight(app)
        } else {
            bg
        };
        let label = format!(" {name}");
        let label = truncate_end(&label, list_w);
        items.push(ListItem::new(Line::from(Span::styled(label, st))));
    }

    let mut state = ListState::default();
    if !app.theme_selector.names.is_empty() {
        let selected = app.theme_selector.selected;
        let max_scroll = app.theme_selector.names.len().saturating_sub(visible);
        let scroll = app.theme_selector.scroll.min(max_scroll);
        *state.offset_mut() = scroll;
        state.select(Some(selected));
        app.theme_selector.scroll = scroll;
    }
    f.render_stateful_widget(List::new(items), list_area, &mut state);

    let total = app.theme_selector.names.len();
    let max_scroll = total.saturating_sub(visible);
    draw_shell_scrollbar_v(
        f,
        vbar_area,
        app.theme_selector.scroll,
        max_scroll,
        total,
        visible,
        app.ascii_only,
        &app.theme,
    );
}
