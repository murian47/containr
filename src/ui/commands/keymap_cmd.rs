//! Key mapping commands (`:map ...`, `:unmap ...`).

use crate::config::KeyBinding;
use crate::ui::core::key_types::{
    KeyCodeNorm, KeyScope, KeySpec, parse_key_spec, parse_scope, scope_to_string,
};
use crate::ui::core::keymap::{cmdline_is_destructive, is_single_letter_without_modifiers};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellFocus, ShellView};
use std::collections::{HashMap, HashSet};

fn format_key_spec(spec: KeySpec) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    if (spec.mods & 1) != 0 {
        parts.push("C");
    }
    if (spec.mods & 2) != 0 {
        parts.push("S");
    }
    if (spec.mods & 4) != 0 {
        parts.push("A");
    }
    let key = match spec.code {
        KeyCodeNorm::Char(' ') => "Space".to_string(),
        KeyCodeNorm::Char(',') => ",".to_string(),
        KeyCodeNorm::Char(c) => c.to_string(),
        KeyCodeNorm::F(n) => format!("F{n}"),
        KeyCodeNorm::Enter => "Enter".to_string(),
        KeyCodeNorm::Esc => "Esc".to_string(),
        KeyCodeNorm::Tab => "Tab".to_string(),
        KeyCodeNorm::Backspace => "Backspace".to_string(),
        KeyCodeNorm::Delete => "Delete".to_string(),
        KeyCodeNorm::Home => "Home".to_string(),
        KeyCodeNorm::End => "End".to_string(),
        KeyCodeNorm::PageUp => "PageUp".to_string(),
        KeyCodeNorm::PageDown => "PageDown".to_string(),
        KeyCodeNorm::Up => "Up".to_string(),
        KeyCodeNorm::Down => "Down".to_string(),
        KeyCodeNorm::Left => "Left".to_string(),
        KeyCodeNorm::Right => "Right".to_string(),
    };
    if parts.is_empty() {
        key
    } else {
        format!("{}-{}", parts.join("-"), key)
    }
}

pub(in crate::ui) fn handle_map(app: &mut App, first: &str, rest: &[&str]) -> bool {
    let sub = first;
    if sub.is_empty() {
        app.set_warn(
            "usage: :map [scope] <KEY> <COMMAND...> [--sidebar] [--label <text>]  |  :map list  |  :unmap [scope] <KEY>",
        );
        return true;
    }

    if sub == "list" {
        // Show effective bindings (defaults + overrides). Mark explicit entries with '*'.
        let mut explicit: HashMap<(KeyScope, KeySpec), (String, bool, Option<String>)> =
            HashMap::new();
        let mut unsafe_entries: Vec<(String, String, String)> = Vec::new();
        for kb in &app.keymap {
            let Some(scope) = parse_scope(&kb.scope) else {
                continue;
            };
            let Ok(spec) = parse_key_spec(&kb.key) else {
                continue;
            };
            let cmd = kb.cmd.trim().trim_start_matches(':').to_string();
            if !cmd.is_empty()
                && is_single_letter_without_modifiers(spec)
                && cmdline_is_destructive(&cmd)
            {
                unsafe_entries.push((
                    scope_to_string(scope).to_string(),
                    format_key_spec(spec),
                    kb.cmd.trim().to_string(),
                ));
                continue;
            }
            explicit.insert(
                (scope, spec),
                (cmd, kb.show_in_sidebar, kb.sidebar_label.clone()),
            );
        }

        let mut keys: HashSet<(KeyScope, KeySpec)> = HashSet::new();
        keys.extend(app.keymap_defaults.keys().copied());
        keys.extend(explicit.keys().copied());

        let mut entries: Vec<(String, String, String, bool, bool, Option<String>)> = Vec::new();
        for (scope, spec) in keys {
            let scope_str = scope_to_string(scope).to_string();
            let key_str = format_key_spec(spec);
            let (cmd, is_explicit, show_in_sidebar, sidebar_label) =
                if let Some((cmd, show_in_sidebar, sidebar_label)) = explicit.get(&(scope, spec)) {
                    if cmd.is_empty() {
                        (
                            "<disabled>".to_string(),
                            true,
                            *show_in_sidebar,
                            sidebar_label.clone(),
                        )
                    } else {
                        (
                            format!(":{}", cmd),
                            true,
                            *show_in_sidebar,
                            sidebar_label.clone(),
                        )
                    }
                } else if let Some(cmd) = app.keymap_defaults.get(&(scope, spec)) {
                    (format!(":{}", cmd), false, false, None)
                } else {
                    ("<disabled>".to_string(), false, false, None)
                };
            entries.push((
                scope_str,
                key_str,
                cmd,
                is_explicit,
                show_in_sidebar,
                sidebar_label,
            ));
        }
        entries.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));

        if entries.is_empty() {
            app.set_info("no key bindings configured");
        } else {
            app.set_info("Key bindings (* = configured/overridden, S = shown in sidebar):");
            for (scope, key, cmd, explicit, show_in_sidebar, sidebar_label) in entries {
                let star = if explicit { "*" } else { " " };
                let side = if show_in_sidebar { "S" } else { " " };
                let label_hint = sidebar_label
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!(" label={s}"))
                    .unwrap_or_default();
                app.set_info(format!(
                    "{star}{side} {scope:<13} {key:<12} -> {cmd}{label_hint}"
                ));
            }
            for (scope, key, cmd) in unsafe_entries {
                app.set_info(format!(
                    "* INVALID {scope:<8} {key:<12} -> {cmd}  (destructive commands require a modifier)"
                ));
            }
        }
        app.shell_msgs.return_view = app.shell_view;
        app.shell_view = ShellView::Messages;
        app.shell_focus = ShellFocus::List;
        app.shell_msgs.scroll = usize::MAX;
        return true;
    }

    // Syntax: :map [scope] <KEY> <CMD...>
    let (scope, key_str, cmd_tokens) = if let Some(scope) = parse_scope(sub) {
        let Some(key_str) = rest.first().copied() else {
            app.set_warn("usage: :map [scope] <KEY> <COMMAND...>");
            return true;
        };
        let cmd_tokens = rest.iter().skip(1).copied().collect::<Vec<&str>>();
        (scope, key_str, cmd_tokens)
    } else {
        let cmd_tokens = rest.to_vec();
        (KeyScope::Global, sub, cmd_tokens)
    };
    let mut show_in_sidebar: Option<bool> = None;
    let mut sidebar_label: Option<String> = None;
    let mut cmd_parts: Vec<&str> = Vec::new();
    let mut idx = 0usize;
    while idx < cmd_tokens.len() {
        let part = cmd_tokens[idx];
        match part {
            "--sidebar" => show_in_sidebar = Some(true),
            "--label" | "--sidebar-label" => {
                let Some(value) = cmd_tokens.get(idx + 1).copied() else {
                    app.set_warn("usage: :map ... --label <text>");
                    return true;
                };
                sidebar_label = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
                idx = idx.saturating_add(1);
            }
            _ => cmd_parts.push(part),
        }
        idx = idx.saturating_add(1);
    }
    let cmd_rest = cmd_parts.join(" ");
    if cmd_rest.trim().is_empty() {
        app.set_warn("usage: :map [scope] <KEY> <COMMAND...>");
        return true;
    }
    let spec = match parse_key_spec(key_str) {
        Ok(s) => s,
        Err(e) => {
            app.set_warn(format!("invalid key: {e}"));
            return true;
        }
    };

    let scope_str = scope_to_string(scope).to_string();
    let key_canon = format_key_spec(spec);
    let cmd_store = cmd_rest.trim().trim_start_matches(':').to_string();
    if is_single_letter_without_modifiers(spec) && cmdline_is_destructive(&cmd_store) {
        app.set_warn("destructive commands require a modifier (Ctrl/Shift/Alt)");
        return true;
    }

    // Replace existing entry if present, otherwise insert.
    if let Some(kb) = app.keymap.iter_mut().find(|kb| {
        parse_scope(&kb.scope) == Some(scope) && parse_key_spec(&kb.key).ok() == Some(spec)
    }) {
        kb.cmd = cmd_store;
        if let Some(v) = show_in_sidebar {
            kb.show_in_sidebar = v;
        }
        if sidebar_label.is_some() {
            kb.sidebar_label = sidebar_label.clone();
        }
    } else {
        app.keymap.push(KeyBinding {
            key: key_canon.clone(),
            scope: scope_str.clone(),
            cmd: cmd_store,
            show_in_sidebar: show_in_sidebar.unwrap_or(false),
            sidebar_label,
        });
    }
    app.rebuild_keymap();
    app.persist_config();
    app.set_info(format!(
        "mapped {scope_str} {key_canon}{}",
        if show_in_sidebar == Some(true) {
            " (sidebar)"
        } else {
            ""
        }
    ));
    true
}

pub(in crate::ui) fn handle_unmap(app: &mut App, first: &str, rest: &[&str]) -> bool {
    if first.is_empty() {
        app.set_warn("usage: :unmap [scope] <KEY>");
        return true;
    }
    let (scope, key_str) = if let Some(scope) = parse_scope(first) {
        let Some(key_str) = rest.first().copied() else {
            app.set_warn("usage: :unmap [scope] <KEY>");
            return true;
        };
        (scope, key_str)
    } else {
        (KeyScope::Global, first)
    };

    let spec = match parse_key_spec(key_str) {
        Ok(s) => s,
        Err(e) => {
            app.set_warn(format!("invalid key: {e}"));
            return true;
        }
    };
    let scope_str = scope_to_string(scope).to_string();
    let key_canon = format_key_spec(spec);

    let mut removed = false;
    let before = app.keymap.len();
    app.keymap.retain(|kb| {
        let same =
            parse_scope(&kb.scope) == Some(scope) && parse_key_spec(&kb.key).ok() == Some(spec);
        if same {
            removed = true;
        }
        !same
    });

    // If there was no explicit mapping, insert a disable marker to override defaults.
    if !removed {
        app.keymap.push(KeyBinding {
            key: key_canon.clone(),
            scope: scope_str.clone(),
            cmd: String::new(),
            show_in_sidebar: false,
            sidebar_label: None,
        });
    }
    app.rebuild_keymap();
    app.persist_config();
    if removed && app.keymap.len() < before {
        app.set_info(format!(
            "unmapped {scope_str} {key_canon} (restored defaults)"
        ));
    } else {
        app.set_info(format!("unmapped {scope_str} {key_canon}"));
    }
    app.shell_msgs.return_view = app.shell_view;
    app.shell_view = ShellView::Messages;
    app.shell_focus = ShellFocus::List;
    app.shell_msgs.scroll = usize::MAX;
    true
}
