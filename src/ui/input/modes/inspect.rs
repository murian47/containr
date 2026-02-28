use crate::ui::core::types::InspectMode;
use crate::ui::render::utils::write_text_file;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellView;
use crate::ui::text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_inspect_mode(app: &mut App, key: KeyEvent) -> bool {
    if app.shell_view != ShellView::Inspect {
        return false;
    }
    match app.inspect.mode {
        InspectMode::Search => match key.code {
            KeyCode::Enter => app.inspect_commit_search(),
            KeyCode::Esc => app.inspect_exit_input(),
            KeyCode::Backspace => {
                backspace_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                app.rebuild_inspect_lines();
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                app.rebuild_inspect_lines();
            }
            KeyCode::Left => {
                app.inspect.input_cursor =
                    clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.inspect.input.chars().count();
                app.inspect.input_cursor =
                    clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_add(1)
                        .min(len);
            }
            KeyCode::Home => app.inspect.input_cursor = 0,
            KeyCode::End => app.inspect.input_cursor = app.inspect.input.chars().count(),
            KeyCode::Char(ch) => {
                if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                    insert_char_at_cursor(
                        &mut app.inspect.input,
                        &mut app.inspect.input_cursor,
                        ch,
                    );
                    app.rebuild_inspect_lines();
                }
            }
            _ => {}
        },
        InspectMode::Command => match key.code {
            KeyCode::Enter => {
                let cmd = app.inspect.input.trim().to_string();
                app.push_cmd_history(&cmd);
                let (force, path) = if let Some(rest) = cmd.strip_prefix("save!") {
                    (true, rest.trim())
                } else if let Some(rest) = cmd.strip_prefix("save") {
                    (false, rest.trim())
                } else {
                    (false, "")
                };
                if cmd.starts_with("save") {
                    if path.is_empty() {
                        app.inspect.error = Some("usage: save <file>".to_string());
                    } else {
                        match app.inspect.value.as_ref() {
                            None => app.inspect.error = Some("no inspect data loaded".to_string()),
                            Some(v) => match serde_json::to_string_pretty(v) {
                                Ok(s) => match write_text_file(path, &s, force) {
                                    Ok(p) => {
                                        app.set_info(format!("saved inspect to {}", p.display()))
                                    }
                                    Err(e) => {
                                        app.inspect.error = Some(format!("save failed: {e:#}"))
                                    }
                                },
                                Err(e) => {
                                    app.inspect.error =
                                        Some(format!("failed to serialize inspect: {e:#}"))
                                }
                            },
                        }
                    }
                    app.inspect.mode = InspectMode::Normal;
                    app.inspect.input.clear();
                    app.inspect.input_cursor = 0;
                    app.rebuild_inspect_lines();
                    return true;
                }
                match cmd.as_str() {
                    "" => {}
                    "q" | "quit" => app.back_from_full_view(),
                    "e" | "expand" | "expandall" => app.inspect_expand_all(),
                    "c" | "collapse" | "collapseall" => app.inspect_collapse_all(),
                    "y" => app.inspect_copy_selected_value(true),
                    "p" => app.inspect_copy_selected_path(),
                    _ => app.inspect.error = Some(format!("unknown command: {cmd}")),
                }
                app.inspect.mode = InspectMode::Normal;
                app.inspect.input.clear();
                app.inspect.input_cursor = 0;
                app.rebuild_inspect_lines();
            }
            KeyCode::Esc => {
                app.inspect.mode = InspectMode::Normal;
                app.inspect.input.clear();
                app.inspect.input_cursor = 0;
                app.rebuild_inspect_lines();
                app.inspect.cmd_history.reset_nav();
            }
            KeyCode::Up => {
                if let Some(s) = app.inspect.cmd_history.prev(&app.inspect.input) {
                    set_text_and_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor, s);
                }
            }
            KeyCode::Down => {
                if let Some(s) = app.inspect.cmd_history.next() {
                    set_text_and_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor, s);
                }
            }
            KeyCode::Backspace => {
                backspace_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                app.inspect.cmd_history.on_edit();
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor);
                app.inspect.cmd_history.on_edit();
            }
            KeyCode::Left => {
                app.inspect.input_cursor =
                    clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.inspect.input.chars().count();
                app.inspect.input_cursor =
                    clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_add(1)
                        .min(len);
            }
            KeyCode::Home => app.inspect.input_cursor = 0,
            KeyCode::End => app.inspect.input_cursor = app.inspect.input.chars().count(),
            KeyCode::Char(ch) => {
                if !ch.is_control() {
                    insert_char_at_cursor(
                        &mut app.inspect.input,
                        &mut app.inspect.input_cursor,
                        ch,
                    );
                    app.inspect.cmd_history.on_edit();
                }
            }
            _ => {}
        },
        InspectMode::Normal => {}
    }
    app.inspect.mode != InspectMode::Normal
}
