//! Key handling / input dispatch.

use crate::ui::commands;
use crate::ui::render::cmdline::cmdline_apply_completion;
use crate::ui::render::utils::write_text_file;
use crate::ui::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    is_single_letter_without_modifiers, key_spec_from_event, lookup_binding,
    lookup_scoped_binding, set_text_and_cursor, shell_cycle_focus, ActionRequest, App, BindingHit,
    Connection, InspectMode, InspectTarget, KeyScope, LogsMode, ShellFocus, ShellView,
};
use crossterm::event::{KeyCode, KeyModifiers};
use tokio::sync::{mpsc, watch};
use std::time::Duration;

pub(super) fn handle_shell_key_impl(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
    refresh_interval_tx: &watch::Sender<Duration>,
    refresh_pause_tx: &watch::Sender<bool>,
    image_update_limit_tx: &watch::Sender<usize>,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    // "always" bindings are evaluated before everything else (including input modes).
    if let Some(spec) = key_spec_from_event(key) {
        if let Some(hit) = lookup_binding(app, KeyScope::Always, spec) {
            match hit {
                BindingHit::Disabled => return,
                BindingHit::Cmd(cmd) => {
                    commands::cmdline_cmd::execute_cmdline(
                        app,
                        &cmd,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_refresh_tx,
                        dash_all_enabled_tx,
                        refresh_interval_tx,
                        refresh_pause_tx,
                        image_update_limit_tx,
                        inspect_req_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
            }
        }
    }

    if app.refresh_paused
        && key.modifiers.is_empty()
        && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
    {
        app.refresh_now(
            refresh_tx,
            dash_refresh_tx,
            dash_all_refresh_tx,
            refresh_pause_tx,
        );
        return;
    }

    if app.shell_cmdline.mode {
        if let Some(confirm) = app.shell_cmdline.confirm.clone() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Re-run the original command with the force modifier to auto-confirm.
                    let cmdline = format!("!{}", confirm.cmdline);
                    app.shell_cmdline.confirm = None;
                    app.shell_cmdline.mode = false;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.history.reset_nav();
                    commands::cmdline_cmd::execute_cmdline(
                        app,
                        &cmdline,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_refresh_tx,
                        dash_all_enabled_tx,
                        refresh_interval_tx,
                        refresh_pause_tx,
                        image_update_limit_tx,
                        inspect_req_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // Cancel.
                    app.shell_cmdline.confirm = None;
                    app.shell_cmdline.mode = false;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.history.reset_nav();
                    return;
                }
                _ => return,
            }
        }

        match key.code {
            KeyCode::Enter => {
                let cmdline = app.shell_cmdline.input.trim().to_string();
                app.shell_cmdline.mode = false;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.push_cmd_history(&cmdline);
                commands::cmdline_cmd::execute_cmdline(
                    app,
                    &cmdline,
                    conn_tx,
                    refresh_tx,
                    dash_refresh_tx,
                    dash_all_refresh_tx,
                    dash_all_enabled_tx,
                    refresh_interval_tx,
                    refresh_pause_tx,
                    image_update_limit_tx,
                    inspect_req_tx,
                    logs_req_tx,
                    action_req_tx,
                );
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
                    set_text_and_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, s);
                }
            }
            KeyCode::Down => {
                if let Some(s) = app.shell_cmdline.history.next() {
                    set_text_and_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, s);
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
                app.shell_cmdline.cursor = clamp_cursor_to_text(&app.shell_cmdline.input, app.shell_cmdline.cursor)
                    .saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.shell_cmdline.input.chars().count();
                app.shell_cmdline.cursor =
                    clamp_cursor_to_text(&app.shell_cmdline.input, app.shell_cmdline.cursor).saturating_add(1).min(len);
            }
            KeyCode::Home => app.shell_cmdline.cursor = 0,
            KeyCode::End => app.shell_cmdline.cursor = app.shell_cmdline.input.chars().count(),
            KeyCode::Tab => {
                cmdline_apply_completion(app);
            }
            KeyCode::Char(ch) => {
                // Common readline-like movement shortcuts.
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
        return;
    }

    // Input modes first (vim-like): when editing, do not treat keys as global shortcuts.
    if app.shell_view == ShellView::Logs {
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
                    app.logs.input_cursor = clamp_cursor_to_text(&app.logs.input, app.logs.input_cursor)
                        .saturating_add(1)
                        .min(len);
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
                    // Minimal command mode for now.
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
                        return;
                    }
                    let mut parts = cmdline.split_whitespace();
                    let cmd = parts.next().unwrap_or("");
                    match cmd {
                        "" => {}
                        "q" | "quit" => app.back_from_full_view(),
                        "j" => {
                            let Some(n) = parts.next() else {
                                app.set_warn("usage: j <line>");
                                // keep mode change below
                                app.logs.mode = LogsMode::Normal;
                                app.logs.command.clear();
                                app.logs.command_cursor = 0;
                                app.logs_rebuild_matches();
                                return;
                            };
                            match n.parse::<usize>() {
                                Ok(n) if n > 0 => {
                                    let total = app.logs_total_lines();
                                    app.logs.cursor =
                                        n.saturating_sub(1).min(total.saturating_sub(1));
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
                                    return;
                                };
                                match v.parse::<usize>() {
                                    Ok(n) if (1..=200_000).contains(&n) => {
                                        app.logs.tail = n;
                                        app.persist_config();
                                        if let Some(id) = app.logs.for_id.clone() {
                                            app.logs.loading = true;
                                            let _ = logs_req_tx.send((id, app.logs.tail.max(1)));
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
                        insert_char_at_cursor(
                            &mut app.logs.command,
                            &mut app.logs.command_cursor,
                            ch,
                        );
                        app.logs.cmd_history.on_edit();
                    }
                }
                _ => {}
            },
            LogsMode::Normal => {}
        }
        if app.logs.mode != LogsMode::Normal {
            return;
        }
    }

    if app.shell_view == ShellView::Inspect {
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
                                None => {
                                    app.inspect.error = Some("no inspect data loaded".to_string())
                                }
                                Some(v) => match serde_json::to_string_pretty(v) {
                                    Ok(s) => match write_text_file(path, &s, force) {
                                        Ok(p) => app
                                            .set_info(format!("saved inspect to {}", p.display())),
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
                        return;
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
                        set_text_and_cursor(
                            &mut app.inspect.input,
                            &mut app.inspect.input_cursor,
                            s,
                        );
                    }
                }
                KeyCode::Down => {
                    if let Some(s) = app.inspect.cmd_history.next() {
                        set_text_and_cursor(
                            &mut app.inspect.input,
                            &mut app.inspect.input_cursor,
                            s,
                        );
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
                    app.inspect.input_cursor = clamp_cursor_to_text(&app.inspect.input, app.inspect.input_cursor)
                        .saturating_sub(1);
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
        if app.inspect.mode != InspectMode::Normal {
            return;
        }
    }

    if app.shell_view == ShellView::ThemeSelector {
        match key.code {
            KeyCode::Esc => {
                if app.theme_selector.search_mode {
                    app.theme_selector.search_mode = false;
                    app.theme_selector.search_input.clear();
                    app.theme_selector.search_cursor = 0;
                } else {
                    app.theme_selector_cancel();
                }
                return;
            }
            KeyCode::Char('q') if key.modifiers.is_empty() => {
                app.theme_selector_cancel();
                return;
            }
            KeyCode::Enter => {
                if app.theme_selector.search_mode {
                    app.theme_selector.search_mode = false;
                } else {
                    app.theme_selector_apply();
                }
                return;
            }
            KeyCode::Char('/') if key.modifiers.is_empty() => {
                app.theme_selector.search_mode = true;
                app.theme_selector.search_input.clear();
                app.theme_selector.search_cursor = 0;
                return;
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
                    return;
                }
                KeyCode::Delete => {
                    delete_at_cursor(
                        &mut app.theme_selector.search_input,
                        &mut app.theme_selector.search_cursor,
                    );
                    let query = app.theme_selector.search_input.clone();
                    app.theme_selector_search(&query);
                    return;
                }
                KeyCode::Left => {
                    app.theme_selector.search_cursor =
                        clamp_cursor_to_text(&app.theme_selector.search_input, app.theme_selector.search_cursor)
                            .saturating_sub(1);
                    return;
                }
                KeyCode::Right => {
                    let len = app.theme_selector.search_input.chars().count();
                    app.theme_selector.search_cursor =
                        clamp_cursor_to_text(&app.theme_selector.search_input, app.theme_selector.search_cursor)
                            .saturating_add(1)
                            .min(len);
                    return;
                }
                KeyCode::Home => {
                    app.theme_selector.search_cursor = 0;
                    return;
                }
                KeyCode::End => {
                    app.theme_selector.search_cursor = app.theme_selector.search_input.chars().count();
                    return;
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
                        return;
                    }
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Up => {
                app.theme_selector_move(-1);
            }
            KeyCode::Down => {
                app.theme_selector_move(1);
            }
            KeyCode::PageUp => {
                app.theme_selector_page_move(-1);
            }
            KeyCode::PageDown => {
                app.theme_selector_page_move(1);
            }
            KeyCode::Home => {
                app.theme_selector_move(-(app.theme_selector.selected as i32));
            }
            KeyCode::End => {
                let last = app
                    .theme_selector
                    .names
                    .len()
                    .saturating_sub(1) as i32;
                let delta = last.saturating_sub(app.theme_selector.selected as i32);
                app.theme_selector_move(delta);
            }
            _ => {}
        }
        return;
    }

    // Custom key bindings (outside of input modes). Skip single-letter shortcuts when sidebar has focus.
    if let Some(spec) = key_spec_from_event(key) {
        if app.shell_focus != ShellFocus::Sidebar || !is_single_letter_without_modifiers(spec) {
            if let Some(hit) = lookup_scoped_binding(app, spec) {
                match hit {
                    BindingHit::Disabled => return,
                    BindingHit::Cmd(cmd) => {
                        commands::cmdline_cmd::execute_cmdline(
                            app,
                            &cmd,
                            conn_tx,
                            refresh_tx,
                            dash_refresh_tx,
                            dash_all_refresh_tx,
                            dash_all_enabled_tx,
                            refresh_interval_tx,
                            refresh_pause_tx,
                            image_update_limit_tx,
                            inspect_req_tx,
                            logs_req_tx,
                            action_req_tx,
                        );
                        return;
                    }
                }
            }
        }
    }

    if app.log_dock_enabled
        && app.shell_focus == ShellFocus::Dock
        && !matches!(
            app.shell_view,
            ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages | ShellView::ThemeSelector
        )
    {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(1)
            }
            KeyCode::PageUp => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(10),
            KeyCode::Left => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_sub(4),
            KeyCode::Right => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_add(4),
            KeyCode::Home => app.shell_msgs.scroll = 0,
            KeyCode::End => app.shell_msgs.scroll = usize::MAX,
            _ => {}
        }
        if !matches!(key.code, KeyCode::Tab) {
            return;
        }
    }

    // Global keys.
    match key.code {
        KeyCode::Tab => {
            shell_cycle_focus(app);
            return;
        }
        KeyCode::Char(':') if key.modifiers.is_empty() => {
            // In Logs/Inspect, ':' is view-local command mode (vim-like).
            match app.shell_view {
                ShellView::Logs => {
                    app.logs.mode = LogsMode::Command;
                    app.logs.command.clear();
                    app.logs.command_cursor = 0;
                    app.logs_rebuild_matches();
                }
                ShellView::Inspect => app.inspect_enter_command(),
                _ => {
                    app.shell_cmdline.mode = true;
                    app.shell_cmdline.input.clear();
                    app.shell_cmdline.cursor = 0;
                    app.shell_cmdline.confirm = None;
                }
            }
            return;
        }
        KeyCode::Char('q') if key.modifiers.is_empty() => {
            app.back_from_full_view();
            return;
        }
        _ => {}
    }

    super::views::handle_view_navigation(
        app,
        key,
        conn_tx,
        refresh_tx,
        dash_refresh_tx,
        dash_all_enabled_tx,
        inspect_req_tx,
        logs_req_tx,
        action_req_tx,
    );
}
