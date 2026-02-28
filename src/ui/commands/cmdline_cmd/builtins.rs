use super::CmdlineCtx;
use super::{git_cmd, keymap_cmd, theme_cmd};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellView, TemplatesKind, shell_begin_confirm};

pub(super) fn handle_builtin_cmd<'a>(
    app: &mut App,
    cmd: &str,
    force: bool,
    it: &mut impl Iterator<Item = &'a str>,
    cmdline_full: &str,
    ctx: &CmdlineCtx<'_>,
) -> bool {
    match cmd {
        "q" => {
            if force {
                app.should_quit = true;
            } else {
                app.shell_cmdline.mode = true;
                app.shell_cmdline.input.clear();
                app.shell_cmdline.cursor = 0;
                app.shell_cmdline.confirm = Some(crate::ui::core::requests::ShellConfirm {
                    label: "quit".to_string(),
                    cmdline: cmdline_full.to_string(),
                });
            }
            true
        }
        "?" | "help" => {
            app.shell_cmdline.mode = false;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            app.shell_help.return_view = app.shell_view;
            app.shell_view = ShellView::Help;
            app.shell_focus = ShellFocus::List;
            app.shell_help.scroll = 0;
            true
        }
        "messages" | "msgs" => {
            let sub = it.next().unwrap_or("");
            if sub == "copy" {
                app.messages_copy_selected();
                return true;
            }
            let (save_force, wants_save) = if sub == "save!" {
                (true, true)
            } else if sub == "save" {
                (false, true)
            } else {
                (false, false)
            };
            if wants_save {
                let rest: Vec<&str> = it.collect();
                let path = rest.join(" ").trim().to_string();
                if path.is_empty() {
                    app.set_warn("usage: :messages save <file>");
                } else {
                    app.messages_save(&path, save_force);
                }
                return true;
            }
            app.shell_cmdline.mode = false;
            app.shell_cmdline.confirm = None;
            app.shell_cmdline.input.clear();
            app.shell_cmdline.cursor = 0;
            if app.shell_view == ShellView::Messages {
                app.back_from_full_view();
            } else {
                app.mark_messages_seen();
                app.shell_msgs.return_view = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs.scroll = usize::MAX;
                app.shell_msgs.hscroll = 0;
            }
            true
        }
        "log" => {
            let sub = it.next().unwrap_or("");
            if sub != "dock" {
                app.set_warn("usage: :log dock");
                return true;
            }
            let height_raw = it.next().unwrap_or("");
            if !height_raw.is_empty() {
                match height_raw.parse::<u16>() {
                    Ok(h) if (3..=12).contains(&h) => {
                        app.log_dock_height = h;
                        app.log_dock_enabled = true;
                    }
                    _ => {
                        app.set_warn("usage: :log dock [3..12]");
                        return true;
                    }
                }
            } else {
                app.log_dock_enabled = !app.log_dock_enabled;
            }
            if app.log_dock_enabled {
                app.shell_msgs.scroll = usize::MAX;
                app.shell_msgs.hscroll = 0;
            }
            if app.shell_view == ShellView::Messages {
                app.back_from_full_view();
            } else if !app.log_dock_enabled && app.shell_focus == ShellFocus::Dock {
                app.shell_focus = ShellFocus::List;
            }
            app.persist_config();
            true
        }
        "ack" => {
            let sub = it.next().unwrap_or("");
            if sub == "all" {
                app.container_action_error.clear();
                app.image_action_error.clear();
                app.volume_action_error.clear();
                app.network_action_error.clear();
                app.template_action_error.clear();
                app.net_template_action_error.clear();
                app.conn_error = None;
                app.last_error = None;
                app.dashboard.error = None;
                app.logs.error = None;
                app.inspect.error = None;
                app.refresh_error_streak = 0;
                app.refresh_pause_reason = None;
                app.mark_messages_seen();
                app.set_info("cleared all action error markers");
                return true;
            }
            match app.shell_view {
                ShellView::Dashboard | ShellView::Stacks => {}
                ShellView::Containers => {
                    let ids: Vec<String> = if !app.marked.is_empty() {
                        app.marked.iter().cloned().collect()
                    } else {
                        app.selected_container()
                            .map(|c| vec![c.id.clone()])
                            .unwrap_or_default()
                    };
                    for id in ids {
                        app.container_action_error.remove(&id);
                    }
                }
                ShellView::Images => {
                    let keys: Vec<String> = if !app.marked_images.is_empty() {
                        app.marked_images.iter().cloned().collect()
                    } else {
                        app.selected_image()
                            .map(|img| vec![App::image_row_key(img)])
                            .unwrap_or_default()
                    };
                    for k in keys {
                        app.image_action_error.remove(&k);
                    }
                }
                ShellView::Volumes => {
                    let names: Vec<String> = if !app.marked_volumes.is_empty() {
                        app.marked_volumes.iter().cloned().collect()
                    } else {
                        app.selected_volume()
                            .map(|v| vec![v.name.clone()])
                            .unwrap_or_default()
                    };
                    for n in names {
                        app.volume_action_error.remove(&n);
                    }
                }
                ShellView::Networks => {
                    let ids: Vec<String> = if !app.marked_networks.is_empty() {
                        app.marked_networks.iter().cloned().collect()
                    } else {
                        app.selected_network()
                            .map(|n| vec![n.id.clone()])
                            .unwrap_or_default()
                    };
                    for id in ids {
                        app.network_action_error.remove(&id);
                    }
                }
                ShellView::Templates => match app.templates_state.kind {
                    TemplatesKind::Stacks => {
                        if let Some(name) = app.selected_template().map(|t| t.name.clone()) {
                            app.template_action_error.remove(&name);
                        }
                    }
                    TemplatesKind::Networks => {
                        if let Some(name) = app.selected_net_template().map(|t| t.name.clone()) {
                            app.net_template_action_error.remove(&name);
                        }
                    }
                },
                ShellView::Logs
                | ShellView::Inspect
                | ShellView::Help
                | ShellView::Messages
                | ShellView::Registries
                | ShellView::ThemeSelector => {}
            }
            app.set_info("cleared action error marker(s) for selection");
            true
        }
        "refresh" => {
            if app.shell_view == ShellView::Templates {
                match app.templates_state.kind {
                    TemplatesKind::Stacks => app.refresh_templates(),
                    TemplatesKind::Networks => app.refresh_net_templates(),
                }
            } else {
                app.refresh_now(
                    ctx.refresh_tx,
                    ctx.dash_refresh_tx,
                    ctx.dash_all_refresh_tx,
                    ctx.refresh_pause_tx,
                );
            }
            true
        }
        "theme" => {
            let sub = it.next().unwrap_or("");
            if sub.is_empty() || sub == "help" {
                app.set_info(format!("active theme: {}", app.theme_name));
                app.set_info("usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>");
                if sub.is_empty() {
                    return true;
                }
            }
            match sub {
                "list" => app.open_theme_selector(),
                "use" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme use <name>");
                        return true;
                    };
                    if let Err(e) = theme_cmd::set_theme(app, name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                "new" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme new <name>");
                        return true;
                    };
                    if let Err(e) = theme_cmd::new_theme(app, name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                "edit" => {
                    let name = it
                        .next()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| app.theme_name.clone());
                    if let Err(e) = theme_cmd::edit_theme(app, &name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                "rm" | "del" | "delete" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme rm <name>");
                        return true;
                    };
                    if !force {
                        shell_begin_confirm(
                            app,
                            format!("theme rm {name}"),
                            cmdline_full.to_string(),
                        );
                        return true;
                    }
                    if let Err(e) = theme_cmd::delete_theme(app, name) {
                        app.set_error(format!("{e:#}"));
                    }
                }
                _ => {
                    app.set_warn(
                        "usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>",
                    );
                }
            }
            true
        }
        "git" => {
            let args: Vec<&str> = it.collect();
            let _ = git_cmd::handle_git(app, &args);
            true
        }
        "map" | "bind" => {
            let first = it.next().unwrap_or("");
            let rest: Vec<&str> = it.collect();
            let _ = keymap_cmd::handle_map(app, first, &rest);
            true
        }
        "unmap" | "unbind" => {
            let first = it.next().unwrap_or("");
            let rest: Vec<&str> = it.collect();
            let _ = keymap_cmd::handle_unmap(app, first, &rest);
            true
        }
        _ => false,
    }
}
