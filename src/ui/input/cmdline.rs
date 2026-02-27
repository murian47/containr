use super::context::InputCtx;
use crate::ui::render::cmdline::cmdline_apply_completion;
use crate::ui::state::app::App;
use crate::ui::text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_cmdline_mode(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) -> bool {
    if !app.shell_cmdline.mode {
        return false;
    }

    if let Some(confirm) = app.shell_cmdline.confirm.clone() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let cmdline = format!("!{}", confirm.cmdline);
                app.shell_cmdline.confirm = None;
                app.shell_cmdline.mode = false;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.shell_cmdline.history.reset_nav();
                ctx.execute_cmdline(app, &cmdline);
                return true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.shell_cmdline.confirm = None;
                app.shell_cmdline.mode = false;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.shell_cmdline.history.reset_nav();
                return true;
            }
            _ => return true,
        }
    }

    match key.code {
        KeyCode::Enter => {
            let cmdline = app.shell_cmdline.input.trim().to_string();
            app.shell_cmdline.mode = false;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            app.push_cmd_history(&cmdline);
            ctx.execute_cmdline(app, &cmdline);
        }
        KeyCode::Esc => {
            app.shell_cmdline.mode = false;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.history.reset_nav();
        }
        KeyCode::Up => {
            if let Some(s) = app.shell_cmdline.history.prev(&app.shell_cmdline.input) {
                set_text_and_cursor(
                    &mut app.shell_cmdline.input,
                    &mut app.shell_cmdline.cursor,
                    s,
                );
            }
        }
        KeyCode::Down => {
            if let Some(s) = app.shell_cmdline.history.next() {
                set_text_and_cursor(
                    &mut app.shell_cmdline.input,
                    &mut app.shell_cmdline.cursor,
                    s,
                );
            }
        }
        KeyCode::Backspace => {
            backspace_at_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor);
            app.shell_cmdline.history.on_edit();
        }
        KeyCode::Delete => {
            delete_at_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor);
            app.shell_cmdline.history.on_edit();
        }
        KeyCode::Left => {
            app.shell_cmdline.cursor =
                clamp_cursor_to_text(&app.shell_cmdline.input, app.shell_cmdline.cursor)
                    .saturating_sub(1);
        }
        KeyCode::Right => {
            let len = app.shell_cmdline.input.chars().count();
            app.shell_cmdline.cursor =
                clamp_cursor_to_text(&app.shell_cmdline.input, app.shell_cmdline.cursor)
                    .saturating_add(1)
                    .min(len);
        }
        KeyCode::Home => app.shell_cmdline.cursor = 0,
        KeyCode::End => app.shell_cmdline.cursor = app.shell_cmdline.input.chars().count(),
        KeyCode::Tab => cmdline_apply_completion(app),
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match ch {
                    'a' | 'A' => app.shell_cmdline.cursor = 0,
                    'e' | 'E' => app.shell_cmdline.cursor = app.shell_cmdline.input.chars().count(),
                    'u' | 'U' => {
                        app.shell_cmdline.input.clear();
                        app.shell_cmdline.cursor = 0;
                        app.shell_cmdline.history.on_edit();
                    }
                    _ => {}
                }
            } else if !ch.is_control() {
                insert_char_at_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, ch);
                app.shell_cmdline.history.on_edit();
            }
        }
        _ => {}
    }
    true
}
