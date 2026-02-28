use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellView;
use crate::ui::text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_theme_selector_mode(app: &mut App, key: KeyEvent) -> bool {
    if app.shell_view != ShellView::ThemeSelector {
        return false;
    }

    match key.code {
        KeyCode::Esc => {
            if app.theme_selector.search_mode {
                app.theme_selector.search_mode = false;
                app.theme_selector.search_input.clear();
                app.theme_selector.search_cursor = 0;
            } else {
                app.theme_selector_cancel();
            }
            return true;
        }
        KeyCode::Char('q') if key.modifiers.is_empty() => {
            app.theme_selector_cancel();
            return true;
        }
        KeyCode::Enter => {
            if app.theme_selector.search_mode {
                app.theme_selector.search_mode = false;
            } else {
                app.theme_selector_apply();
            }
            return true;
        }
        KeyCode::Char('/') if key.modifiers.is_empty() => {
            app.theme_selector.search_mode = true;
            app.theme_selector.search_input.clear();
            app.theme_selector.search_cursor = 0;
            return true;
        }
        _ => {}
    }

    if app.theme_selector.search_mode {
        match key.code {
            KeyCode::Backspace => {
                backspace_at_cursor(
                    &mut app.theme_selector.search_input,
                    &mut app.theme_selector.search_cursor,
                );
                let query = app.theme_selector.search_input.clone();
                app.theme_selector_search(&query);
                return true;
            }
            KeyCode::Delete => {
                delete_at_cursor(
                    &mut app.theme_selector.search_input,
                    &mut app.theme_selector.search_cursor,
                );
                let query = app.theme_selector.search_input.clone();
                app.theme_selector_search(&query);
                return true;
            }
            KeyCode::Left => {
                app.theme_selector.search_cursor = clamp_cursor_to_text(
                    &app.theme_selector.search_input,
                    app.theme_selector.search_cursor,
                )
                .saturating_sub(1);
                return true;
            }
            KeyCode::Right => {
                let len = app.theme_selector.search_input.chars().count();
                app.theme_selector.search_cursor = clamp_cursor_to_text(
                    &app.theme_selector.search_input,
                    app.theme_selector.search_cursor,
                )
                .saturating_add(1)
                .min(len);
                return true;
            }
            KeyCode::Home => {
                app.theme_selector.search_cursor = 0;
                return true;
            }
            KeyCode::End => {
                app.theme_selector.search_cursor = app.theme_selector.search_input.chars().count();
                return true;
            }
            KeyCode::Char(ch) => {
                if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                    insert_char_at_cursor(
                        &mut app.theme_selector.search_input,
                        &mut app.theme_selector.search_cursor,
                        ch,
                    );
                    let query = app.theme_selector.search_input.clone();
                    app.theme_selector_search(&query);
                    return true;
                }
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Up => app.theme_selector_move(-1),
        KeyCode::Down => app.theme_selector_move(1),
        KeyCode::PageUp => app.theme_selector_page_move(-1),
        KeyCode::PageDown => app.theme_selector_page_move(1),
        KeyCode::Home => app.theme_selector_move(-(app.theme_selector.selected as i32)),
        KeyCode::End => {
            let last = app.theme_selector.names.len().saturating_sub(1) as i32;
            let delta = last.saturating_sub(app.theme_selector.selected as i32);
            app.theme_selector_move(delta);
        }
        _ => {}
    }
    true
}
