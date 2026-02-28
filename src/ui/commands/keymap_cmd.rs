//! Key mapping commands (`:map ...`, `:unmap ...`).

use super::super::{
    App, KeyCodeNorm, KeyScope, KeySpec, ShellFocus, ShellView, cmdline_is_destructive,
    is_single_letter_without_modifiers, parse_key_spec, parse_scope, scope_to_string,
};
use crate::config::KeyBinding;
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
            "usage: :map [scope] <KEY> <COMMAND...>  |  :map list  |  :unmap [scope] <KEY>",
        );
        return true;
    }

    if sub == "list" {
        // Show effective bindings (defaults + overrides). Mark explicit entries with '*'.
        let mut explicit: HashMap<(KeyScope, KeySpec), String> = HashMap::new();
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
            explicit.insert((scope, spec), cmd);
        }

        let mut keys: HashSet<(KeyScope, KeySpec)> = HashSet::new();
        keys.extend(app.keymap_defaults.keys().copied());
        keys.extend(explicit.keys().copied());

        let mut entries: Vec<(String, String, String, bool)> = Vec::new();
        for (scope, spec) in keys {
            let scope_str = scope_to_string(scope).to_string();
            let key_str = format_key_spec(spec);
            let (cmd, is_explicit) = if let Some(cmd) = explicit.get(&(scope, spec)) {
                if cmd.is_empty() {
                    ("<disabled>".to_string(), true)
                } else {
                    (format!(":{}", cmd), true)
                }
            } else if let Some(cmd) = app.keymap_defaults.get(&(scope, spec)) {
                (format!(":{}", cmd), false)
            } else {
                ("<disabled>".to_string(), false)
            };
            entries.push((scope_str, key_str, cmd, is_explicit));
        }
        entries.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));

        if entries.is_empty() {
            app.set_info("no key bindings configured");
        } else {
            app.set_info("Key bindings (* = configured/overridden):");
            for (scope, key, cmd, explicit) in entries {
                let star = if explicit { "*" } else { " " };
                app.set_info(format!("{star} {scope:<13} {key:<12} -> {cmd}"));
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
    let (scope, key_str, cmd_rest) = if let Some(scope) = parse_scope(sub) {
        let Some(key_str) = rest.first().copied() else {
            app.set_warn("usage: :map [scope] <KEY> <COMMAND...>");
            return true;
        };
        let cmd_rest = rest
            .iter()
            .skip(1)
            .copied()
            .collect::<Vec<&str>>()
            .join(" ");
        (scope, key_str, cmd_rest)
    } else {
        let cmd_rest = rest.to_vec().join(" ");
        (KeyScope::Global, sub, cmd_rest)
    };
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
    } else {
        app.keymap.push(KeyBinding {
            key: key_canon.clone(),
            scope: scope_str.clone(),
            cmd: cmd_store,
        });
    }
    app.rebuild_keymap();
    app.persist_config();
    app.set_info(format!("mapped {scope_str} {key_canon}"));
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
