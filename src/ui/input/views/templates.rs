use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, TemplatesKind};
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_templates_navigation(app: &mut App, key: KeyEvent) {
    if app.shell_focus == ShellFocus::Details {
        handle_templates_details_navigation(app, key);
    } else {
        handle_templates_list_navigation(app, key);
    }
}

fn handle_templates_details_navigation(app: &mut App, key: KeyEvent) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.templates_state.templates_details_scroll = app
                    .templates_state
                    .templates_details_scroll
                    .saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => app.templates_state.templates_details_scroll += 1,
            KeyCode::PageUp => {
                app.templates_state.templates_details_scroll = app
                    .templates_state
                    .templates_details_scroll
                    .saturating_sub(10)
            }
            KeyCode::PageDown => app.templates_state.templates_details_scroll += 10,
            KeyCode::Home => app.templates_state.templates_details_scroll = 0,
            KeyCode::End => app.templates_state.templates_details_scroll = usize::MAX,
            _ => {}
        },
        TemplatesKind::Networks => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.templates_state.net_templates_details_scroll = app
                    .templates_state
                    .net_templates_details_scroll
                    .saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.templates_state.net_templates_details_scroll += 1
            }
            KeyCode::PageUp => {
                app.templates_state.net_templates_details_scroll = app
                    .templates_state
                    .net_templates_details_scroll
                    .saturating_sub(10)
            }
            KeyCode::PageDown => app.templates_state.net_templates_details_scroll += 10,
            KeyCode::Home => app.templates_state.net_templates_details_scroll = 0,
            KeyCode::End => app.templates_state.net_templates_details_scroll = usize::MAX,
            _ => {}
        },
    }
}

fn handle_templates_list_navigation(app: &mut App, key: KeyEvent) {
    match app.templates_state.kind {
        TemplatesKind::Stacks => {
            let before = app.templates_state.templates_selected;
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    app.templates_state.templates_selected =
                        app.templates_state.templates_selected.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !app.templates_state.templates.is_empty() {
                        app.templates_state.templates_selected =
                            (app.templates_state.templates_selected + 1)
                                .min(app.templates_state.templates.len() - 1);
                    } else {
                        app.templates_state.templates_selected = 0;
                    }
                }
                KeyCode::PageUp => {
                    app.templates_state.templates_selected =
                        app.templates_state.templates_selected.saturating_sub(10)
                }
                KeyCode::PageDown => {
                    if !app.templates_state.templates.is_empty() {
                        app.templates_state.templates_selected =
                            (app.templates_state.templates_selected + 10)
                                .min(app.templates_state.templates.len() - 1);
                    } else {
                        app.templates_state.templates_selected = 0;
                    }
                }
                KeyCode::Home => app.templates_state.templates_selected = 0,
                KeyCode::End => {
                    app.templates_state.templates_selected =
                        app.templates_state.templates.len().saturating_sub(1)
                }
                _ => {}
            }
            if app.templates_state.templates_selected != before {
                app.templates_state.templates_details_scroll = 0;
            }
        }
        TemplatesKind::Networks => {
            let before = app.templates_state.net_templates_selected;
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    app.templates_state.net_templates_selected =
                        app.templates_state.net_templates_selected.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !app.templates_state.net_templates.is_empty() {
                        app.templates_state.net_templates_selected =
                            (app.templates_state.net_templates_selected + 1)
                                .min(app.templates_state.net_templates.len() - 1);
                    } else {
                        app.templates_state.net_templates_selected = 0;
                    }
                }
                KeyCode::PageUp => {
                    app.templates_state.net_templates_selected = app
                        .templates_state
                        .net_templates_selected
                        .saturating_sub(10)
                }
                KeyCode::PageDown => {
                    if !app.templates_state.net_templates.is_empty() {
                        app.templates_state.net_templates_selected =
                            (app.templates_state.net_templates_selected + 10)
                                .min(app.templates_state.net_templates.len() - 1);
                    } else {
                        app.templates_state.net_templates_selected = 0;
                    }
                }
                KeyCode::Home => app.templates_state.net_templates_selected = 0,
                KeyCode::End => {
                    app.templates_state.net_templates_selected =
                        app.templates_state.net_templates.len().saturating_sub(1)
                }
                _ => {}
            }
            if app.templates_state.net_templates_selected != before {
                app.templates_state.net_templates_details_scroll = 0;
            }
        }
    }
}
