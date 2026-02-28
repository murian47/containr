use crate::ui::core::types::StackDetailsFocus;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ActiveView, ListMode, ShellFocus, ShellView};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_main_view_navigation(app: &mut App, key: KeyEvent) {
    if app.shell_focus == ShellFocus::Details {
        handle_main_view_details_navigation(app, key);
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
            ActiveView::Volumes => {
                app.volumes_selected = app.volumes_visible_len().saturating_sub(1)
            }
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

fn handle_main_view_details_navigation(app: &mut App, key: KeyEvent) {
    let stack_name = if app.shell_view == ShellView::Stacks {
        let name = app.selected_stack_entry().map(|s| s.name.clone());
        if let Some(ref n) = name
            && app.stack_network_count(n) == 0
        {
            app.stack_details_focus = StackDetailsFocus::Containers;
        }
        name
    } else {
        None
    };
    let stack_counts =
        if let (ShellView::Stacks, Some(name)) = (app.shell_view, stack_name.as_ref()) {
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
}
