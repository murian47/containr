fn key_spec_from_event(key: crossterm::event::KeyEvent) -> Option<KeySpec> {
    let mut mods: u8 = 0;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        mods |= 1;
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        mods |= 2;
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        mods |= 4;
    }

    let code = match key.code {
        KeyCode::Char(c) => KeyCodeNorm::Char(c),
        KeyCode::F(n) => KeyCodeNorm::F(n),
        KeyCode::Enter => KeyCodeNorm::Enter,
        KeyCode::Esc => KeyCodeNorm::Esc,
        KeyCode::Tab => KeyCodeNorm::Tab,
        KeyCode::Backspace => KeyCodeNorm::Backspace,
        KeyCode::Delete => KeyCodeNorm::Delete,
        KeyCode::Home => KeyCodeNorm::Home,
        KeyCode::End => KeyCodeNorm::End,
        KeyCode::PageUp => KeyCodeNorm::PageUp,
        KeyCode::PageDown => KeyCodeNorm::PageDown,
        KeyCode::Up => KeyCodeNorm::Up,
        KeyCode::Down => KeyCodeNorm::Down,
        KeyCode::Left => KeyCodeNorm::Left,
        KeyCode::Right => KeyCodeNorm::Right,
        _ => return None,
    };
    Some(KeySpec { mods, code })
}

fn parse_key_spec(s: &str) -> Result<KeySpec, String> {
    let raw = s.trim();
    if raw.is_empty() {
        return Err("empty key".to_string());
    }

    let normalized = raw.replace('+', "-");
    let parts: Vec<&str> = normalized.split('-').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("empty key".to_string());
    }

    // Avoid ambiguity between single-letter keys and our modifier shorthands (C/S/A).
    // A plain "a" should mean the "a" key, not "Alt-" with a missing key part.
    if parts.len() == 1 {
        let kp_u = parts[0].trim().to_ascii_uppercase();
        if let Some(n_str) = kp_u.strip_prefix('F') {
            if let Ok(n) = n_str.parse::<u8>() {
                if (1..=24).contains(&n) {
                    return Ok(KeySpec {
                        mods: 0,
                        code: KeyCodeNorm::F(n),
                    });
                }
            }
        }
        return match kp_u.as_str() {
            "ENTER" | "RET" | "RETURN" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Enter,
            }),
            "ESC" | "ESCAPE" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Esc,
            }),
            "TAB" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Tab,
            }),
            "BS" | "BACKSPACE" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Backspace,
            }),
            "DEL" | "DELETE" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Delete,
            }),
            "HOME" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Home,
            }),
            "END" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::End,
            }),
            "PGUP" | "PAGEUP" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::PageUp,
            }),
            "PGDN" | "PAGEDOWN" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::PageDown,
            }),
            "UP" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Up,
            }),
            "DOWN" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Down,
            }),
            "LEFT" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Left,
            }),
            "RIGHT" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Right,
            }),
            "SPACE" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Char(' '),
            }),
            "COMMA" => Ok(KeySpec {
                mods: 0,
                code: KeyCodeNorm::Char(','),
            }),
            _ => {
                let mut chars = parts[0].chars();
                let Some(ch) = chars.next() else {
                    return Err("missing key".to_string());
                };
                if chars.next().is_some() {
                    return Err(format!("invalid key: {}", parts[0]));
                }
                Ok(KeySpec {
                    mods: 0,
                    code: KeyCodeNorm::Char(ch),
                })
            }
        };
    }

    // Multi-part spec: treat all but the last part as modifiers, and the last part as the key.
    // This keeps the syntax unambiguous and allows chords like "C-S-C" (Ctrl+Shift+C).
    let mut mods: u8 = 0;
    let (mods_parts, key_part) = parts.split_at(parts.len().saturating_sub(1));
    let key_part = key_part.first().ok_or_else(|| "missing key".to_string())?;

    for p in mods_parts {
        let p_lc = p.to_ascii_lowercase();
        // Modifiers are accepted case-insensitively for words ("ctrl", "shift", ...).
        // For single-letter shorthands we require uppercase to avoid ambiguity with keys.
        match (*p, p_lc.as_str()) {
            ("C", _) | (_, "ctrl") | (_, "control") | (_, "strg") => mods |= 1,
            ("S", _) | (_, "shift") => mods |= 2,
            ("A", _) | (_, "alt") => mods |= 4,
            (_, "cmd") | (_, "meta") | (_, "super") => {
                return Err("CMD/META/SUPER is not supported by terminals via crossterm".to_string());
            }
            _ => return Err(format!("invalid modifier: {p}")),
        }
    }
    let kp_u = key_part.to_ascii_uppercase();
    // F-keys: allow with modifiers too (e.g. C-F5).
    if let Some(n_str) = kp_u.strip_prefix('F') {
        if let Ok(n) = n_str.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Ok(KeySpec {
                    mods,
                    code: KeyCodeNorm::F(n),
                });
            }
        }
    }

    let code = match kp_u.as_str() {
        "ENTER" | "RET" | "RETURN" => KeyCodeNorm::Enter,
        "ESC" | "ESCAPE" => KeyCodeNorm::Esc,
        "TAB" => KeyCodeNorm::Tab,
        "BS" | "BACKSPACE" => KeyCodeNorm::Backspace,
        "DEL" | "DELETE" => KeyCodeNorm::Delete,
        "HOME" => KeyCodeNorm::Home,
        "END" => KeyCodeNorm::End,
        "PGUP" | "PAGEUP" => KeyCodeNorm::PageUp,
        "PGDN" | "PAGEDOWN" => KeyCodeNorm::PageDown,
        "UP" => KeyCodeNorm::Up,
        "DOWN" => KeyCodeNorm::Down,
        "LEFT" => KeyCodeNorm::Left,
        "RIGHT" => KeyCodeNorm::Right,
        "SPACE" => KeyCodeNorm::Char(' '),
        "COMMA" => KeyCodeNorm::Char(','),
        _ => {
            // Single character.
            let mut chars = key_part.chars();
            let Some(ch) = chars.next() else {
                return Err("missing key".to_string());
            };
            if chars.next().is_some() {
                return Err(format!("invalid key: {key_part}"));
            }
            let ch = if (mods & 2) != 0 && ch.is_ascii_alphabetic() {
                ch.to_ascii_uppercase()
            } else if (mods & 2) == 0 && ch.is_ascii_alphabetic() {
                ch.to_ascii_lowercase()
            } else {
                ch
            };
            KeyCodeNorm::Char(ch)
        }
    };
    Ok(KeySpec { mods, code })
}

fn parse_scope(raw: &str) -> Option<KeyScope> {
    let s = raw.trim().to_ascii_lowercase();
    if s == "always" || s == "allways" || s == "any" {
        return Some(KeyScope::Always);
    }
    if s.is_empty() || s == "global" {
        return Some(KeyScope::Global);
    }
    if let Some(v) = s.strip_prefix("view:") {
        return parse_view_name(v).map(KeyScope::View);
    }
    // Convenience: allow bare view names.
    parse_view_name(&s).map(KeyScope::View)
}

fn parse_view_name(s: &str) -> Option<ShellView> {
    match s {
        "dashboard" | "dash" => Some(ShellView::Dashboard),
        "stacks" | "stack" | "stk" => Some(ShellView::Stacks),
        "containers" | "container" | "ctr" => Some(ShellView::Containers),
        "images" | "image" | "img" => Some(ShellView::Images),
        "volumes" | "volume" | "vol" => Some(ShellView::Volumes),
        "networks" | "network" | "net" => Some(ShellView::Networks),
        "templates" | "template" | "tpl" => Some(ShellView::Templates),
        "registries" | "registry" | "reg" => Some(ShellView::Registries),
        "themes" | "theme" => Some(ShellView::ThemeSelector),
        // Backward compatibility for earlier experiments.
        "nettemplates" | "nettemplate" | "nettpl" | "ntpl" | "nt" => Some(ShellView::Templates),
        "logs" | "log" => Some(ShellView::Logs),
        "inspect" => Some(ShellView::Inspect),
        "messages" | "msgs" => Some(ShellView::Messages),
        "help" => Some(ShellView::Help),
        _ => None,
    }
}

fn scope_to_string(scope: KeyScope) -> &'static str {
    match scope {
        KeyScope::Always => "always",
        KeyScope::Global => "global",
        KeyScope::View(ShellView::Dashboard) => "view:dashboard",
        KeyScope::View(ShellView::Stacks) => "view:stacks",
        KeyScope::View(ShellView::Containers) => "view:containers",
        KeyScope::View(ShellView::Images) => "view:images",
        KeyScope::View(ShellView::Volumes) => "view:volumes",
        KeyScope::View(ShellView::Networks) => "view:networks",
        KeyScope::View(ShellView::Templates) => "view:templates",
        KeyScope::View(ShellView::Registries) => "view:registries",
        KeyScope::View(ShellView::Logs) => "view:logs",
        KeyScope::View(ShellView::Inspect) => "view:inspect",
        KeyScope::View(ShellView::Messages) => "view:messages",
        KeyScope::View(ShellView::Help) => "view:help",
        KeyScope::View(ShellView::ThemeSelector) => "view:themes",
    }
}

fn build_default_keymap() -> HashMap<(KeyScope, KeySpec), String> {
    // Defaults are built-in and can be overridden by user keymap.
    let mut out: HashMap<(KeyScope, KeySpec), String> = HashMap::new();
    let mut add = |scope: KeyScope, key: &str, cmd: &str| {
        if let Ok(spec) = parse_key_spec(key) {
            out.insert((scope, spec), cmd.trim_start_matches(':').to_string());
        }
    };

    // Global UI.
    add(KeyScope::Global, "F5", ":refresh");
    add(KeyScope::Always, "F1", ":help");
    add(KeyScope::Global, "C-b", ":sidebar toggle");
    add(KeyScope::Global, "b", ":sidebar compact");
    add(KeyScope::Global, "C-p", ":layout toggle");
    add(KeyScope::Global, "C-g", ":messages");

    // Containers.
    let c = KeyScope::View(ShellView::Containers);
    add(c, "C-s", ":container start");
    add(c, "C-o", ":container stop");
    add(c, "C-r", ":container restart");
    add(c, "C-d", ":container rm");
    add(c, "C-c", ":container console bash");
    add(c, "C-S-C", ":container console sh");
    add(c, "C-t", ":container tree");
    add(c, "C-i", ":inspect");
    add(c, "C-l", ":logs");

    // Stacks.
    let s = KeyScope::View(ShellView::Stacks);
    add(s, "C-s", ":stack start");
    add(s, "C-o", ":stack stop");
    add(s, "C-r", ":stack restart");
    add(s, "C-d", ":stack rm");
    add(s, "C-u", ":stack update");
    add(s, "C-S-U", ":stack update --all");

    // Images.
    let i = KeyScope::View(ShellView::Images);
    add(i, "C-i", ":inspect");
    add(i, "C-d", ":image rm");

    // Networks.
    let n = KeyScope::View(ShellView::Networks);
    add(n, "C-i", ":inspect");
    add(n, "C-d", ":network rm");

    // Volumes.
    let v = KeyScope::View(ShellView::Volumes);
    add(v, "C-i", ":inspect");
    add(v, "C-d", ":volume rm");

    // Templates.
    let t = KeyScope::View(ShellView::Templates);
    add(t, "C-e", ":template edit");
    add(t, "C-n", ":template new");
    add(t, "C-y", ":template deploy");
    add(t, "C-d", ":template rm");
    add(t, "C-t", ":templates toggle");
    add(t, "C-a", ":ai");

    // Registries.
    let r = KeyScope::View(ShellView::Registries);
    add(r, "C-y", ":registry test");

    // Logs.
    let l = KeyScope::View(ShellView::Logs);
    add(l, "C-l", ":logs reload");
    add(l, "C-c", ":logs copy");

    // Messages.
    let m = KeyScope::View(ShellView::Messages);
    add(m, "C-c", ":messages copy");

    out
}

enum BindingHit {
    Disabled,
    Cmd(String),
}

fn lookup_binding(app: &App, scope: KeyScope, spec: KeySpec) -> Option<BindingHit> {
    if let Some(cmd) = app.keymap_parsed.get(&(scope, spec)) {
        if cmd.trim().is_empty() {
            return Some(BindingHit::Disabled);
        }
        return Some(BindingHit::Cmd(cmd.clone()));
    }
    if let Some(cmd) = app.keymap_defaults.get(&(scope, spec)) {
        return Some(BindingHit::Cmd(cmd.clone()));
    }
    None
}

fn lookup_scoped_binding(app: &App, spec: KeySpec) -> Option<BindingHit> {
    let view_scope = KeyScope::View(app.shell_view);
    lookup_binding(app, KeyScope::Always, spec)
        .or_else(|| lookup_binding(app, view_scope, spec))
        .or_else(|| lookup_binding(app, KeyScope::Global, spec))
}
