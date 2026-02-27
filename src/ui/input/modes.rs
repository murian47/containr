use super::context::InputCtx;
use crate::ui::render::utils::write_text_file;
use crate::ui::core::types::{InspectMode, LogsMode};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::ShellView;
use crate::ui::text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_view_input_modes(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) -> bool {
    if handle_logs_mode(app, key, ctx) {
        return true;
    }
    if handle_inspect_mode(app, key) {
        return true;
    }
    handle_theme_selector_mode(app, key)
}

fn handle_logs_mode(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) -> bool {
    if app.shell_view != ShellView::Logs {
        return false;
    }
    match app.logs.mode {
        LogsMode::Search => match key.code {
            KeyCode::Enter => app.logs_commit_search(),
            KeyCode::Esc => app.logs_cancel_search(),
            KeyCode::Backspace => {
                backspace_at_cursor(&mut app.logs.input, &mut app.logs.input_cursor);
                app.logs_rebuild_matches();
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut app.logs.input, &mut app.logs.input_cursor);
                app.logs_rebuild_matches();
            }
            KeyCode::Left => {
                app.logs.input_cursor =
                    clamp_cursor_to_text(&app.logs.input, app.logs.input_cursor).saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.logs.input.chars().count();
                app.logs.input_cursor =
                    clamp_cursor_to_text(&app.logs.input, app.logs.input_cursor).saturating_add(1).min(len);
            }
            KeyCode::Home => app.logs.input_cursor = 0,
            KeyCode::End => app.logs.input_cursor = app.logs.input.chars().count(),
            KeyCode::Char(ch) => {
                if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                    insert_char_at_cursor(&mut app.logs.input, &mut app.logs.input_cursor, ch);
                    app.logs_rebuild_matches();
                }
            }
            _ => {}
        },
        LogsMode::Command => match key.code {
            KeyCode::Enter => {
                let cmdline = app.logs.command.trim().to_string();
                app.push_cmd_history(&cmdline);
                let (force, path) = if let Some(rest) = cmdline.strip_prefix("save!") {
                    (true, rest.trim())
                } else if let Some(rest) = cmdline.strip_prefix("save") {
                    (false, rest.trim())
                } else {
                    (false, "")
                };
                if cmdline.starts_with("save") {
                    if path.is_empty() {
                        app.set_warn("usage: save <file>");
                    } else {
                        match app.logs.text.as_deref() {
                            None => app.set_warn("no logs loaded"),
                            Some(text) => match write_text_file(path, text, force) {
                                Ok(p) => app.set_info(format!("saved logs to {}", p.display())),
                                Err(e) => app.set_error(format!("save failed: {e:#}")),
                            },
                        }
                    }
                    app.logs.mode = LogsMode::Normal;
                    app.logs.command.clear();
                    app.logs.command_cursor = 0;
                    app.logs_rebuild_matches();
                    return true;
                }
                let mut parts = cmdline.split_whitespace();
                let cmd = parts.next().unwrap_or("");
                match cmd {
                    "" => {}
                    "q" | "quit" => app.back_from_full_view(),
                    "j" => {
                        let Some(n) = parts.next() else {
                            app.set_warn("usage: j <line>");
                            app.logs.mode = LogsMode::Normal;
                            app.logs.command.clear();
                            app.logs.command_cursor = 0;
                            app.logs_rebuild_matches();
                            return true;
                        };
                        match n.parse::<usize>() {
                            Ok(n) if n > 0 => {
                                let total = app.logs_total_lines();
                                app.logs.cursor = n.saturating_sub(1).min(total.saturating_sub(1));
                            }
                            _ => app.set_warn("usage: j <line>"),
                        }
                    }
                    "set" => match parts.next().unwrap_or("") {
                        "number" => app.logs.show_line_numbers = true,
                        "nonumber" => app.logs.show_line_numbers = false,
                        "logtail" => {
                            let Some(v) = parts.next() else {
                                app.set_warn("usage: set logtail <lines>");
                                app.logs.mode = LogsMode::Normal;
                                app.logs.command.clear();
                                app.logs.command_cursor = 0;
                                app.logs_rebuild_matches();
                                return true;
                            };
                            match v.parse::<usize>() {
                                Ok(n) if (1..=200_000).contains(&n) => {
                                    app.logs.tail = n;
                                    app.persist_config();
                                    if let Some(id) = app.logs.for_id.clone() {
                                        app.logs.loading = true;
                                        let _ = ctx.logs_req_tx.send((id, app.logs.tail.max(1)));
                                    }
                                }
                                _ => app.set_warn("logtail must be 1..200000"),
                            }
                        }
                        "regex" => {
                            app.logs.use_regex = true;
                            app.logs_rebuild_matches();
                        }
                        "noregex" => {
                            app.logs.use_regex = false;
                            app.logs_rebuild_matches();
                        }
                        x => app.set_warn(format!("unknown option: {x}")),
                    },
                    _ => app.set_warn(format!("unknown command: {cmdline}")),
                }
                app.logs.mode = LogsMode::Normal;
                app.logs.command.clear();
                app.logs.command_cursor = 0;
                app.logs_rebuild_matches();
            }
            KeyCode::Esc => {
                app.logs.mode = LogsMode::Normal;
                app.logs.command.clear();
                app.logs.command_cursor = 0;
                app.logs_rebuild_matches();
                app.logs.cmd_history.reset_nav();
            }
            KeyCode::Up => {
                if let Some(s) = app.logs.cmd_history.prev(&app.logs.command) {
                    set_text_and_cursor(&mut app.logs.command, &mut app.logs.command_cursor, s);
                }
            }
            KeyCode::Down => {
                if let Some(s) = app.logs.cmd_history.next() {
                    set_text_and_cursor(&mut app.logs.command, &mut app.logs.command_cursor, s);
                }
            }
            KeyCode::Backspace => {
                backspace_at_cursor(&mut app.logs.command, &mut app.logs.command_cursor);
                app.logs.cmd_history.on_edit();
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut app.logs.command, &mut app.logs.command_cursor);
                app.logs.cmd_history.on_edit();
            }
            KeyCode::Left => {
                app.logs.command_cursor =
                    clamp_cursor_to_text(&app.logs.command, app.logs.command_cursor).saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.logs.command.chars().count();
                app.logs.command_cursor = clamp_cursor_to_text(&app.logs.command, app.logs.command_cursor)
                    .saturating_add(1)
                    .min(len);
            }
            KeyCode::Home => app.logs.command_cursor = 0,
            KeyCode::End => app.logs.command_cursor = app.logs.command.chars().count(),
            KeyCode::Char(ch) => {
                if !ch.is_control() {
                    insert_char_at_cursor(&mut app.logs.command, &mut app.logs.command_cursor, ch);
                    app.logs.cmd_history.on_edit();
                }
            }
            _ => {}
        },
        LogsMode::Normal => {}
    }
    app.logs.mode != LogsMode::Normal
}

fn handle_inspect_mode(app: &mut App, key: KeyEvent) -> bool {
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
                    clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor).saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.inspect.input.chars().count();
                app.inspect.input_cursor = clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                    .saturating_add(1)
                    .min(len);
            }
            KeyCode::Home => app.inspect.input_cursor = 0,
            KeyCode::End => app.inspect.input_cursor = app.inspect.input.chars().count(),
            KeyCode::Char(ch) => {
                if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                    insert_char_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor, ch);
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
                                    Ok(p) => app.set_info(format!("saved inspect to {}", p.display())),
                                    Err(e) => app.inspect.error = Some(format!("save failed: {e:#}")),
                                },
                                Err(e) => {
                                    app.inspect.error = Some(format!("failed to serialize inspect: {e:#}"))
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
                    clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor).saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.inspect.input.chars().count();
                app.inspect.input_cursor = clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                    .saturating_add(1)
                    .min(len);
            }
            KeyCode::Home => app.inspect.input_cursor = 0,
            KeyCode::End => app.inspect.input_cursor = app.inspect.input.chars().count(),
            KeyCode::Char(ch) => {
                if !ch.is_control() {
                    insert_char_at_cursor(&mut app.inspect.input, &mut app.inspect.input_cursor, ch);
                    app.inspect.cmd_history.on_edit();
                }
            }
            _ => {}
        },
        InspectMode::Normal => {}
    }
    app.inspect.mode != InspectMode::Normal
}

fn handle_theme_selector_mode(app: &mut App, key: KeyEvent) -> bool {
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
                app.theme_selector.search_cursor =
                    clamp_cursor_to_text(&app.theme_selector.search_input, app.theme_selector.search_cursor)
                        .saturating_sub(1);
                return true;
            }
            KeyCode::Right => {
                let len = app.theme_selector.search_input.chars().count();
                app.theme_selector.search_cursor =
                    clamp_cursor_to_text(&app.theme_selector.search_input, app.theme_selector.search_cursor)
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
