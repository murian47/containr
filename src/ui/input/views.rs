use crate::ui::actions;
use crate::ui::render::sidebar::{shell_move_sidebar, shell_sidebar_items, shell_sidebar_select_item};
use crate::ui::{
    shell_module_shortcut, ActiveView, ActionRequest, App, InspectTarget, ListMode,
    LogsMode, ShellFocus, ShellSidebarItem, ShellView, StackDetailsFocus, TemplatesKind,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::{mpsc, watch};

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_view_navigation(
    app: &mut App,
    key: KeyEvent,
    conn_tx: &watch::Sender<crate::ui::Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if key.modifiers.is_empty() {
        if let KeyCode::Char(mut ch) = key.code {
            for (i, hint) in app.shell_server_shortcuts.iter().copied().enumerate() {
                if hint == '\0' {
                    continue;
                }
                if hint.is_ascii_alphabetic() {
                    ch = ch.to_ascii_uppercase();
                }
                if ch == hint {
                    app.switch_server(
                        i,
                        conn_tx,
                        refresh_tx,
                        dash_refresh_tx,
                        dash_all_enabled_tx,
                    );
                    return;
                }
            }
            if !matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
                let ch_lc = ch.to_ascii_lowercase();
                for v in [
                    ShellView::Dashboard,
                    ShellView::Stacks,
                    ShellView::Containers,
                    ShellView::Images,
                    ShellView::Volumes,
                    ShellView::Networks,
                    ShellView::Templates,
                    ShellView::Registries,
                ] {
                    if ch_lc == shell_module_shortcut(v) {
                        app.set_main_view(v);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                        return;
                    }
                }
            }
        }
    }

    if app.shell_focus == ShellFocus::Sidebar {
        match key.code {
            KeyCode::Up => shell_move_sidebar(app, -1),
            KeyCode::Down => shell_move_sidebar(app, 1),
            KeyCode::Enter => {
                let items = shell_sidebar_items(app);
                let Some(it) = items.get(app.shell_sidebar_selected).copied() else {
                    return;
                };
                match it {
                    ShellSidebarItem::Server(i) => {
                        app.switch_server(
                            i,
                            conn_tx,
                            refresh_tx,
                            dash_refresh_tx,
                            dash_all_enabled_tx,
                        )
                    }
                    ShellSidebarItem::Module(v) => match v {
                        ShellView::Inspect => app.enter_inspect(inspect_req_tx),
                        ShellView::Logs => app.enter_logs(logs_req_tx),
                        _ => {
                            app.set_main_view(v);
                            shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                        }
                    },
                    ShellSidebarItem::Action(a) => {
                        actions::execute_action(app, a, inspect_req_tx, logs_req_tx, action_req_tx)
                    }
                    ShellSidebarItem::Separator => {}
                    ShellSidebarItem::Gap => {}
                }
            }
            _ => {}
        }
        return;
    }

    match app.shell_view {
        ShellView::Dashboard => {}
        ShellView::Stacks
        | ShellView::Containers
        | ShellView::Images
        | ShellView::Volumes
        | ShellView::Networks => {
            if app.shell_focus == ShellFocus::Details {
                let stack_name = if app.shell_view == ShellView::Stacks {
                    let name = app.selected_stack_entry().map(|s| s.name.clone());
                    if let Some(ref n) = name && app.stack_network_count(n) == 0 {
                        app.stack_details_focus = StackDetailsFocus::Containers;
                    }
                    name
                } else {
                    None
                };
                let stack_counts = if let (ShellView::Stacks, Some(ref name)) =
                    (app.shell_view, stack_name.as_ref())
                {
                    let containers = app.stack_container_count(name);
                    let networks = app.stack_network_count(name);
                    Some((containers, networks))
                } else {
                    None
                };
                let scroll = match app.shell_view {
                    ShellView::Stacks => match app.stack_details_focus {
                        StackDetailsFocus::Containers => &mut app.stacks_details_scroll,
                        StackDetailsFocus::Networks => &mut app.stacks_networks_scroll,
                    },
                    ShellView::Containers => &mut app.container_details_scroll,
                    ShellView::Images => &mut app.image_details_scroll,
                    ShellView::Volumes => &mut app.volume_details_scroll,
                    ShellView::Networks => &mut app.network_details_scroll,
                    _ => &mut app.container_details_scroll,
                };
                match key.code {
                    KeyCode::Left | KeyCode::Right => {
                        if app.shell_view == ShellView::Stacks
                            && let Some((_, networks)) = stack_counts
                            && networks > 0
                        {
                            app.stack_details_focus = match app.stack_details_focus {
                                StackDetailsFocus::Containers => StackDetailsFocus::Networks,
                                StackDetailsFocus::Networks => StackDetailsFocus::Containers,
                            };
                            return;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                    KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                    KeyCode::Home => *scroll = 0,
                    KeyCode::End => {
                        if app.shell_view == ShellView::Stacks {
                            if let Some((containers, networks)) = stack_counts {
                                let count = match app.stack_details_focus {
                                    StackDetailsFocus::Containers => containers,
                                    StackDetailsFocus::Networks => networks,
                                };
                                *scroll = count.saturating_sub(1);
                            } else {
                                *scroll = 0;
                            }
                        } else {
                            *scroll = usize::MAX;
                        }
                    }
                    _ => {}
                }
                return;
            }

            app.active_view = match app.shell_view {
                ShellView::Stacks => ActiveView::Stacks,
                ShellView::Containers => ActiveView::Containers,
                ShellView::Images => ActiveView::Images,
                ShellView::Volumes => ActiveView::Volumes,
                ShellView::Networks => ActiveView::Networks,
                _ => app.active_view,
            };

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        app.move_up();
                    }
                }
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        app.move_down();
                    }
                }
                KeyCode::Home => match app.active_view {
                    ActiveView::Stacks => app.stacks_selected = 0,
                    ActiveView::Containers => app.selected = 0,
                    ActiveView::Images => app.images_selected = 0,
                    ActiveView::Volumes => app.volumes_selected = 0,
                    ActiveView::Networks => app.networks_selected = 0,
                },
                KeyCode::End => match app.active_view {
                    ActiveView::Stacks => app.stacks_selected = app.stacks.len().saturating_sub(1),
                    ActiveView::Containers => app.selected = app.view_len().saturating_sub(1),
                    ActiveView::Images => app.images_selected = app.images_visible_len().saturating_sub(1),
                    ActiveView::Volumes => app.volumes_selected = app.volumes_visible_len().saturating_sub(1),
                    ActiveView::Networks => app.networks_selected = app.networks.len().saturating_sub(1),
                },
                KeyCode::Char(' ') => {
                    if app.active_view == ActiveView::Containers
                        && app.list_mode == ListMode::Tree
                        && app.toggle_tree_expanded_selected()
                    {
                    } else {
                        app.toggle_mark_selected();
                    }
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => app.mark_all(),
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => app.clear_marks(),
                KeyCode::Enter => {
                    if app.active_view == ActiveView::Containers
                        && app.list_mode == ListMode::Tree
                        && app.toggle_tree_expanded_selected()
                    {}
                }
                _ => {}
            }
        }
        ShellView::Templates => {
            if app.shell_focus == ShellFocus::Details {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.templates_state.templates_details_scroll = app.templates_state.templates_details_scroll.saturating_sub(1),
                        KeyCode::Down | KeyCode::Char('j') => app.templates_state.templates_details_scroll += 1,
                        KeyCode::PageUp => app.templates_state.templates_details_scroll = app.templates_state.templates_details_scroll.saturating_sub(10),
                        KeyCode::PageDown => app.templates_state.templates_details_scroll += 10,
                        KeyCode::Home => app.templates_state.templates_details_scroll = 0,
                        KeyCode::End => app.templates_state.templates_details_scroll = usize::MAX,
                        _ => {}
                    },
                    TemplatesKind::Networks => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.templates_state.net_templates_details_scroll = app.templates_state.net_templates_details_scroll.saturating_sub(1),
                        KeyCode::Down | KeyCode::Char('j') => app.templates_state.net_templates_details_scroll += 1,
                        KeyCode::PageUp => app.templates_state.net_templates_details_scroll = app.templates_state.net_templates_details_scroll.saturating_sub(10),
                        KeyCode::PageDown => app.templates_state.net_templates_details_scroll += 10,
                        KeyCode::Home => app.templates_state.net_templates_details_scroll = 0,
                        KeyCode::End => app.templates_state.net_templates_details_scroll = usize::MAX,
                        _ => {}
                    },
                }
            } else {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => {
                        let before = app.templates_state.templates_selected;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => app.templates_state.templates_selected = app.templates_state.templates_selected.saturating_sub(1),
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !app.templates_state.templates.is_empty() {
                                    app.templates_state.templates_selected = (app.templates_state.templates_selected + 1).min(app.templates_state.templates.len() - 1);
                                } else {
                                    app.templates_state.templates_selected = 0;
                                }
                            }
                            KeyCode::PageUp => app.templates_state.templates_selected = app.templates_state.templates_selected.saturating_sub(10),
                            KeyCode::PageDown => {
                                if !app.templates_state.templates.is_empty() {
                                    app.templates_state.templates_selected = (app.templates_state.templates_selected + 10).min(app.templates_state.templates.len() - 1);
                                } else {
                                    app.templates_state.templates_selected = 0;
                                }
                            }
                            KeyCode::Home => app.templates_state.templates_selected = 0,
                            KeyCode::End => app.templates_state.templates_selected = app.templates_state.templates.len().saturating_sub(1),
                            _ => {}
                        }
                        if app.templates_state.templates_selected != before {
                            app.templates_state.templates_details_scroll = 0;
                        }
                    }
                    TemplatesKind::Networks => {
                        let before = app.templates_state.net_templates_selected;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => app.templates_state.net_templates_selected = app.templates_state.net_templates_selected.saturating_sub(1),
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !app.templates_state.net_templates.is_empty() {
                                    app.templates_state.net_templates_selected = (app.templates_state.net_templates_selected + 1).min(app.templates_state.net_templates.len() - 1);
                                } else {
                                    app.templates_state.net_templates_selected = 0;
                                }
                            }
                            KeyCode::PageUp => app.templates_state.net_templates_selected = app.templates_state.net_templates_selected.saturating_sub(10),
                            KeyCode::PageDown => {
                                if !app.templates_state.net_templates.is_empty() {
                                    app.templates_state.net_templates_selected = (app.templates_state.net_templates_selected + 10).min(app.templates_state.net_templates.len() - 1);
                                } else {
                                    app.templates_state.net_templates_selected = 0;
                                }
                            }
                            KeyCode::Home => app.templates_state.net_templates_selected = 0,
                            KeyCode::End => app.templates_state.net_templates_selected = app.templates_state.net_templates.len().saturating_sub(1),
                            _ => {}
                        }
                        if app.templates_state.net_templates_selected != before {
                            app.templates_state.net_templates_details_scroll = 0;
                        }
                    }
                }
            }
        }
        ShellView::Registries => {
            if app.shell_focus == ShellFocus::Details {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.registries_details_scroll = app.registries_details_scroll.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => app.registries_details_scroll += 1,
                    KeyCode::PageUp => app.registries_details_scroll = app.registries_details_scroll.saturating_sub(10),
                    KeyCode::PageDown => app.registries_details_scroll += 10,
                    KeyCode::Home => app.registries_details_scroll = 0,
                    KeyCode::End => app.registries_details_scroll = usize::MAX,
                    _ => {}
                }
            } else {
                let before = app.registries_selected;
                let total = app.registries_cfg.registries.len();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => app.registries_selected = app.registries_selected.saturating_sub(1),
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
        ShellView::Logs => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.logs_move_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.logs_move_down(1),
            KeyCode::PageUp => app.logs_move_up(10),
            KeyCode::PageDown => app.logs_move_down(10),
            KeyCode::Left => app.logs.hscroll = app.logs.hscroll.saturating_sub(4),
            KeyCode::Right => app.logs.hscroll = app.logs.hscroll.saturating_add(4),
            KeyCode::Home => app.logs.cursor = 0,
            KeyCode::End => app.logs.cursor = app.logs_total_lines().saturating_sub(1),
            KeyCode::Esc => {
                if app.logs.select_anchor.is_some() {
                    app.logs_clear_selection();
                }
            }
            KeyCode::Char(' ') => app.logs_toggle_selection(),
            KeyCode::Char('m') => {
                app.logs.use_regex = !app.logs.use_regex;
                app.logs_rebuild_matches();
            }
            KeyCode::Char('l') => app.logs.show_line_numbers = !app.logs.show_line_numbers,
            KeyCode::Char('/') => {
                app.logs.mode = LogsMode::Search;
                app.logs.input = app.logs.query.clone();
                app.logs.input_cursor = app.logs.input.chars().count();
                app.logs_rebuild_matches();
            }
            KeyCode::Char(':') => {
                app.logs.mode = LogsMode::Command;
                app.logs.command.clear();
                app.logs.command_cursor = 0;
                app.logs_rebuild_matches();
            }
            KeyCode::Char('n') => app.logs_next_match(),
            KeyCode::Char('N') => app.logs_prev_match(),
            _ => {}
        },
        ShellView::Inspect => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.inspect_move_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.inspect_move_down(1),
            KeyCode::PageUp => app.inspect_move_up(10),
            KeyCode::PageDown => app.inspect_move_down(10),
            KeyCode::Left => app.inspect.scroll = app.inspect.scroll.saturating_sub(4),
            KeyCode::Right => app.inspect.scroll = app.inspect.scroll.saturating_add(4),
            KeyCode::Home => {
                app.inspect.selected = 0;
                app.inspect.scroll = 0;
            }
            KeyCode::End => {
                if !app.inspect.lines.is_empty() {
                    app.inspect.selected = app.inspect.lines.len() - 1;
                } else {
                    app.inspect.selected = 0;
                }
            }
            KeyCode::Enter => app.inspect_toggle_selected(),
            KeyCode::Char('/') => app.inspect_enter_search(),
            KeyCode::Char(':') => app.inspect_enter_command(),
            KeyCode::Char('n') => app.inspect_jump_next_match(),
            KeyCode::Char('N') => app.inspect_jump_prev_match(),
            _ => {}
        },
        ShellView::Help => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.shell_help.scroll = app.shell_help.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => app.shell_help.scroll = app.shell_help.scroll.saturating_add(1),
            KeyCode::PageUp => app.shell_help.scroll = app.shell_help.scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_help.scroll = app.shell_help.scroll.saturating_add(10),
            KeyCode::Home => app.shell_help.scroll = 0,
            KeyCode::End => app.shell_help.scroll = usize::MAX,
            _ => {}
        },
        ShellView::Messages => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(1),
            KeyCode::PageUp => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_msgs.scroll = app.shell_msgs.scroll.saturating_add(10),
            KeyCode::Left => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_sub(4),
            KeyCode::Right => app.shell_msgs.hscroll = app.shell_msgs.hscroll.saturating_add(4),
            KeyCode::Home => app.shell_msgs.scroll = 0,
            KeyCode::End => app.shell_msgs.scroll = usize::MAX,
            _ => {}
        },
        ShellView::ThemeSelector => {}
    }
}
