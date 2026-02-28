use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellFocus;
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_registries_navigation(app: &mut App, key: KeyEvent) {
    if app.shell_focus == ShellFocus::Details {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.registries_details_scroll = app.registries_details_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => app.registries_details_scroll += 1,
            KeyCode::PageUp => {
                app.registries_details_scroll = app.registries_details_scroll.saturating_sub(10)
            }
            KeyCode::PageDown => app.registries_details_scroll += 10,
            KeyCode::Home => app.registries_details_scroll = 0,
            KeyCode::End => app.registries_details_scroll = usize::MAX,
            _ => {}
        }
    } else {
        let before = app.registries_selected;
        let total = app.registries_cfg.registries.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.registries_selected = app.registries_selected.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if total > 0 {
                    app.registries_selected = (app.registries_selected + 1).min(total - 1);
                } else {
                    app.registries_selected = 0;
                }
            }
            KeyCode::PageUp => app.registries_selected = app.registries_selected.saturating_sub(10),
            KeyCode::PageDown => {
                if total > 0 {
                    app.registries_selected = (app.registries_selected + 10).min(total - 1);
                } else {
                    app.registries_selected = 0;
                }
            }
            KeyCode::Home => app.registries_selected = 0,
            KeyCode::End => app.registries_selected = total.saturating_sub(1),
            _ => {}
        }
        if app.registries_selected != before {
            app.registries_details_scroll = 0;
        }
    }
}
