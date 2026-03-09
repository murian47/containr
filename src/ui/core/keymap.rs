use crate::ui::core::key_types::{
    KeyCodeNorm, KeySpec, build_default_keymap, parse_key_spec, parse_scope,
};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::MsgLevel;
use crate::ui::state::shell_types::ShellView;
use std::collections::HashMap;

pub(in crate::ui) fn is_single_letter_without_modifiers(spec: KeySpec) -> bool {
    spec.mods == 0 && matches!(spec.code, KeyCodeNorm::Char(c) if c.is_ascii_alphabetic())
}

pub(in crate::ui) fn cmdline_is_destructive(raw: &str) -> bool {
    let s = raw.trim().trim_start_matches(':').trim();
    if s.is_empty() {
        return false;
    }
    let mut it = s.split_whitespace();
    let Some(cmd_raw) = it.next() else {
        return false;
    };
    let cmd_raw = cmd_raw
        .strip_prefix('!')
        .unwrap_or(cmd_raw)
        .trim_matches('!');
    let cmd = cmd_raw.to_ascii_lowercase();
    let sub = it.next().unwrap_or("").to_ascii_lowercase();
    match cmd.as_str() {
        "stack" | "stacks" | "stk" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "container" | "ctr" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "template" | "tpl" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "nettemplate" | "nettpl" | "ntpl" | "nt" => {
            matches!(sub.as_str(), "rm" | "remove" | "delete")
        }
        "theme" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "server" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "image" | "img" => matches!(sub.as_str(), "rm" | "remove" | "delete" | "untag"),
        "volume" | "vol" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "network" | "net" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        _ => false,
    }
}

impl App {
    fn add_default_binding(
        defaults: &mut HashMap<(crate::ui::core::key_types::KeyScope, KeySpec), String>,
        scope: crate::ui::core::key_types::KeyScope,
        key: &str,
        cmd: &str,
    ) {
        if let Ok(spec) = parse_key_spec(key) {
            defaults.insert((scope, spec), cmd.trim_start_matches(':').to_string());
        }
    }

    fn add_dynamic_navigation_defaults(&mut self) {
        use crate::ui::core::key_types::KeyScope;

        // Module shortcuts are active in all views except logs/inspect to preserve existing UX.
        let module_scopes = [
            KeyScope::View(ShellView::Dashboard),
            KeyScope::View(ShellView::Stacks),
            KeyScope::View(ShellView::Containers),
            KeyScope::View(ShellView::Images),
            KeyScope::View(ShellView::Volumes),
            KeyScope::View(ShellView::Networks),
            KeyScope::View(ShellView::Templates),
            KeyScope::View(ShellView::Registries),
            KeyScope::View(ShellView::Messages),
            KeyScope::View(ShellView::Help),
            KeyScope::View(ShellView::ThemeSelector),
        ];
        let module_keys = [
            ("d", "view dashboard"),
            ("s", "view stacks"),
            ("c", "view containers"),
            ("i", "view images"),
            ("v", "view volumes"),
            ("n", "view networks"),
            ("t", "view templates"),
            ("r", "view registries"),
        ];
        for scope in module_scopes {
            for (key, cmd) in module_keys {
                Self::add_default_binding(&mut self.keymap_defaults, scope, key, cmd);
            }
        }

        // Server shortcuts (1..9 and generated letter hints) are global.
        // Keep them out of cmdline capture, because command-line input is handled before
        // scoped/global bindings but after always-bindings.
        for (i, hint) in self.shell_server_shortcuts.iter().copied().enumerate() {
            if hint == '\0' {
                continue;
            }
            let cmd = format!("server use-index {}", i + 1);
            if hint.is_ascii_alphabetic() {
                let lower = hint.to_ascii_lowercase();
                Self::add_default_binding(
                    &mut self.keymap_defaults,
                    KeyScope::Global,
                    &lower.to_string(),
                    &cmd,
                );
                let upper = hint.to_ascii_uppercase();
                Self::add_default_binding(
                    &mut self.keymap_defaults,
                    KeyScope::Global,
                    &format!("S-{upper}"),
                    &cmd,
                );
            } else {
                Self::add_default_binding(
                    &mut self.keymap_defaults,
                    KeyScope::Global,
                    &hint.to_string(),
                    &cmd,
                );
            }
        }
    }

    pub(in crate::ui) fn rebuild_keymap(&mut self) {
        self.keymap_parsed.clear();
        self.keymap_defaults = build_default_keymap();
        self.add_dynamic_navigation_defaults();
        let mut invalid: Vec<(String, String)> = Vec::new();
        for kb in &self.keymap {
            let cmd = kb.cmd.trim();
            let scope = match parse_scope(&kb.scope) {
                Some(s) => s,
                None => {
                    invalid.push((kb.scope.clone(), "invalid scope".to_string()));
                    continue;
                }
            };
            match parse_key_spec(&kb.key) {
                Ok(spec) => {
                    if is_single_letter_without_modifiers(spec) && cmdline_is_destructive(cmd) {
                        invalid.push((
                            format!("{} {}", kb.scope.trim(), kb.key.trim()),
                            "refusing to bind destructive command to unmodified single letter"
                                .to_string(),
                        ));
                        continue;
                    }
                    // Empty cmd means "disabled" for this key in this scope.
                    self.keymap_parsed
                        .insert((scope, spec), cmd.trim_start_matches(':').to_string());
                }
                Err(e) => {
                    invalid.push((kb.key.clone(), e));
                }
            }
        }
        for (key, err) in invalid {
            self.log_msg(
                MsgLevel::Warn,
                format!("invalid key binding '{key}': {err}"),
            );
        }
    }
}
