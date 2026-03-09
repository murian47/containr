use crate::ui::core::key_types::{
    BindingHit, KeyScope, lookup_scoped_binding, parse_key_spec, parse_scope,
};
use crate::ui::core::view::shell_module_shortcut;
use crate::ui::render::text::truncate_end;
use crate::ui::render::utils::{draw_focus_accent, shell_row_highlight};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellAction, ShellFocus, ShellSidebarItem, ShellView};
use crate::ui::theme;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(in crate::ui) struct SidebarShortcut {
    pub(in crate::ui) key: String,
    pub(in crate::ui) key_hint: String,
    pub(in crate::ui) cmd: String,
    pub(in crate::ui) label: String,
}

fn sidebar_key_hint(key: &str) -> String {
    let Ok(spec) = parse_key_spec(key) else {
        return key.to_string();
    };
    match (spec.mods, spec.code) {
        (1, crate::ui::core::key_types::KeyCodeNorm::Char(c)) => format!("^{c}"),
        (3, crate::ui::core::key_types::KeyCodeNorm::Char(c)) => {
            format!("^{}", c.to_ascii_uppercase())
        }
        _ => key.to_string(),
    }
}

pub(in crate::ui) fn shell_sidebar_shortcuts(app: &App) -> Vec<SidebarShortcut> {
    #[derive(Clone)]
    struct MarkedShortcut {
        scope: KeyScope,
        key: String,
        label: Option<String>,
    }

    let marked: Vec<MarkedShortcut> = app
        .keymap
        .iter()
        .filter(|kb| kb.show_in_sidebar)
        .filter_map(|kb| {
            let scope = parse_scope(&kb.scope)?;
            let _spec = parse_key_spec(&kb.key).ok()?;
            let scope_active = match scope {
                KeyScope::Always | KeyScope::Global => true,
                KeyScope::View(v) => v == app.shell_view,
            };
            if !scope_active {
                return None;
            }
            let cmd = kb.cmd.trim();
            if cmd.is_empty() {
                return None;
            }
            Some(MarkedShortcut {
                scope,
                key: kb.key.trim().to_string(),
                label: kb
                    .sidebar_label
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(ToString::to_string),
            })
        })
        .collect();

    let mut key_scopes: HashMap<_, Vec<KeyScope>> = HashMap::new();
    for m in &marked {
        if let Ok(spec) = parse_key_spec(&m.key) {
            key_scopes.entry(spec).or_default().push(m.scope);
        }
    }

    let mut out: Vec<SidebarShortcut> = Vec::new();
    for (spec, scopes) in key_scopes {
        let collision = scopes.len() > 1;
        let scope_tag = match [
            KeyScope::Always,
            KeyScope::View(app.shell_view),
            KeyScope::Global,
        ]
        .into_iter()
        .find_map(|scope| {
            if scopes.contains(&scope) {
                Some(match scope {
                    KeyScope::Always => "A",
                    KeyScope::View(_) => "V",
                    KeyScope::Global => "G",
                })
            } else {
                None
            }
        }) {
            Some(tag) => tag,
            None => continue,
        };
        let hit = lookup_scoped_binding(app, spec);
        let Some(maybe_cmd) = (match hit {
            Some(BindingHit::Disabled) => Some(None),
            Some(BindingHit::Cmd(cmd)) => Some(Some(cmd)),
            None => None,
        }) else {
            continue;
        };
        let Some(cmd) = maybe_cmd else {
            continue;
        };
        let Some(marked_for_key) = marked
            .iter()
            .find(|m| parse_key_spec(&m.key).ok() == Some(spec))
        else {
            continue;
        };
        let mut label = marked_for_key
            .label
            .clone()
            .unwrap_or_else(|| cmd.trim_start_matches(':').to_string());
        if collision {
            label.push_str(&format!(" [{scope_tag}]"));
        }
        out.push(SidebarShortcut {
            key: marked_for_key.key.clone(),
            key_hint: sidebar_key_hint(&marked_for_key.key),
            cmd: format!(":{}", cmd.trim_start_matches(':')),
            label,
        });
    }

    out.sort_by(|a, b| a.key.cmp(&b.key));
    out.dedup_by(|a, b| a.key == b.key && a.cmd == b.cmd && a.label == b.label);
    out
}

pub(in crate::ui) fn shell_sidebar_items(app: &App) -> Vec<ShellSidebarItem> {
    let mut items: Vec<ShellSidebarItem> = Vec::new();
    for i in 0..app.servers.len() {
        items.push(ShellSidebarItem::Server(i));
    }
    items.push(ShellSidebarItem::Separator);
    items.push(ShellSidebarItem::Module(ShellView::Dashboard));
    items.push(ShellSidebarItem::Module(ShellView::Stacks));
    items.push(ShellSidebarItem::Module(ShellView::Containers));
    items.push(ShellSidebarItem::Module(ShellView::Images));
    items.push(ShellSidebarItem::Module(ShellView::Volumes));
    items.push(ShellSidebarItem::Module(ShellView::Networks));
    items.push(ShellSidebarItem::Gap);
    items.push(ShellSidebarItem::Module(ShellView::Templates));
    items.push(ShellSidebarItem::Module(ShellView::Registries));
    // Help is accessible via :? / :help (not a module entry).

    let actions: Vec<ShellAction> = match app.shell_view {
        ShellView::Dashboard => vec![],
        ShellView::Stacks => vec![
            ShellAction::Start,
            ShellAction::Stop,
            ShellAction::Restart,
            ShellAction::Delete,
            ShellAction::StackUpdate,
            ShellAction::StackUpdateAll,
        ],
        ShellView::Containers => vec![
            ShellAction::Inspect,
            ShellAction::Logs,
            ShellAction::Start,
            ShellAction::Stop,
            ShellAction::Restart,
            ShellAction::Delete,
            ShellAction::Console,
        ],
        ShellView::Images => vec![
            ShellAction::Inspect,
            ShellAction::ImageUntag,
            ShellAction::ImageForceRemove,
        ],
        ShellView::Volumes => vec![ShellAction::Inspect, ShellAction::VolumeRemove],
        ShellView::Networks => vec![ShellAction::Inspect, ShellAction::NetworkRemove],
        ShellView::Templates => vec![
            ShellAction::TemplateEdit,
            ShellAction::TemplateNew,
            ShellAction::TemplateDelete,
            ShellAction::TemplateDeploy,
            ShellAction::TemplateRedeploy,
        ],
        ShellView::Registries => vec![ShellAction::RegistryTest],
        ShellView::Inspect | ShellView::Logs | ShellView::Help => vec![],
        ShellView::Messages | ShellView::ThemeSelector => vec![],
    };
    if !actions.is_empty() {
        items.push(ShellSidebarItem::Separator);
        for a in actions {
            items.push(ShellSidebarItem::Action(a));
        }
    }
    let shortcuts = shell_sidebar_shortcuts(app);
    if !shortcuts.is_empty() {
        items.push(ShellSidebarItem::Separator);
        for (idx, _) in shortcuts.iter().enumerate() {
            items.push(ShellSidebarItem::Shortcut(idx));
        }
    }
    items
}

fn shell_is_selectable(item: ShellSidebarItem) -> bool {
    !matches!(item, ShellSidebarItem::Separator | ShellSidebarItem::Gap)
}

pub(in crate::ui) fn shell_move_sidebar(app: &mut App, dir: i32) {
    let items = shell_sidebar_items(app);
    if items.is_empty() {
        app.shell_sidebar_selected = 0;
        return;
    }
    let mut idx = app.shell_sidebar_selected.min(items.len() - 1);
    for _ in 0..items.len() {
        if dir < 0 {
            idx = idx.saturating_sub(1);
        } else {
            idx = (idx + 1).min(items.len() - 1);
        }
        if shell_is_selectable(items[idx]) {
            app.shell_sidebar_selected = idx;
            return;
        }
        if idx == 0 || idx == items.len() - 1 {
            break;
        }
    }
    app.shell_sidebar_selected = idx;
}

pub(in crate::ui) fn shell_sidebar_select_item(app: &mut App, target: ShellSidebarItem) {
    let items = shell_sidebar_items(app);
    if let Some((idx, _)) = items
        .iter()
        .enumerate()
        .find(|(_, it)| **it == target && shell_is_selectable(**it))
    {
        app.shell_sidebar_selected = idx;
    }
}

pub(in crate::ui) fn draw_shell_sidebar(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner_area = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let inner_w = inner_area.width.max(1) as usize;

    let items = shell_sidebar_items(app);
    let shortcuts = shell_sidebar_shortcuts(app);
    let mut rendered: Vec<ListItem> = Vec::new();
    for (idx, it) in items.iter().enumerate() {
        let selected = app.shell_focus == ShellFocus::Sidebar && idx == app.shell_sidebar_selected;
        let st = if selected {
            shell_row_highlight(app)
        } else {
            bg
        };

        match *it {
            ShellSidebarItem::Separator => {
                let base_bg = theme::parse_color(&app.theme.panel.bg);
                let divider_style = app.theme.divider.to_style().bg(base_bg);
                rendered.push(ListItem::new(Line::from(Span::styled(
                    "─".repeat(inner_w),
                    divider_style,
                ))));
            }
            ShellSidebarItem::Gap => {
                rendered.push(ListItem::new(Line::from(Span::styled(" ".to_string(), bg))));
            }
            ShellSidebarItem::Server(i) => {
                let name = app.servers.get(i).map(|s| s.name.as_str()).unwrap_or("?");
                let base = format!(" {name}");
                let active_style = app.theme.active.to_style();
                if app.shell_sidebar_collapsed {
                    let st = if !selected && i == app.server_selected {
                        active_style
                    } else {
                        st
                    };
                    rendered.push(ListItem::new(Line::from(Span::styled(base, st))));
                } else {
                    let hint = app.shell_server_shortcuts.get(i).copied().unwrap_or('?');
                    let hint = format!("[{hint}]");
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let base_style = if !selected && i == app.server_selected {
                        active_style
                    } else {
                        st
                    };
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(Color::White)
                    } else {
                        bg.fg(theme::parse_color(&app.theme.text_dim.fg))
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
            ShellSidebarItem::Module(v) => {
                let name = v.title();
                let base = format!(" {name}");
                let active_style = app.theme.active.to_style();
                if app.shell_sidebar_collapsed {
                    let base_style = if !selected && v == app.shell_view {
                        active_style
                    } else {
                        st
                    };
                    rendered.push(ListItem::new(Line::from(Span::styled(base, base_style))));
                } else {
                    let hint = shell_module_shortcut(v);
                    let hint = format!("[{hint}]");
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let base_style = if !selected && v == app.shell_view {
                        active_style
                    } else {
                        st
                    };
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(theme::parse_color(&app.theme.panel.fg))
                    } else {
                        bg.patch(app.theme.text_dim.to_style())
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
            ShellSidebarItem::Action(a) => {
                let label = a.label();
                let base = format!(" {label}");
                let base_style = if selected {
                    shell_row_highlight(app)
                } else {
                    bg.patch(app.theme.text.to_style())
                };
                if app.shell_sidebar_collapsed {
                    rendered.push(ListItem::new(Line::from(Span::styled(base, base_style))));
                } else {
                    let hint = format!("[{}]", a.ctrl_hint());
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(theme::parse_color(&app.theme.panel.fg))
                    } else {
                        bg.patch(app.theme.text_dim.to_style())
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
            ShellSidebarItem::Shortcut(i) => {
                let Some(sc) = shortcuts.get(i) else {
                    continue;
                };
                let base = format!(" {}", sc.label);
                let base_style = if selected {
                    shell_row_highlight(app)
                } else {
                    bg.patch(app.theme.text.to_style())
                };
                if app.shell_sidebar_collapsed {
                    rendered.push(ListItem::new(Line::from(Span::styled(base, base_style))));
                } else {
                    let hint = format!("[{}]", sc.key_hint);
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(theme::parse_color(&app.theme.panel.fg))
                    } else {
                        bg.patch(app.theme.text_dim.to_style())
                    };
                    rendered.push(ListItem::new(Line::from(vec![
                        Span::styled(base_shown, base_style),
                        Span::styled(" ".repeat(gap), base_style),
                        Span::styled(hint, hint_style),
                    ])));
                }
            }
        }
    }
    if rendered.is_empty() {
        rendered.push(ListItem::new(Line::from("")));
    }
    let mut state = ListState::default();
    state.select(Some(
        app.shell_sidebar_selected
            .min(rendered.len().saturating_sub(1)),
    ));
    let list = List::new(rendered).highlight_symbol("").style(bg);
    f.render_stateful_widget(list, inner_area, &mut state);
    draw_focus_accent(f, app, area, app.shell_focus == ShellFocus::Sidebar);
}
