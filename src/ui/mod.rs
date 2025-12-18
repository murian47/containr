//! Terminal UI (TUI) entrypoint and rendering.
//!
//! High-level architecture:
//! - `run_tui` owns the terminal lifecycle and the event loop.
//! - `App` is the in-memory state machine for views, selections, and input modes.
//! - Background tasks (ssh/local docker commands) run asynchronously and feed results back via channels.
//! - Rendering reads from `App` and draws the current UI state.
//!
//! When refactoring, keep these boundaries:
//! - IO/runner functions should not mutate UI widgets directly.
//! - UI code should use semantic theme roles (`theme::ThemeSpec`) instead of hard-coded colors.

pub mod theme;
mod commands;

use crate::config::{self, ContainrConfig, KeyBinding, ServerEntry};
use crate::docker::{
    self, ContainerAction, ContainerRow, DockerCfg, ImageRow, NetworkRow, VolumeRow,
};
use crate::runner::Runner;
use crate::ssh::Ssh;
use anyhow::Context as _;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Cell, List, ListItem, ListState, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
};
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};
use std::{collections::HashMap, collections::HashSet, fmt::Write as _};
use tokio::sync::mpsc;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct KeySpec {
    mods: u8, // bitmask: 1=Ctrl 2=Shift 4=Alt
    code: KeyCodeNorm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyScope {
    Always,
    Global,
    View(ShellView),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyCodeNorm {
    Char(char),
    F(u8),
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Default, Clone)]
struct CmdHistory {
    entries: Vec<String>,
    pos: Option<usize>,
    saved_current: String,
}

impl CmdHistory {
    fn new() -> Self {
        Self::default()
    }

    fn reset_nav(&mut self) {
        self.pos = None;
        self.saved_current.clear();
    }

    fn on_edit(&mut self) {
        if self.pos.is_some() {
            self.reset_nav();
        }
    }

    fn push(&mut self, cmd: &str, max: usize) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if self.entries.last().is_some_and(|x| x == cmd) {
            return;
        }
        self.entries.push(cmd.to_string());
        let max = max.max(1);
        if self.entries.len() > max {
            let drain = self.entries.len() - max;
            self.entries.drain(0..drain);
        }
    }

    fn prev(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let len = self.entries.len();
        let pos = match self.pos {
            None => {
                self.saved_current = current.to_string();
                len - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.pos = Some(pos);
        self.entries.get(pos).cloned()
    }

    fn next(&mut self) -> Option<String> {
        let Some(pos) = self.pos else {
            return None;
        };
        let len = self.entries.len();
        if pos + 1 >= len {
            self.pos = None;
            return Some(std::mem::take(&mut self.saved_current));
        }
        let pos = pos + 1;
        self.pos = Some(pos);
        self.entries.get(pos).cloned()
    }
}

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

    let mut mods: u8 = 0;
    let mut key_part: Option<&str> = None;
    for p in parts {
        let p_lc = p.to_ascii_lowercase();
        // Modifiers are accepted case-insensitively for words ("ctrl", "shift", ...).
        // For single-letter shorthands we require uppercase to avoid ambiguity with keys
        // like "C-s" where the "s" should be the key, not Shift.
        match (p, p_lc.as_str()) {
            ("C", _) | (_, "ctrl") | (_, "control") | (_, "strg") => mods |= 1,
            ("S", _) | (_, "shift") => mods |= 2,
            ("A", _) | (_, "alt") => mods |= 4,
            (_, "cmd") | (_, "meta") | (_, "super") => {
                return Err(
                    "CMD/META/SUPER is not supported by terminals via crossterm".to_string()
                );
            }
            _ => {
                key_part = Some(p);
            }
        }
    }

    let key_part = key_part.ok_or_else(|| "missing key".to_string())?;
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
        "containers" | "container" | "ctr" => Some(ShellView::Containers),
        "images" | "image" | "img" => Some(ShellView::Images),
        "volumes" | "volume" | "vol" => Some(ShellView::Volumes),
        "networks" | "network" | "net" => Some(ShellView::Networks),
        "templates" | "template" | "tpl" => Some(ShellView::Templates),
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
        KeyScope::View(ShellView::Containers) => "view:containers",
        KeyScope::View(ShellView::Images) => "view:images",
        KeyScope::View(ShellView::Volumes) => "view:volumes",
        KeyScope::View(ShellView::Networks) => "view:networks",
        KeyScope::View(ShellView::Templates) => "view:templates",
        KeyScope::View(ShellView::Logs) => "view:logs",
        KeyScope::View(ShellView::Inspect) => "view:inspect",
        KeyScope::View(ShellView::Messages) => "view:messages",
        KeyScope::View(ShellView::Help) => "view:help",
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

    // Networks.
    let n = KeyScope::View(ShellView::Networks);
    add(n, "C-d", ":network rm");

    // Templates.
    let t = KeyScope::View(ShellView::Templates);
    add(t, "Enter", ":template edit");
    add(t, "C-e", ":template edit");
    add(t, "C-n", ":template new");
    add(t, "C-y", ":template deploy");
    add(t, "C-d", ":template rm");
    add(t, "C-t", ":templates toggle");

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

fn shell_begin_confirm(app: &mut App, label: impl Into<String>, cmdline: impl Into<String>) {
    app.shell_cmd_mode = true;
    app.shell_cmd_input.clear();
    app.shell_cmd_cursor = 0;
    app.shell_confirm = Some(ShellConfirm {
        label: label.into(),
        cmdline: cmdline.into(),
    });
}

fn clamp_cursor_to_text(text: &str, cursor: usize) -> usize {
    cursor.min(text.chars().count())
}

fn insert_char_at_cursor(text: &mut String, cursor: &mut usize, ch: char) {
    let mut out = String::new();
    let mut idx = 0usize;
    let cur = clamp_cursor_to_text(text, *cursor);
    for c in text.chars() {
        if idx == cur {
            out.push(ch);
        }
        out.push(c);
        idx += 1;
    }
    if idx == cur {
        out.push(ch);
    }
    *text = out;
    *cursor = cur.saturating_add(1);
}

fn backspace_at_cursor(text: &mut String, cursor: &mut usize) {
    let cur = clamp_cursor_to_text(text, *cursor);
    if cur == 0 {
        *cursor = 0;
        return;
    }
    let target = cur - 1;
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i != target {
            out.push(c);
        }
    }
    *text = out;
    *cursor = target;
}

fn delete_at_cursor(text: &mut String, cursor: &mut usize) {
    let cur = clamp_cursor_to_text(text, *cursor);
    let len = text.chars().count();
    if cur >= len {
        *cursor = len;
        return;
    }
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i != cur {
            out.push(c);
        }
    }
    *text = out;
    *cursor = cur.min(text.chars().count());
}

fn set_text_and_cursor(text: &mut String, cursor: &mut usize, new_text: String) {
    *text = new_text;
    *cursor = text.chars().count();
}

fn input_window_with_cursor(text: &str, cursor: usize, width: usize) -> (String, String, String) {
    // Returns (before_cursor, cursor_cell, after_cursor) of the visible window.
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let cursor = cursor.min(len);

    if len <= width {
        let before: String = chars.iter().take(cursor).collect();
        let at = if cursor < len {
            chars[cursor].to_string()
        } else {
            " ".to_string()
        };
        let after: String = chars.iter().skip(cursor.saturating_add(1)).collect();
        return (before, at, after);
    }

    let mut start = 0usize;
    if cursor >= width {
        start = cursor - width + 1;
    }
    if start + width > len {
        start = len - width;
    }
    let end = (start + width).min(len);
    let rel = cursor.saturating_sub(start).min(end - start);
    let before: String = chars[start..start + rel].iter().collect();
    let at = if start + rel < end {
        chars[start + rel].to_string()
    } else {
        " ".to_string()
    };
    let after_start = (start + rel + 1).min(end);
    let after: String = chars[after_start..end].iter().collect();
    (before, at, after)
}

fn is_single_letter_without_modifiers(spec: KeySpec) -> bool {
    spec.mods == 0 && matches!(spec.code, KeyCodeNorm::Char(c) if c.is_ascii_alphabetic())
}

fn cmdline_is_destructive(raw: &str) -> bool {
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
        "container" | "ctr" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "template" | "tpl" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "nettemplate" | "nettpl" | "ntpl" | "nt" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "theme" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "server" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "image" | "img" => matches!(sub.as_str(), "rm" | "remove" | "delete" | "untag"),
        "volume" | "vol" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        "network" | "net" => matches!(sub.as_str(), "rm" | "remove" | "delete"),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TemplatesKind {
    Stacks,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListMode {
    Flat,
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveView {
    Containers,
    Images,
    Volumes,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ShellView {
    Containers,
    Images,
    Volumes,
    Networks,
    Templates,
    Inspect,
    Logs,
    Help,
    Messages,
}

impl ShellView {
    fn slug(self) -> &'static str {
        match self {
            ShellView::Containers => "containers",
            ShellView::Images => "images",
            ShellView::Volumes => "volumes",
            ShellView::Networks => "networks",
            ShellView::Templates => "templates",
            ShellView::Inspect => "inspect",
            ShellView::Logs => "logs",
            ShellView::Help => "help",
            ShellView::Messages => "messages",
        }
    }

    fn title(self) -> &'static str {
        match self {
            ShellView::Containers => "Containers",
            ShellView::Images => "Images",
            ShellView::Volumes => "Volumes",
            ShellView::Networks => "Networks",
            ShellView::Templates => "Templates",
            ShellView::Inspect => "Inspect",
            ShellView::Logs => "Logs",
            ShellView::Help => "Help",
            ShellView::Messages => "Messages",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellFocus {
    Sidebar,
    List,
    Details,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellSplitMode {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellSidebarItem {
    Separator,
    Gap,
    Server(usize),
    Module(ShellView),
    Action(ShellAction),
}

#[derive(Clone, Debug)]
enum ShellInteractive {
    RunCommand { cmd: String },
    RunLocalCommand { cmd: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MsgLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
struct SessionMsg {
    at: Duration,
    level: MsgLevel,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellAction {
    Start,
    Stop,
    Restart,
    Delete,
    Console,
    ImageUntag,
    ImageForceRemove,
    VolumeRemove,
    NetworkRemove,
    TemplateEdit,
    TemplateNew,
    TemplateDelete,
    TemplateDeploy,
}

impl ShellAction {
    fn label(self) -> &'static str {
        match self {
            ShellAction::Start => "Start",
            ShellAction::Stop => "Stop",
            ShellAction::Restart => "Restart",
            ShellAction::Delete => "Delete",
            ShellAction::Console => "Console",
            ShellAction::ImageUntag => "Untag",
            ShellAction::ImageForceRemove => "Remove",
            ShellAction::VolumeRemove => "Remove",
            ShellAction::NetworkRemove => "Remove",
            ShellAction::TemplateEdit => "Edit",
            ShellAction::TemplateNew => "New",
            ShellAction::TemplateDelete => "Delete",
            ShellAction::TemplateDeploy => "Deploy",
        }
    }

    fn ctrl_hint(self) -> &'static str {
        match self {
            ShellAction::Start => "^s",
            ShellAction::Stop => "^o",
            ShellAction::Restart => "^r",
            ShellAction::Delete => "^d",
            // Console: ^c = bash, ^C = sh (Ctrl+Shift+C)
            ShellAction::Console => "^c",
            // Non-container actions: keep a separate chord to avoid ambiguity
            ShellAction::ImageUntag => "^u",
            ShellAction::ImageForceRemove => "^d",
            ShellAction::VolumeRemove => "^d",
            ShellAction::NetworkRemove => "^d",
            ShellAction::TemplateEdit => "^e",
            ShellAction::TemplateNew => "^n",
            ShellAction::TemplateDelete => "^d",
            ShellAction::TemplateDeploy => "^y",
        }
    }
}

fn shell_module_shortcut(view: ShellView) -> char {
    match view {
        ShellView::Containers => 'c',
        ShellView::Images => 'm',
        ShellView::Volumes => 'v',
        ShellView::Networks => 'n',
        ShellView::Templates => 't',
        ShellView::Inspect => 'i',
        ShellView::Logs => 'l',
        ShellView::Help => '?',
        // Not a primary module; used only for internal navigation/help display.
        ShellView::Messages => 'g',
    }
}

fn build_server_shortcuts(servers: &[ServerEntry]) -> Vec<char> {
    // First 1..9 use digits. Remaining use deterministic "random-looking" uppercase letters.
    let mut out: Vec<char> = Vec::with_capacity(servers.len());
    let mut used: HashSet<char> = HashSet::new();

    for (i, _) in servers.iter().enumerate() {
        if i < 9 {
            let ch = char::from_digit((i + 1) as u32, 10).unwrap_or('?');
            out.push(ch);
            used.insert(ch);
        } else {
            out.push('\0');
        }
    }

    // Avoid letters that could be confused with common module letters in uppercase.
    for ch in ['C', 'M', 'I', 'V', 'N', 'L'] {
        used.insert(ch);
    }

    let pool: Vec<char> = ('A'..='Z').filter(|c| !used.contains(c)).collect();
    if pool.is_empty() {
        for i in 9..servers.len() {
            out[i] = 'A';
        }
        return out;
    }

    // Stable assignment based on server name.
    for i in 9..servers.len() {
        let name = &servers[i].name;
        let mut h: u64 = 0xcbf29ce484222325;
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let start = (h as usize) % pool.len();
        let mut chosen = None;
        for off in 0..pool.len() {
            let c = pool[(start + off) % pool.len()];
            if !used.contains(&c) {
                chosen = Some(c);
                break;
            }
        }
        let c = chosen.unwrap_or(pool[start]);
        out[i] = c;
        used.insert(c);
    }
    out
}

#[derive(Clone, Debug)]
enum ViewEntry {
    StackHeader {
        name: String,
        total: usize,
        running: usize,
        expanded: bool,
    },
    UngroupedHeader {
        total: usize,
        running: usize,
    },
    Container {
        id: String,
        indent: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogsMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectKind {
    Container,
    Image,
    Volume,
    Network,
}

#[derive(Debug, Clone)]
struct InspectTarget {
    kind: InspectKind,
    key: String,
    arg: String,
    label: String,
}

#[derive(Debug, Clone)]
struct InspectLine {
    path: String,     // JSON pointer
    depth: usize,     // indentation
    label: String,    // key/index label (already printable)
    summary: String,  // preview text (no newlines)
    expandable: bool, // object/array
    expanded: bool,   // current state
    matches: bool,    // search match
}

#[derive(Clone, Debug)]
struct TemplateEntry {
    name: String,
    dir: PathBuf,
    compose_path: PathBuf,
    has_compose: bool,
    desc: String,
}

#[derive(Clone, Debug)]
struct NetTemplateEntry {
    name: String,
    dir: PathBuf,
    cfg_path: PathBuf,
    has_cfg: bool,
    desc: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct NetworkTemplateIpv4 {
    subnet: Option<String>,
    gateway: Option<String>,
    #[serde(rename = "ip_range")]
    ip_range: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct NetworkTemplateSpec {
    name: String,
    #[allow(dead_code)]
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default, rename = "ipvlan_mode")]
    ipvlan_mode: Option<String>,
    #[serde(default)]
    internal: Option<bool>,
    #[serde(default)]
    attachable: Option<bool>,
    #[serde(default)]
    ipv4: Option<NetworkTemplateIpv4>,
    #[serde(default)]
    options: Option<HashMap<String, String>>,
    #[serde(default)]
    labels: Option<HashMap<String, String>>,
}

#[derive(Clone, Copy, Debug)]
struct DeployMarker {
    started: Instant,
}

#[derive(Clone, Copy, Debug)]
struct ActionMarker {
    action: ContainerAction,
    until: Instant,
}

#[derive(Clone, Copy, Debug)]
struct SimpleMarker {
    until: Instant,
}

#[derive(Debug, Clone)]
enum ActionRequest {
    Container {
        action: ContainerAction,
        id: String,
    },
    TemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_compose: PathBuf,
    },
    NetTemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_cfg: PathBuf,
        force: bool,
    },
    ImageUntag {
        marker_key: String,
        reference: String,
    },
    ImageForceRemove {
        marker_key: String,
        id: String,
    },
    VolumeRemove {
        name: String,
    },
    NetworkRemove {
        id: String,
    },
}

struct App {
    containers: Vec<ContainerRow>,
    images: Vec<ImageRow>,
    volumes: Vec<VolumeRow>,
    networks: Vec<NetworkRow>,
    image_referenced_by_id: HashMap<String, bool>,
    image_referenced_count_by_id: HashMap<String, usize>,
    image_running_count_by_id: HashMap<String, usize>,
    volume_referenced_by_name: HashMap<String, bool>,
    volume_referenced_count_by_name: HashMap<String, usize>,
    volume_running_count_by_name: HashMap<String, usize>,
    volume_containers_by_name: HashMap<String, Vec<String>>,
    images_unused_only: bool,
    volumes_unused_only: bool,
    usage_refresh_needed: bool,
    usage_loading: bool,
    selected: usize,
    active_view: ActiveView,
    list_mode: ListMode,
    view: Vec<ViewEntry>,
    view_dirty: bool,
    stack_collapsed: HashSet<String>,
    container_idx_by_id: HashMap<String, usize>,
    marked: HashSet<String>,          // container ids
    marked_images: HashSet<String>,   // image row keys (ref:repo:tag or id:<sha256..>)
    marked_volumes: HashSet<String>,  // volume names
    marked_networks: HashSet<String>, // network ids
    images_selected: usize,
    volumes_selected: usize,
    networks_selected: usize,
    last_refresh: Option<Instant>,
    conn_error: Option<String>,
    last_error: Option<String>,
    loading: bool,
    loading_since: Option<Instant>,
    action_inflight: HashMap<String, ActionMarker>,
    image_action_inflight: HashMap<String, SimpleMarker>,
    volume_action_inflight: HashMap<String, SimpleMarker>,
    network_action_inflight: HashMap<String, SimpleMarker>,
    inspect_loading: bool,
    inspect_error: Option<String>,
    inspect_value: Option<Value>,
    inspect_target: Option<InspectTarget>,
    inspect_for_id: Option<String>,
    inspect_lines: Vec<InspectLine>,
    inspect_selected: usize,
    inspect_scroll_top: usize,
    inspect_scroll: usize,
    inspect_query: String,
    inspect_expanded: HashSet<String>,
    inspect_match_paths: Vec<String>,
    inspect_path_rank: HashMap<String, usize>,
	inspect_mode: InspectMode,
	inspect_input: String,
	inspect_input_cursor: usize,
	inspect_cmd_history: CmdHistory,

    servers: Vec<ServerEntry>,
    active_server: Option<String>,
    server_selected: usize,
    config_path: std::path::PathBuf,
    current_target: String,

    logs_loading: bool,
    logs_error: Option<String>,
    logs_text: Option<String>,
    logs_for_id: Option<String>,
    logs_tail: usize,
    logs_cursor: usize,
    logs_scroll_top: usize,
    logs_select_anchor: Option<usize>,
    logs_hscroll: usize,
    logs_max_width: usize,
	logs_mode: LogsMode,
	logs_input: String,
	logs_query: String,
	logs_command: String,
	logs_input_cursor: usize,
	logs_command_cursor: usize,
	logs_cmd_history: CmdHistory,
    logs_use_regex: bool,
    logs_regex: Option<Regex>,
    logs_regex_error: Option<String>,
    logs_match_lines: Vec<usize>,
    logs_show_line_numbers: bool,

    mouse_enabled: bool,

    ip_cache: HashMap<String, (String, Instant)>,
    ip_refresh_needed: bool,
    should_quit: bool,
    ascii_only: bool,

    theme_name: String,
    theme: theme::ThemeSpec,

    shell_view: ShellView,
    shell_last_main_view: ShellView,
    shell_focus: ShellFocus,
    shell_sidebar_collapsed: bool,
    shell_sidebar_hidden: bool,
    shell_sidebar_selected: usize,
    shell_split_mode: ShellSplitMode,
    shell_split_by_view: HashMap<String, ShellSplitMode>,
    shell_server_shortcuts: Vec<char>,
	shell_pending_interactive: Option<ShellInteractive>,
	shell_cmd_mode: bool,
	shell_cmd_input: String,
	shell_cmd_cursor: usize,
	shell_confirm: Option<ShellConfirm>,
	shell_cmd_history: CmdHistory,
    shell_help_scroll: usize,
    shell_help_return: ShellView,
    refresh_secs: u64,
    cmd_history_max: usize,

    session_start: Instant,
    session_msgs: Vec<SessionMsg>,
    shell_msgs_scroll: usize, // cursor (absolute); usize::MAX = last
    shell_msgs_hscroll: usize,
    shell_msgs_return: ShellView,

    keymap: Vec<KeyBinding>,
    keymap_parsed: HashMap<(KeyScope, KeySpec), String>,
    keymap_defaults: HashMap<(KeyScope, KeySpec), String>,

    templates_dir: PathBuf,
    templates_kind: TemplatesKind,
    templates: Vec<TemplateEntry>,
    templates_selected: usize,
    templates_error: Option<String>,
    templates_details_scroll: usize,
    templates_refresh_after_edit: Option<String>,
    template_deploy_inflight: HashMap<String, DeployMarker>,

    net_templates: Vec<NetTemplateEntry>,
    net_templates_selected: usize,
    net_templates_error: Option<String>,
    net_templates_details_scroll: usize,
    net_templates_refresh_after_edit: Option<String>,
    net_template_deploy_inflight: HashMap<String, DeployMarker>,

    theme_refresh_after_edit: Option<String>,
}

#[derive(Clone, Debug)]
struct ShellConfirm {
    label: String,
    cmdline: String, // command line without leading ':'
}

impl App {
    fn log_msg(&mut self, level: MsgLevel, text: impl Into<String>) {
        let text = text.into();
        let at = self.session_start.elapsed();
        self.session_msgs.push(SessionMsg { at, level, text });
    }

    fn get_view_split_mode(&self, view: ShellView) -> Option<ShellSplitMode> {
        self.shell_split_by_view.get(view.slug()).copied()
    }

    fn set_view_split_mode(&mut self, view: ShellView, mode: ShellSplitMode) {
        self.shell_split_by_view
            .insert(view.slug().to_string(), mode);
    }

    fn cmd_history_max_effective(&self) -> usize {
        self.cmd_history_max.clamp(1, 5000)
    }

    fn set_cmd_history_entries(&mut self, mut entries: Vec<String>) {
        entries.retain(|s| !s.trim().is_empty());
        let max = self.cmd_history_max_effective();
        if entries.len() > max {
            let drain = entries.len() - max;
            entries.drain(0..drain);
        }
        self.shell_cmd_history.entries = entries.clone();
        self.shell_cmd_history.reset_nav();
        self.logs_cmd_history.entries = entries.clone();
        self.logs_cmd_history.reset_nav();
        self.inspect_cmd_history.entries = entries;
        self.inspect_cmd_history.reset_nav();
    }

    fn push_cmd_history(&mut self, cmd: &str) {
        let max = self.cmd_history_max_effective();
        self.shell_cmd_history.push(cmd, max);
        // Keep all command modes in sync.
        let entries = self.shell_cmd_history.entries.clone();
        self.logs_cmd_history.entries = entries.clone();
        self.inspect_cmd_history.entries = entries;
        self.shell_cmd_history.reset_nav();
        self.logs_cmd_history.reset_nav();
        self.inspect_cmd_history.reset_nav();
        self.persist_config();
    }

    fn clear_last_error(&mut self) {
        self.last_error = None;
    }

    fn set_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.last_error = Some(t.clone());
        self.log_msg(MsgLevel::Error, t);
    }

    fn set_warn(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.last_error = Some(t.clone());
        self.log_msg(MsgLevel::Warn, t);
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.log_msg(MsgLevel::Info, text);
    }

    fn messages_copy_selected(&mut self) {
        if self.session_msgs.is_empty() {
            self.set_warn("no messages");
            return;
        }
        let idx = if self.shell_msgs_scroll == usize::MAX {
            self.session_msgs.len().saturating_sub(1)
        } else {
            self.shell_msgs_scroll
                .min(self.session_msgs.len().saturating_sub(1))
        };
        let m = &self.session_msgs[idx];
        let lvl = match m.level {
            MsgLevel::Info => "INFO",
            MsgLevel::Warn => "WARN",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let line = format!("{ts} {lvl} {}", m.text);
        if let Err(e) = copy_to_clipboard(&line) {
            self.set_error(format!("{e:#}"));
        } else {
            self.set_info("copied message to clipboard");
        }
    }

    fn clear_conn_error(&mut self) {
        self.conn_error = None;
    }

    fn set_conn_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.conn_error = Some(t.clone());
        self.set_error(t);
    }

    fn image_row_key(img: &ImageRow) -> String {
        if img.repository != "<none>" && img.tag != "<none>" && !img.tag.trim().is_empty() {
            format!("ref:{}:{}", img.repository, img.tag)
        } else {
            format!("id:{}", img.id)
        }
    }

    fn image_row_ref(img: &ImageRow) -> Option<String> {
        if img.repository != "<none>" && img.tag != "<none>" && !img.tag.trim().is_empty() {
            Some(format!("{}:{}", img.repository, img.tag))
        } else {
            None
        }
    }

    fn new(
        servers: Vec<ServerEntry>,
        keymap: Vec<KeyBinding>,
        active_server: Option<String>,
        config_path: std::path::PathBuf,
        view_layout: HashMap<String, String>,
        theme_name: String,
        theme: theme::ThemeSpec,
    ) -> Self {
        let mut server_selected = 0usize;
        if let Some(name) = &active_server {
            if let Some(idx) = servers.iter().position(|s| &s.name == name) {
                server_selected = idx;
            }
        }
        let mut app = Self {
            containers: Vec::new(),
            images: Vec::new(),
            volumes: Vec::new(),
            networks: Vec::new(),
            image_referenced_by_id: HashMap::new(),
            image_referenced_count_by_id: HashMap::new(),
            image_running_count_by_id: HashMap::new(),
            images_unused_only: false,
            volume_referenced_by_name: HashMap::new(),
            volume_referenced_count_by_name: HashMap::new(),
            volume_running_count_by_name: HashMap::new(),
            volume_containers_by_name: HashMap::new(),
            volumes_unused_only: false,
            usage_refresh_needed: true,
            usage_loading: false,
            selected: 0,
            active_view: ActiveView::Containers,
            list_mode: ListMode::Flat,
            view: Vec::new(),
            view_dirty: true,
            stack_collapsed: HashSet::new(),
            container_idx_by_id: HashMap::new(),
            marked: HashSet::new(),
            marked_images: HashSet::new(),
            marked_volumes: HashSet::new(),
            marked_networks: HashSet::new(),
            images_selected: 0,
            volumes_selected: 0,
            networks_selected: 0,
            last_refresh: None,
            conn_error: None,
            last_error: None,
            loading: true,
            loading_since: Some(Instant::now()),
            action_inflight: HashMap::new(),
            image_action_inflight: HashMap::new(),
            volume_action_inflight: HashMap::new(),
            network_action_inflight: HashMap::new(),
            inspect_loading: false,
            inspect_error: None,
            inspect_value: None,
            inspect_target: None,
            inspect_for_id: None,
            inspect_lines: Vec::new(),
            inspect_selected: 0,
            inspect_scroll_top: 0,
            inspect_scroll: 0,
            inspect_query: String::new(),
            inspect_expanded: HashSet::new(),
            inspect_match_paths: Vec::new(),
            inspect_path_rank: HashMap::new(),
            inspect_mode: InspectMode::Normal,
            inspect_input: String::new(),
            inspect_input_cursor: 0,
            inspect_cmd_history: CmdHistory::new(),

            servers,
            active_server,
            server_selected,
            config_path,
            current_target: String::new(),

            logs_loading: false,
            logs_error: None,
            logs_text: None,
            logs_for_id: None,
            logs_tail: 500,
            logs_cursor: 0,
            logs_scroll_top: 0,
            logs_select_anchor: None,
            logs_hscroll: 0,
            logs_max_width: 0,
            logs_mode: LogsMode::Normal,
            logs_input: String::new(),
            logs_query: String::new(),
            logs_command: String::new(),
            logs_input_cursor: 0,
            logs_command_cursor: 0,
            logs_cmd_history: CmdHistory::new(),
            logs_use_regex: false,
            logs_regex: None,
            logs_regex_error: None,
            logs_match_lines: Vec::new(),
            logs_show_line_numbers: false,

            mouse_enabled: false,

            ip_cache: HashMap::new(),
            ip_refresh_needed: true,
            should_quit: false,
            ascii_only: false,
            theme_name,
            theme,
            shell_view: ShellView::Containers,
            shell_last_main_view: ShellView::Containers,
            shell_focus: ShellFocus::Sidebar,
            shell_sidebar_collapsed: false,
            shell_sidebar_hidden: false,
            shell_sidebar_selected: 0,
            shell_split_mode: ShellSplitMode::Horizontal,
            shell_split_by_view: view_layout
                .into_iter()
                .filter_map(|(k, v)| {
                    let mode = match v.to_ascii_lowercase().as_str() {
                        "h" | "hor" | "horizontal" => Some(ShellSplitMode::Horizontal),
                        "v" | "ver" | "vertical" => Some(ShellSplitMode::Vertical),
                        _ => None,
                    }?;
                    Some((k, mode))
                })
                .collect(),
            shell_server_shortcuts: Vec::new(),
            shell_pending_interactive: None,
            shell_cmd_mode: false,
            shell_cmd_input: String::new(),
            shell_cmd_cursor: 0,
            shell_confirm: None,
            shell_cmd_history: CmdHistory::new(),
            shell_help_scroll: 0,
            shell_help_return: ShellView::Containers,
            refresh_secs: 5,
            cmd_history_max: 200,

            session_start: Instant::now(),
            session_msgs: Vec::new(),
            shell_msgs_scroll: 0,
            shell_msgs_hscroll: 0,
            shell_msgs_return: ShellView::Containers,

            keymap,
            keymap_parsed: HashMap::new(),
            keymap_defaults: HashMap::new(),

            templates_dir: PathBuf::from("templates"),
            templates_kind: TemplatesKind::Stacks,
            templates: Vec::new(),
            templates_selected: 0,
            templates_error: None,
            templates_details_scroll: 0,
            templates_refresh_after_edit: None,
            template_deploy_inflight: HashMap::new(),

            net_templates: Vec::new(),
            net_templates_selected: 0,
            net_templates_error: None,
            net_templates_details_scroll: 0,
            net_templates_refresh_after_edit: None,
            net_template_deploy_inflight: HashMap::new(),

            theme_refresh_after_edit: None,
        };
        app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
        app.rebuild_keymap();
        if let Some(mode) = app.get_view_split_mode(app.shell_view) {
            app.shell_split_mode = mode;
        }
        app
    }

    fn refresh_templates(&mut self) {
        self.templates_error = None;
        self.templates.clear();
        self.templates_details_scroll = 0;

        self.migrate_templates_layout_if_needed();

        let dir = self.stack_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.templates_error = Some(format!("failed to create templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.templates_error = Some(format!("failed to read templates dir: {e}"));
                return;
            }
        };

        let mut out: Vec<TemplateEntry> = Vec::new();
        for ent in entries.flatten() {
            let path = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let compose_path = path.join("compose.yaml");
            let has_compose = compose_path.exists();
            let desc = if has_compose {
                extract_template_description(&compose_path).unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            out.push(TemplateEntry {
                name,
                dir: path,
                compose_path,
                has_compose,
                desc,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.templates = out;
        if self.templates_selected >= self.templates.len() {
            self.templates_selected = self.templates.len().saturating_sub(1);
        }
    }

    fn selected_template(&self) -> Option<&TemplateEntry> {
        self.templates.get(self.templates_selected)
    }

    fn net_templates_dir(&self) -> PathBuf {
        self.templates_dir.join("networks")
    }

    fn stack_templates_dir(&self) -> PathBuf {
        self.templates_dir.join("stacks")
    }

    fn migrate_templates_layout_if_needed(&mut self) {
        // Old layout: <templates_dir>/<name>/compose.yaml and <templates_dir>/networks/...
        // New layout: <templates_dir>/stacks/<name>/compose.yaml and <templates_dir>/networks/...
        let stacks = self.stack_templates_dir();
        if stacks.exists() {
            return;
        }
        let root = self.templates_dir.clone();
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut to_move: Vec<(String, PathBuf)> = Vec::new();
        for ent in entries.flatten() {
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "networks" || name == "stacks" {
                continue;
            }
            to_move.push((name, ent.path()));
        }
        if to_move.is_empty() {
            return;
        }
        if let Err(e) = fs::create_dir_all(&stacks) {
            self.log_msg(
                MsgLevel::Warn,
                format!("failed to create stacks templates dir '{}': {e}", stacks.display()),
            );
            return;
        }
        for (name, from) in to_move {
            let to = stacks.join(&name);
            if to.exists() {
                self.log_msg(
                    MsgLevel::Warn,
                    format!(
                        "template migration skipped: '{}' already exists in stacks/",
                        name
                    ),
                );
                continue;
            }
            if let Err(e) = fs::rename(&from, &to) {
                self.log_msg(
                    MsgLevel::Warn,
                    format!(
                        "template migration failed for '{}': {}",
                        name,
                        e
                    ),
                );
            }
        }
    }

    fn refresh_net_templates(&mut self) {
        self.net_templates_error = None;
        self.net_templates.clear();
        self.net_templates_details_scroll = 0;

        self.migrate_templates_layout_if_needed();

        let dir = self.net_templates_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            self.net_templates_error = Some(format!("failed to create net templates dir: {e}"));
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.net_templates_error = Some(format!("failed to read net templates dir: {e}"));
                return;
            }
        };

        let mut out: Vec<NetTemplateEntry> = Vec::new();
        for ent in entries.flatten() {
            let path = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let cfg_path = path.join("network.json");
            let has_cfg = cfg_path.exists();
            let desc = if has_cfg {
                extract_net_template_description(&cfg_path).unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            out.push(NetTemplateEntry {
                name,
                dir: path,
                cfg_path,
                has_cfg,
                desc,
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.net_templates = out;
        if self.net_templates_selected >= self.net_templates.len() {
            self.net_templates_selected = self.net_templates.len().saturating_sub(1);
        }
    }

    fn selected_net_template(&self) -> Option<&NetTemplateEntry> {
        self.net_templates.get(self.net_templates_selected)
    }

    fn rebuild_keymap(&mut self) {
        self.keymap_parsed.clear();
        self.keymap_defaults = build_default_keymap();
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

    fn selected_container(&self) -> Option<&ContainerRow> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        match self.list_mode {
            ListMode::Flat => self.containers.get(self.selected),
            ListMode::Tree => {
                let Some(entry) = self.view.get(self.selected) else {
                    return None;
                };
                let ViewEntry::Container { id, .. } = entry else {
                    return None;
                };
                let idx = self.container_idx_by_id.get(id)?;
                self.containers.get(*idx)
            }
        }
    }

    fn selected_stack(&self) -> Option<(&str, usize, usize, bool)> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        if self.list_mode != ListMode::Tree {
            return None;
        }
        let Some(entry) = self.view.get(self.selected) else {
            return None;
        };
        match entry {
            ViewEntry::StackHeader {
                name,
                total,
                running,
                expanded,
            } => Some((name.as_str(), *total, *running, *expanded)),
            ViewEntry::UngroupedHeader { total, running } => {
                Some(("Ungrouped", *total, *running, true))
            }
            _ => None,
        }
    }

    fn selected_stack_container_ids(&mut self) -> Option<Vec<String>> {
        if self.active_view != ActiveView::Containers {
            return None;
        }
        if self.list_mode != ListMode::Tree {
            return None;
        }
        self.ensure_view();
        let Some(entry) = self.view.get(self.selected) else {
            return None;
        };
        let ViewEntry::StackHeader { name, .. } = entry else {
            return None;
        };
        let stack = name.clone();
        let mut ids: Vec<String> = self
            .containers
            .iter()
            .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack.as_str()))
            .map(|c| c.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        Some(ids)
    }

    fn view_len(&mut self) -> usize {
        if self.active_view != ActiveView::Containers {
            return 0;
        }
        self.ensure_view();
        match self.list_mode {
            ListMode::Flat => self.containers.len(),
            ListMode::Tree => self.view.len(),
        }
    }

    fn ensure_view(&mut self) {
        if self.active_view != ActiveView::Containers {
            return;
        }
        if self.list_mode != ListMode::Tree {
            self.view.clear();
            self.view_dirty = false;
            return;
        }
        if !self.view_dirty {
            return;
        }
        self.view_dirty = false;
        self.rebuild_tree_view();
    }

    fn current_anchor(&self) -> Option<(String, Option<String>)> {
        // (container_id, stack_name) where stack_name is Some only if selection is a stack header.
        match self.list_mode {
            ListMode::Flat => self.selected_container().map(|c| (c.id.clone(), None)),
            ListMode::Tree => match self.view.get(self.selected) {
                Some(ViewEntry::Container { id, .. }) => Some((id.clone(), None)),
                Some(ViewEntry::StackHeader { name, .. }) => {
                    Some(("".to_string(), Some(name.clone())))
                }
                Some(ViewEntry::UngroupedHeader { .. }) => {
                    Some(("".to_string(), Some("Ungrouped".to_string())))
                }
                None => None,
            },
        }
    }

    fn rebuild_tree_view(&mut self) {
        use std::collections::BTreeMap;

        let anchor = self.current_anchor();

        let mut stacks: BTreeMap<String, Vec<&ContainerRow>> = BTreeMap::new();
        let mut ungrouped: Vec<&ContainerRow> = Vec::new();
        for c in &self.containers {
            if let Some(stack) = stack_name_from_labels(&c.labels) {
                stacks.entry(stack).or_default().push(c);
            } else {
                ungrouped.push(c);
            }
        }

        let mut out: Vec<ViewEntry> = Vec::new();

        for (name, mut cs) in stacks {
            cs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            let total = cs.len();
            let running = cs
                .iter()
                .filter(|c| !is_container_stopped(&c.status))
                .count();
            let expanded = !self.stack_collapsed.contains(&name);
            out.push(ViewEntry::StackHeader {
                name: name.clone(),
                total,
                running,
                expanded,
            });
            if expanded {
                for c in cs {
                    out.push(ViewEntry::Container {
                        id: c.id.clone(),
                        indent: 2,
                    });
                }
            }
        }

        if !ungrouped.is_empty() {
            let total = ungrouped.len();
            let running = ungrouped
                .iter()
                .filter(|c| !is_container_stopped(&c.status))
                .count();
            out.push(ViewEntry::UngroupedHeader { total, running });
            ungrouped.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            for c in ungrouped {
                out.push(ViewEntry::Container {
                    id: c.id.clone(),
                    indent: 2,
                });
            }
        }

        self.view = out;

        // Restore selection when possible.
        if let Some((id, stack)) = anchor {
            if !id.is_empty() {
                if let Some(idx) = self
                    .view
                    .iter()
                    .position(|e| matches!(e, ViewEntry::Container { id: cid, .. } if cid == &id))
                {
                    self.selected = idx;
                    return;
                }
            }
            if let Some(stack) = stack {
                if let Some(idx) = self.view.iter().position(
                    |e| matches!(e, ViewEntry::StackHeader { name, .. } if name == &stack),
                ) {
                    self.selected = idx;
                    return;
                }
            }
        }
        if self.selected >= self.view.len() {
            self.selected = self.view.len().saturating_sub(1);
        }
    }

    fn toggle_tree_expanded_selected(&mut self) -> bool {
        if self.active_view != ActiveView::Containers || self.list_mode != ListMode::Tree {
            return false;
        }
        self.ensure_view();
        let Some(entry) = self.view.get(self.selected).cloned() else {
            return false;
        };
        match entry {
            ViewEntry::StackHeader { name, .. } => {
                if !self.stack_collapsed.insert(name.clone()) {
                    self.stack_collapsed.remove(&name);
                }
                self.view_dirty = true;
                self.ensure_view();
                true
            }
            _ => false,
        }
    }

    fn is_marked(&self, id: &str) -> bool {
        self.marked.contains(id)
    }

    fn is_image_marked(&self, key: &str) -> bool {
        self.marked_images.contains(key)
    }

    fn is_volume_marked(&self, name: &str) -> bool {
        self.marked_volumes.contains(name)
    }

    fn is_network_marked(&self, id: &str) -> bool {
        self.marked_networks.contains(id)
    }

    fn toggle_mark_selected(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                let Some(id) = self.selected_container().map(|c| c.id.clone()) else {
                    return;
                };
                if !self.marked.remove(&id) {
                    self.marked.insert(id);
                }
            }
            ActiveView::Images => {
                let Some(img) = self.selected_image() else {
                    return;
                };
                let key = App::image_row_key(img);
                if !self.marked_images.remove(&key) {
                    self.marked_images.insert(key);
                }
            }
            ActiveView::Volumes => {
                let Some(name) = self.selected_volume().map(|v| v.name.clone()) else {
                    return;
                };
                if !self.marked_volumes.remove(&name) {
                    self.marked_volumes.insert(name);
                }
            }
            ActiveView::Networks => {
                let Some(id) = self.selected_network().map(|n| n.id.clone()) else {
                    return;
                };
                if !self.marked_networks.remove(&id) {
                    self.marked_networks.insert(id);
                }
            }
        }
    }

    fn mark_all(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                for c in &self.containers {
                    self.marked.insert(c.id.clone());
                }
            }
            ActiveView::Images => {
                if self.images_unused_only {
                    for img in &self.images {
                        if !self.image_referenced(img) {
                            self.marked_images.insert(App::image_row_key(img));
                        }
                    }
                } else {
                    for img in &self.images {
                        self.marked_images.insert(App::image_row_key(img));
                    }
                }
            }
            ActiveView::Volumes => {
                if self.volumes_unused_only {
                    for v in &self.volumes {
                        if !self.volume_referenced(v) {
                            self.marked_volumes.insert(v.name.clone());
                        }
                    }
                } else {
                    for v in &self.volumes {
                        self.marked_volumes.insert(v.name.clone());
                    }
                }
            }
            ActiveView::Networks => {
                for n in &self.networks {
                    self.marked_networks.insert(n.id.clone());
                }
            }
        }
    }

    fn clear_marks(&mut self) {
        match self.active_view {
            ActiveView::Containers => self.marked.clear(),
            ActiveView::Images => self.marked_images.clear(),
            ActiveView::Volumes => self.marked_volumes.clear(),
            ActiveView::Networks => self.marked_networks.clear(),
        }
    }

    fn clear_all_marks(&mut self) {
        self.marked.clear();
        self.marked_images.clear();
        self.marked_volumes.clear();
        self.marked_networks.clear();
    }

    fn prune_marks(&mut self) {
        if self.marked.is_empty() || self.containers.is_empty() {
            if self.containers.is_empty() {
                // Keep marks during transient loading; they will be pruned after we have data again.
            }
            return;
        }
        let present: HashSet<&str> = self.containers.iter().map(|c| c.id.as_str()).collect();
        self.marked.retain(|id| present.contains(id.as_str()));
    }

    fn prune_image_marks(&mut self) {
        if self.marked_images.is_empty() || self.images.is_empty() {
            if self.images.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<String> = self.images.iter().map(App::image_row_key).collect();
        self.marked_images.retain(|k| present.contains(k));
    }

    fn prune_volume_marks(&mut self) {
        if self.marked_volumes.is_empty() || self.volumes.is_empty() {
            if self.volumes.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<&str> = self.volumes.iter().map(|v| v.name.as_str()).collect();
        self.marked_volumes
            .retain(|name| present.contains(name.as_str()));
    }

    fn prune_network_marks(&mut self) {
        if self.marked_networks.is_empty() || self.networks.is_empty() {
            if self.networks.is_empty() {
                // Keep marks during transient loading.
            }
            return;
        }
        let present: HashSet<&str> = self.networks.iter().map(|n| n.id.as_str()).collect();
        self.marked_networks
            .retain(|id| present.contains(id.as_str()));
    }

    fn move_up(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                if self.view_len() == 0 {
                    self.selected = 0;
                    return;
                }
                self.selected = self.selected.saturating_sub(1);
            }
            ActiveView::Images => self.images_selected = self.images_selected.saturating_sub(1),
            ActiveView::Volumes => self.volumes_selected = self.volumes_selected.saturating_sub(1),
            ActiveView::Networks => {
                self.networks_selected = self.networks_selected.saturating_sub(1)
            }
        }
    }

    fn move_down(&mut self) {
        match self.active_view {
            ActiveView::Containers => {
                if self.view_len() == 0 {
                    self.selected = 0;
                    return;
                }
                self.selected = (self.selected + 1).min(self.view_len().saturating_sub(1));
            }
            ActiveView::Images => {
                if self.images_visible_len() == 0 {
                    self.images_selected = 0;
                } else {
                    self.images_selected =
                        (self.images_selected + 1).min(self.images_visible_len() - 1);
                }
            }
            ActiveView::Volumes => {
                if self.volumes_visible_len() == 0 {
                    self.volumes_selected = 0;
                } else {
                    self.volumes_selected =
                        (self.volumes_selected + 1).min(self.volumes_visible_len() - 1);
                }
            }
            ActiveView::Networks => {
                if self.networks.is_empty() {
                    self.networks_selected = 0;
                } else {
                    self.networks_selected =
                        (self.networks_selected + 1).min(self.networks.len() - 1);
                }
            }
        }
    }

    fn set_containers(&mut self, containers: Vec<ContainerRow>) {
        self.containers = containers;
        self.container_idx_by_id.clear();
        for (i, c) in self.containers.iter().enumerate() {
            self.container_idx_by_id.insert(c.id.clone(), i);
        }
        self.loading = false;
        self.loading_since = None;
        self.ip_refresh_needed = true;
        self.prune_marks();
        self.view_dirty = true;
        self.reconcile_action_markers();
        self.ensure_view();
        let max = match self.list_mode {
            ListMode::Flat => self.containers.len(),
            ListMode::Tree => self.view.len(),
        };
        if self.selected >= max {
            self.selected = max.saturating_sub(1);
        }
    }

    fn reconcile_noncontainer_action_markers(&mut self) {
        let now = Instant::now();
        let present_image_ids: HashSet<&str> = self.images.iter().map(|i| i.id.as_str()).collect();
        let present_image_refs: HashSet<String> =
            self.images.iter().map(App::image_row_key).collect();
        self.image_action_inflight.retain(|k, m| {
            if now >= m.until {
                return false;
            }
            if k.starts_with("ref:") {
                return present_image_refs.contains(k);
            }
            // Fallback: allow raw image IDs to keep markers across tag changes.
            present_image_ids.contains(k.as_str()) || present_image_refs.contains(k)
        });
        let present_vols: HashSet<&str> = self.volumes.iter().map(|v| v.name.as_str()).collect();
        self.volume_action_inflight
            .retain(|name, m| now < m.until && present_vols.contains(name.as_str()));
        let present_nets: HashSet<&str> = self.networks.iter().map(|n| n.id.as_str()).collect();
        self.network_action_inflight
            .retain(|id, m| now < m.until && present_nets.contains(id.as_str()));
    }

    fn image_referenced(&self, img: &ImageRow) -> bool {
        self.image_referenced_by_id
            .get(&img.id)
            .copied()
            .unwrap_or(false)
    }

    fn volume_referenced(&self, v: &VolumeRow) -> bool {
        self.volume_referenced_by_name
            .get(&v.name)
            .copied()
            .unwrap_or(false)
    }

    fn reconcile_action_markers(&mut self) {
        // The docker start/stop/restart command may return before docker ps reflects the new state.
        // Keep showing the marker until we observe a matching state, or until the marker expires.
        let now = Instant::now();
        self.action_inflight.retain(|id, marker| {
            if now >= marker.until {
                return false;
            }
            let Some(c) = self.containers.iter().find(|c| &c.id == id) else {
                // If it's gone, we consider the action done (or the container removed).
                return false;
            };
            let running =
                c.status.trim().starts_with("Up") || c.status.trim().starts_with("Restarting");
            let stopped = is_container_stopped(&c.status);
            match marker.action {
                ContainerAction::Start => !running,
                ContainerAction::Stop => !stopped,
                ContainerAction::Restart => !running,
                ContainerAction::Remove => true,
            }
        });
    }

    fn start_loading(&mut self, clear_list: bool) {
        self.loading = true;
        self.loading_since = Some(Instant::now());
        self.clear_last_error();
        if clear_list {
            self.containers.clear();
            self.selected = 0;
            self.images.clear();
            self.volumes.clear();
            self.networks.clear();
            self.image_referenced_by_id.clear();
            self.image_referenced_count_by_id.clear();
            self.image_running_count_by_id.clear();
            self.volume_referenced_by_name.clear();
            self.volume_referenced_count_by_name.clear();
            self.volume_running_count_by_name.clear();
            self.volume_containers_by_name.clear();
            self.images_selected = 0;
            self.volumes_selected = 0;
            self.networks_selected = 0;
        }
    }

    fn open_inspect_state(&mut self, target: InspectTarget) {
        self.inspect_loading = true;
        self.inspect_error = None;
        self.inspect_value = None;
        self.inspect_target = Some(target.clone());
        self.inspect_for_id = Some(target.key);
        self.inspect_lines.clear();
        self.inspect_selected = 0;
        self.inspect_scroll_top = 0;
        self.inspect_scroll = 0;
        self.inspect_query.clear();
        self.inspect_expanded.clear();
        self.inspect_expanded.insert("".to_string()); // root expanded by default
        self.inspect_match_paths.clear();
        self.inspect_path_rank.clear();
        self.inspect_mode = InspectMode::Normal;
        self.inspect_input.clear();
    }

    fn rebuild_inspect_lines(&mut self) {
        self.inspect_path_rank = collect_path_rank(self.inspect_value.as_ref());
        let effective_query = self.inspect_effective_query().to_string();
        self.inspect_match_paths =
            collect_match_paths(self.inspect_value.as_ref(), &effective_query);
        let match_set: HashSet<String> = self.inspect_match_paths.iter().cloned().collect();
        self.inspect_lines = build_inspect_lines(
            self.inspect_value.as_ref(),
            &self.inspect_expanded,
            &match_set,
            &effective_query,
        );
        if self.inspect_selected >= self.inspect_lines.len() {
            self.inspect_selected = self.inspect_lines.len().saturating_sub(1);
        }
        if self.inspect_scroll > self.inspect_selected {
            self.inspect_scroll = self.inspect_selected;
        }
    }

    fn inspect_move_up(&mut self, by: usize) {
        if self.inspect_lines.is_empty() {
            self.inspect_selected = 0;
            self.inspect_scroll = 0;
            return;
        }
        self.inspect_selected = self.inspect_selected.saturating_sub(by);
        if self.inspect_selected < self.inspect_scroll {
            self.inspect_scroll = self.inspect_selected;
        }
    }

    fn inspect_move_down(&mut self, by: usize) {
        if self.inspect_lines.is_empty() {
            self.inspect_selected = 0;
            self.inspect_scroll = 0;
            return;
        }
        self.inspect_selected = self
            .inspect_selected
            .saturating_add(by)
            .min(self.inspect_lines.len() - 1);
    }

    fn inspect_toggle_selected(&mut self) {
        let Some(line) = self.inspect_lines.get(self.inspect_selected) else {
            return;
        };
        if !line.expandable {
            return;
        }
        if self.inspect_expanded.contains(&line.path) {
            self.inspect_expanded.remove(&line.path);
        } else {
            self.inspect_expanded.insert(line.path.clone());
        }
        self.rebuild_inspect_lines();
    }

    fn inspect_expand_all(&mut self) {
        let Some(root) = self.inspect_value.as_ref() else {
            return;
        };
        self.inspect_expanded = collect_expandable_paths(root);
        self.inspect_expanded.insert("".to_string());
        self.rebuild_inspect_lines();
    }

    fn inspect_collapse_all(&mut self) {
        self.inspect_expanded.clear();
        self.inspect_expanded.insert("".to_string());
        self.rebuild_inspect_lines();
    }

    fn inspect_jump_next_match(&mut self) {
        if self.inspect_mode != InspectMode::Normal {
            return;
        }
        if self.inspect_match_paths.is_empty() {
            return;
        }
        let current_path = self
            .inspect_lines
            .get(self.inspect_selected)
            .map(|l| l.path.as_str())
            .unwrap_or("");

        let current_rank = self
            .inspect_path_rank
            .get(current_path)
            .copied()
            .unwrap_or(0);

        let mut best: Option<(usize, String)> = None;
        for p in &self.inspect_match_paths {
            let r = self.inspect_path_rank.get(p).copied().unwrap_or(usize::MAX);
            if r > current_rank && best.as_ref().map(|(br, _)| r < *br).unwrap_or(true) {
                best = Some((r, p.clone()));
            }
        }
        let target = best
            .map(|(_, p)| p)
            .or_else(|| self.inspect_match_paths.first().cloned());
        if let Some(target) = target {
            self.inspect_focus_path(&target);
        }
    }

    fn inspect_jump_prev_match(&mut self) {
        if self.inspect_mode != InspectMode::Normal {
            return;
        }
        if self.inspect_match_paths.is_empty() {
            return;
        }
        let current_path = self
            .inspect_lines
            .get(self.inspect_selected)
            .map(|l| l.path.as_str())
            .unwrap_or("");

        let current_rank = self
            .inspect_path_rank
            .get(current_path)
            .copied()
            .unwrap_or(0);

        let mut best: Option<(usize, String)> = None;
        for p in &self.inspect_match_paths {
            let r = self.inspect_path_rank.get(p).copied().unwrap_or(0);
            if r < current_rank && best.as_ref().map(|(br, _)| r > *br).unwrap_or(true) {
                best = Some((r, p.clone()));
            }
        }
        let target = best
            .map(|(_, p)| p)
            .or_else(|| self.inspect_match_paths.last().cloned());
        if let Some(target) = target {
            self.inspect_focus_path(&target);
        }
    }

    fn inspect_focus_path(&mut self, path: &str) {
        for parent in ancestors_of_pointer(path) {
            self.inspect_expanded.insert(parent);
        }
        self.rebuild_inspect_lines();
        if let Some(idx) = self.inspect_lines.iter().position(|l| l.path == path) {
            self.inspect_selected = idx;
        }
    }

    fn inspect_effective_query(&self) -> &str {
        match self.inspect_mode {
            InspectMode::Search => &self.inspect_input,
            _ => &self.inspect_query,
        }
    }

    fn inspect_enter_search(&mut self) {
        self.inspect_mode = InspectMode::Search;
        self.inspect_input = self.inspect_query.clone();
        self.inspect_input_cursor = self.inspect_input.chars().count();
        self.rebuild_inspect_lines();
    }

    fn inspect_enter_command(&mut self) {
        self.inspect_mode = InspectMode::Command;
        self.inspect_input.clear();
        self.inspect_input_cursor = 0;
    }

    fn inspect_exit_input(&mut self) {
        self.inspect_mode = InspectMode::Normal;
        self.inspect_input.clear();
        self.inspect_input_cursor = 0;
        self.rebuild_inspect_lines();
    }

    fn inspect_commit_search(&mut self) {
        self.inspect_query = self.inspect_input.clone();
        self.inspect_mode = InspectMode::Normal;
        self.inspect_input.clear();
        self.inspect_input_cursor = 0;
        self.rebuild_inspect_lines();
        if let Some(first) = self.inspect_match_paths.first().cloned() {
            self.inspect_focus_path(&first);
        }
    }

    fn inspect_copy_selected_value(&mut self, pretty: bool) {
        let Some(root) = self.inspect_value.as_ref() else {
            return;
        };
        let Some(line) = self.inspect_lines.get(self.inspect_selected) else {
            return;
        };
        let Some(value) = root.pointer(&line.path) else {
            self.inspect_error = Some("failed to locate selected value".to_string());
            return;
        };

        let text = if pretty {
            match serde_json::to_string_pretty(value) {
                Ok(s) => s,
                Err(e) => {
                    self.inspect_error = Some(format!("failed to serialize value: {:#}", e));
                    return;
                }
            }
        } else {
            value.to_string()
        };

        if let Err(e) = copy_to_clipboard(&text) {
            self.inspect_error = Some(format!("{:#}", e));
        }
    }

    fn inspect_copy_selected_path(&mut self) {
        let Some(line) = self.inspect_lines.get(self.inspect_selected) else {
            return;
        };
        if let Err(e) = copy_to_clipboard(&line.path) {
            self.inspect_error = Some(format!("{:#}", e));
        }
    }

    fn open_logs_state(&mut self, id: String) {
        self.logs_loading = true;
        self.logs_error = None;
        self.logs_text = None;
        self.logs_for_id = Some(id);
        self.logs_cursor = 0;
        self.logs_scroll_top = 0;
        self.logs_select_anchor = None;
        self.logs_hscroll = 0;
        self.logs_max_width = 0;
        self.logs_mode = LogsMode::Normal;
        self.logs_input.clear();
        self.logs_query.clear();
        self.logs_command.clear();
        self.logs_regex = None;
        self.logs_regex_error = None;
        self.logs_match_lines.clear();
        self.logs_show_line_numbers = false;
    }

    fn logs_move_up(&mut self, by: usize) {
        self.logs_cursor = self.logs_cursor.saturating_sub(by);
    }

    fn logs_move_down(&mut self, by: usize) {
        let total = self.logs_total_lines();
        if total == 0 {
            self.logs_cursor = 0;
            return;
        }
        self.logs_cursor = self.logs_cursor.saturating_add(by).min(total - 1);
    }

    fn logs_total_lines(&self) -> usize {
        self.logs_text
            .as_ref()
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    fn logs_toggle_selection(&mut self) {
        if self.logs_select_anchor.take().is_none() {
            self.logs_select_anchor = Some(self.logs_cursor);
        }
    }

    fn logs_clear_selection(&mut self) {
        self.logs_select_anchor = None;
    }

    fn logs_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.logs_select_anchor?;
        let a = anchor.min(self.logs_cursor);
        let b = anchor.max(self.logs_cursor);
        Some((a, b))
    }

    fn logs_copy_selection(&mut self) {
        let Some(text) = self.logs_text.as_deref() else {
            self.set_warn("no logs loaded");
            return;
        };

        let total = self.logs_total_lines();
        if total == 0 {
            self.set_warn("no logs loaded");
            return;
        }

        let (start, end) = self
            .logs_selection_range()
            .unwrap_or((self.logs_cursor, self.logs_cursor));
        let start = start.min(total.saturating_sub(1));
        let end = end.min(total.saturating_sub(1));

        let mut out = String::new();
        for (i, line) in text.lines().enumerate() {
            if i < start {
                continue;
            }
            if i > end {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }

        if out.is_empty() {
            self.set_warn("nothing to copy");
            return;
        }

        if let Err(e) = copy_to_clipboard(&out) {
            self.set_error(format!("{e:#}"));
        } else {
            let count = end.saturating_sub(start) + 1;
            self.set_info(format!("copied {count} line(s) to clipboard"));
            self.logs_clear_selection();
        }
    }

    fn logs_rebuild_matches(&mut self) {
        let q = match self.logs_mode {
            LogsMode::Search => self.logs_input.trim(),
            LogsMode::Normal | LogsMode::Command => self.logs_query.trim(),
        };
        if q.is_empty() {
            self.logs_match_lines.clear();
            self.logs_regex = None;
            self.logs_regex_error = None;
            return;
        }

        let Some(text) = &self.logs_text else {
            self.logs_match_lines.clear();
            return;
        };

        if self.logs_use_regex {
            match RegexBuilder::new(q).case_insensitive(true).build() {
                Ok(re) => {
                    self.logs_regex = Some(re);
                    self.logs_regex_error = None;
                }
                Err(e) => {
                    self.logs_regex = None;
                    self.logs_regex_error = Some(format!("{e}"));
                    self.logs_match_lines.clear();
                    return;
                }
            }

            let Some(re) = self.logs_regex.as_ref() else {
                self.logs_match_lines.clear();
                return;
            };
            self.logs_match_lines = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| if re.is_match(line) { Some(i) } else { None })
                .collect();
        } else {
            self.logs_regex = None;
            self.logs_regex_error = None;
            let q_lc = q.to_ascii_lowercase();
            self.logs_match_lines = text
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    if line.to_ascii_lowercase().contains(&q_lc) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
        }
    }

    fn logs_commit_search(&mut self) {
        self.logs_query = self.logs_input.clone();
        self.logs_mode = LogsMode::Normal;
        self.logs_input.clear();
        self.logs_input_cursor = 0;
        self.logs_rebuild_matches();
        if let Some(first) = self.logs_match_lines.first().copied() {
            self.logs_cursor = first;
        }
    }

    fn logs_cancel_search(&mut self) {
        self.logs_mode = LogsMode::Normal;
        self.logs_input.clear();
        self.logs_input_cursor = 0;
        self.logs_rebuild_matches();
    }

    fn logs_next_match(&mut self) {
        if self.logs_mode != LogsMode::Normal {
            return;
        }
        if self.logs_match_lines.is_empty() {
            return;
        }
        let cur = self.logs_cursor;
        let next = self
            .logs_match_lines
            .iter()
            .copied()
            .find(|&i| i > cur)
            .or_else(|| self.logs_match_lines.first().copied())
            .unwrap();
        self.logs_cursor = next;
    }

    fn logs_prev_match(&mut self) {
        if self.logs_mode != LogsMode::Normal {
            return;
        }
        if self.logs_match_lines.is_empty() {
            return;
        }
        let cur = self.logs_cursor;
        let prev = self
            .logs_match_lines
            .iter()
            .copied()
            .rfind(|&i| i < cur)
            .or_else(|| self.logs_match_lines.last().copied())
            .unwrap();
        self.logs_cursor = prev;
    }

    fn selected_image(&self) -> Option<&ImageRow> {
        let idx = self.images_visible_index_at(self.images_selected)?;
        self.images.get(idx)
    }

    fn selected_volume(&self) -> Option<&VolumeRow> {
        let idx = self.volumes_visible_index_at(self.volumes_selected)?;
        self.volumes.get(idx)
    }

    fn selected_network(&self) -> Option<&NetworkRow> {
        self.networks.get(self.networks_selected)
    }

    fn is_system_network(n: &NetworkRow) -> bool {
        // Docker/system-managed networks that should not be modified from the UI.
        // - Default networks: bridge/host/none
        // - Swarm: ingress, docker_gwbridge
        matches!(
            n.name.as_str(),
            "bridge" | "host" | "none" | "ingress" | "docker_gwbridge"
        )
    }

    fn is_system_network_id(&self, id: &str) -> bool {
        self.networks
            .iter()
            .find(|n| n.id == id)
            .map(App::is_system_network)
            .unwrap_or(false)
    }

    fn images_visible_index_at(&self, pos: usize) -> Option<usize> {
        if !self.images_unused_only {
            if pos < self.images.len() {
                return Some(pos);
            }
            return None;
        }
        self.images
            .iter()
            .enumerate()
            .filter(|(_, img)| !self.image_referenced(img))
            .nth(pos)
            .map(|(i, _)| i)
    }

    fn images_visible_len(&self) -> usize {
        if !self.images_unused_only {
            self.images.len()
        } else {
            self.images
                .iter()
                .filter(|img| !self.image_referenced(img))
                .count()
        }
    }

    fn volumes_visible_index_at(&self, pos: usize) -> Option<usize> {
        if !self.volumes_unused_only {
            if pos < self.volumes.len() {
                return Some(pos);
            }
            return None;
        }
        self.volumes
            .iter()
            .enumerate()
            .filter(|(_, v)| !self.volume_referenced(v))
            .nth(pos)
            .map(|(i, _)| i)
    }

    fn volumes_visible_len(&self) -> usize {
        if !self.volumes_unused_only {
            self.volumes.len()
        } else {
            self.volumes
                .iter()
                .filter(|v| !self.volume_referenced(v))
                .count()
        }
    }

    fn selected_inspect_target(&self) -> Option<InspectTarget> {
        match self.active_view {
            ActiveView::Containers => {
                let c = self.selected_container()?;
                Some(InspectTarget {
                    kind: InspectKind::Container,
                    key: c.id.clone(),
                    arg: c.id.clone(),
                    label: c.name.clone(),
                })
            }
            ActiveView::Images => {
                let img = self.selected_image()?;
                Some(InspectTarget {
                    kind: InspectKind::Image,
                    key: img.id.clone(),
                    arg: img.id.clone(),
                    label: img.name(),
                })
            }
            ActiveView::Volumes => {
                let v = self.selected_volume()?;
                Some(InspectTarget {
                    kind: InspectKind::Volume,
                    key: v.name.clone(),
                    arg: v.name.clone(),
                    label: v.name.clone(),
                })
            }
            ActiveView::Networks => {
                let n = self.selected_network()?;
                Some(InspectTarget {
                    kind: InspectKind::Network,
                    key: n.id.clone(),
                    arg: n.id.clone(),
                    label: n.name.clone(),
                })
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Connection {
    runner: Runner,
    docker: DockerCfg,
}

fn extract_container_ip(v: &Value) -> Option<String> {
    // Prefer user-defined networks.
    let ip = v
        .pointer("/NetworkSettings/Networks")
        .and_then(|n| n.as_object())
        .and_then(|map| {
            for (_name, net) in map {
                if let Some(ip) = net.get("IPAddress").and_then(|x| x.as_str()) {
                    let ip = ip.trim();
                    if !ip.is_empty() {
                        return Some(ip.to_string());
                    }
                }
            }
            None
        })
        .or_else(|| {
            v.pointer("/NetworkSettings/IPAddress")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    ip
}

#[derive(Debug, Clone, Default)]
struct UsageSnapshot {
    image_ref_count_by_id: HashMap<String, usize>,
    image_run_count_by_id: HashMap<String, usize>,
    volume_ref_count_by_name: HashMap<String, usize>,
    volume_run_count_by_name: HashMap<String, usize>,
    volume_containers_by_name: HashMap<String, Vec<String>>,
    ip_by_container_id: HashMap<String, String>,
}

fn normalize_image_id(id: &str) -> String {
    let s = id.trim();
    if s.is_empty() {
        return "".to_string();
    }
    if s.starts_with("sha256:") {
        return s.to_string();
    }
    format!("sha256:{}", s)
}

pub async fn run_tui(
    runner: Runner,
    cfg: DockerCfg,
    refresh: Duration,
    logs_tail: usize,
    cmd_history_max: usize,
    cmd_history: Vec<String>,
    templates_dir: String,
    view_layout: HashMap<String, String>,
    active_theme: String,
    servers: Vec<ServerEntry>,
    keymap: Vec<KeyBinding>,
    active_server: Option<String>,
    config_path: std::path::PathBuf,
    mouse_enabled: bool,
    ascii_only: bool,
) -> anyhow::Result<()> {
    let mut terminal = setup_terminal(mouse_enabled).context("failed to setup terminal")?;
    let (theme_spec, theme_err) = match theme::load_theme(&config_path, &active_theme) {
        Ok(t) => (t, None),
        Err(e) => (theme::default_theme_spec(), Some(e)),
    };
    let mut app = App::new(
        servers,
        keymap,
        active_server,
        config_path,
        view_layout,
        active_theme,
        theme_spec,
    );
    if let Some(e) = theme_err {
        app.log_msg(MsgLevel::Warn, format!("failed to load theme: {:#}", e));
    }
    app.current_target = runner.key();
    app.mouse_enabled = mouse_enabled;
    app.ascii_only = ascii_only;
    app.refresh_secs = refresh.as_secs().max(1);
    app.logs_tail = logs_tail.max(1);
    app.cmd_history_max = cmd_history_max.clamp(1, 5000);
    app.set_cmd_history_entries(cmd_history);
    app.templates_dir = expand_user_path(&templates_dir);
    app.refresh_templates();

    // Background fetch: container list, inspect, logs, and actions are done via
    // background tasks so the UI stays responsive.
    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<(
        String,
        anyhow::Result<(
            Vec<ContainerRow>,
            Vec<ImageRow>,
            Vec<VolumeRow>,
            Vec<NetworkRow>,
        )>,
    )>();
    let (refresh_tx, mut refresh_rx) = mpsc::unbounded_channel::<()>();

    let (inspect_req_tx, mut inspect_req_rx) = mpsc::unbounded_channel::<InspectTarget>();
    let (inspect_res_tx, mut inspect_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<Value>)>();

    let (action_req_tx, mut action_req_rx) = mpsc::unbounded_channel::<ActionRequest>();
    let (action_res_tx, mut action_res_rx) =
        mpsc::unbounded_channel::<(ActionRequest, anyhow::Result<String>)>();

    let (logs_req_tx, mut logs_req_rx) = mpsc::unbounded_channel::<(String, usize)>();
    let (logs_res_tx, mut logs_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<String>)>();

    let (ip_req_tx, mut ip_req_rx) = mpsc::unbounded_channel::<Vec<String>>();
    let (ip_res_tx, mut ip_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<HashMap<String, String>>)>();

    let (usage_req_tx, mut usage_req_rx) = mpsc::unbounded_channel::<Vec<String>>();
    let (usage_res_tx, mut usage_res_rx) =
        mpsc::unbounded_channel::<(String, anyhow::Result<UsageSnapshot>)>();

    let (conn_tx, conn_rx) = watch::channel(Connection {
        runner: runner.clone(),
        docker: cfg.clone(),
    });

    let (refresh_interval_tx, refresh_interval_rx) =
        watch::channel(Duration::from_secs(app.refresh_secs.max(1)));
    let fetch_task = tokio::spawn(async move {
        let mut refresh_interval_rx = refresh_interval_rx;
        let mut interval = tokio::time::interval(*refresh_interval_rx.borrow());
        let conn_rx = conn_rx;
        let mut conn_rx = conn_rx;
        loop {
            tokio::select! {
              _ = interval.tick() => {}
              maybe = refresh_rx.recv() => {
                if maybe.is_none() {
                  break;
                }
              }
              changed = refresh_interval_rx.changed() => {
                if changed.is_err() {
                  break;
                }
                interval = tokio::time::interval(*refresh_interval_rx.borrow());
              }
              changed = conn_rx.changed() => {
                if changed.is_err() {
                  break;
                }
              }
            }

            let conn = conn_rx.borrow().clone();
            let key = conn.runner.key();
            let cmd = docker::overview_command(&conn.docker);
            let child = match conn.runner.spawn_killable(&cmd) {
                Ok(c) => c,
                Err(e) => {
                    let _ = result_tx.send((key, Err(e)));
                    continue;
                }
            };

            let mut child_opt = Some(child);
            let output = tokio::select! {
              out = async {
                let child = child_opt.take().expect("child already taken");
                child.wait_with_output().await
              } => out,
              changed = conn_rx.changed() => {
                // Server switch: kill the in-flight SSH command to avoid waiting
                // for slow "docker stats" on the old server.
                if changed.is_ok() {
                  if let Some(mut child) = child_opt.take() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                  }
                  continue;
                }
                if let Some(mut child) = child_opt.take() {
                  let _ = child.kill().await;
                  let _ = child.wait().await;
                }
                break;
              }
            };

            let res = match output {
                Ok(out) => {
                    if out.status.success() {
                        match String::from_utf8(out.stdout) {
                            Ok(s) => docker::parse_overview_output(&s),
                            Err(e) => Err(anyhow::anyhow!("ssh stdout was not valid UTF-8: {}", e)),
                        }
                    } else {
                        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                        Err(anyhow::anyhow!(
                            "ssh failed: {}",
                            if stderr.is_empty() {
                                "<no stderr>"
                            } else {
                                &stderr
                            }
                        ))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("failed to run ssh: {}", e)),
            };

            let _ = result_tx.send((key, res));
        }
    });

    let inspect_conn_rx = conn_tx.subscribe();
    let inspect_task = tokio::spawn(async move {
        let inspect_conn_rx = inspect_conn_rx;
        while let Some(req) = inspect_req_rx.recv().await {
            let conn = inspect_conn_rx.borrow().clone();
            let res = match req.kind {
                InspectKind::Container => {
                    docker::fetch_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
                InspectKind::Image => {
                    docker::fetch_image_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
                InspectKind::Volume => {
                    docker::fetch_volume_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
                InspectKind::Network => {
                    docker::fetch_network_inspect(&conn.runner, &conn.docker, &req.arg).await
                }
            };
            let res = res.and_then(|raw| {
                serde_json::from_str::<Value>(&raw).context("inspect output was not JSON")
            });
            let _ = inspect_res_tx.send((req.key, res));
        }
    });

    let action_conn_rx = conn_tx.subscribe();
    let action_task = tokio::spawn(async move {
        let action_conn_rx = action_conn_rx;
        while let Some(req) = action_req_rx.recv().await {
            let conn = action_conn_rx.borrow().clone();
            let res = match &req {
                ActionRequest::Container { action, id } => {
                    docker::container_action(&conn.runner, &conn.docker, *action, id).await
                }
                ActionRequest::TemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_compose,
                } => {
                    async {
                        let remote_dir = match runner {
                            Runner::Local => {
                                let home = std::env::var("HOME")
                                    .map_err(|_| anyhow::anyhow!("HOME is not set"))?;
                                format!("{home}/.config/containr/apps/{name}")
                            }
                            Runner::Ssh(_) => deploy_remote_dir_for(name),
                        };
                        let remote_compose = format!("{remote_dir}/compose.yaml");
                        let mkdir_cmd = format!("mkdir -p {remote_dir}");
                        let up_cmd =
                            format!("cd {remote_dir} && {} compose up -d", docker.docker_cmd);
                        runner.run(&mkdir_cmd).await?;
                        runner.copy_file_to(local_compose, &remote_compose).await?;
                        let out = runner.run(&up_cmd).await?;
                        Ok::<_, anyhow::Error>(out)
                    }
                    .await
                }
                ActionRequest::NetTemplateDeploy {
                    name,
                    runner,
                    docker,
                    local_cfg,
                    force,
                } => {
                    async {
                        let raw = fs::read_to_string(local_cfg)
                            .with_context(|| format!("failed to read {}", local_cfg.display()))?;
                        let spec: NetworkTemplateSpec =
                            serde_json::from_str(&raw).context("network.json was not valid JSON")?;
                        let net_name = spec.name.trim();
                        anyhow::ensure!(!net_name.is_empty(), "network template: name is empty");

                        let remote_dir = match runner {
                            Runner::Local => {
                                let home = std::env::var("HOME")
                                    .map_err(|_| anyhow::anyhow!("HOME is not set"))?;
                                format!("{home}/.config/containr/networks/{name}")
                            }
                            Runner::Ssh(_) => deploy_remote_net_dir_for(name),
                        };
                        let remote_cfg = format!("{remote_dir}/network.json");
                        let mkdir_cmd = format!("mkdir -p {remote_dir}");
                        runner.run(&mkdir_cmd).await?;
                        runner.copy_file_to(local_cfg, &remote_cfg).await?;

                        let docker_cmd = &docker.docker_cmd;
                        let net_q = shell_single_quote(net_name);
                        let exists_cmd = format!("{docker_cmd} network inspect {net_q} >/dev/null 2>&1");
                        let exists = runner.run(&exists_cmd).await.is_ok();
                        if exists && !*force {
                            return Ok::<_, anyhow::Error>("exists".to_string());
                        }
                        if exists && *force {
                            let rm_cmd = format!("{docker_cmd} network rm {net_q}");
                            runner.run(&rm_cmd).await?;
                        }

                        let mut parts: Vec<String> = Vec::new();
                        parts.push(docker_cmd.clone());
                        parts.push("network".to_string());
                        parts.push("create".to_string());

                        let driver = spec
                            .driver
                            .as_deref()
                            .unwrap_or("bridge")
                            .trim()
                            .to_string();
                        parts.push("--driver".to_string());
                        parts.push(shell_single_quote(&driver));

                        if spec.internal.unwrap_or(false) {
                            parts.push("--internal".to_string());
                        }
                        if spec.attachable.unwrap_or(false) {
                            parts.push("--attachable".to_string());
                        }

                        if let Some(ipv4) = &spec.ipv4 {
                            if let Some(subnet) = ipv4.subnet.as_deref().filter(|s| !s.trim().is_empty()) {
                                parts.push("--subnet".to_string());
                                parts.push(shell_single_quote(subnet.trim()));
                            }
                            if let Some(gw) = ipv4.gateway.as_deref().filter(|s| !s.trim().is_empty()) {
                                parts.push("--gateway".to_string());
                                parts.push(shell_single_quote(gw.trim()));
                            }
                            if let Some(r) = ipv4.ip_range.as_deref().filter(|s| !s.trim().is_empty()) {
                                parts.push("--ip-range".to_string());
                                parts.push(shell_single_quote(r.trim()));
                            }
                        }

                        // Driver-specific helpers.
                        if driver == "ipvlan" {
                            let parent = spec.parent.as_deref().unwrap_or("").trim();
                            anyhow::ensure!(!parent.is_empty(), "ipvlan requires 'parent'");
                            parts.push("--opt".to_string());
                            parts.push(shell_single_quote(&format!("parent={parent}")));
                            if let Some(mode) = spec.ipvlan_mode.as_deref().filter(|s| !s.trim().is_empty()) {
                                parts.push("--opt".to_string());
                                parts.push(shell_single_quote(&format!("ipvlan_mode={}", mode.trim())));
                            }
                        }

                        if let Some(opts) = &spec.options {
                            for (k, v) in opts {
                                let k = k.trim();
                                if k.is_empty() {
                                    continue;
                                }
                                parts.push("--opt".to_string());
                                parts.push(shell_single_quote(&format!("{k}={v}")));
                            }
                        }
                        if let Some(labels) = &spec.labels {
                            for (k, v) in labels {
                                let k = k.trim();
                                if k.is_empty() {
                                    continue;
                                }
                                parts.push("--label".to_string());
                                parts.push(shell_single_quote(&format!("{k}={v}")));
                            }
                        }

                        parts.push(net_q);
                        let create_cmd = parts.join(" ");
                        let out = runner.run(&create_cmd).await?;
                        Ok::<_, anyhow::Error>(out)
                    }
                    .await
                }
                ActionRequest::ImageUntag { reference, .. } => {
                    docker::image_remove(&conn.runner, &conn.docker, reference).await
                }
                ActionRequest::ImageForceRemove { id, .. } => {
                    docker::image_remove_force(&conn.runner, &conn.docker, id).await
                }
                ActionRequest::VolumeRemove { name } => {
                    docker::volume_remove(&conn.runner, &conn.docker, name).await
                }
                ActionRequest::NetworkRemove { id } => {
                    docker::network_remove(&conn.runner, &conn.docker, id).await
                }
            };
            let _ = action_res_tx.send((req, res));
        }
    });

    let logs_conn_rx = conn_tx.subscribe();
    let logs_task = tokio::spawn(async move {
        let logs_conn_rx = logs_conn_rx;
        while let Some((id, tail)) = logs_req_rx.recv().await {
            let conn = logs_conn_rx.borrow().clone();
            let res = docker::fetch_logs(&conn.runner, &conn.docker, &id, tail.max(1)).await;
            let _ = logs_res_tx.send((id, res));
        }
    });

    let ip_conn_rx = conn_tx.subscribe();
    let ip_task = tokio::spawn(async move {
        let ip_conn_rx = ip_conn_rx;
        while let Some(ids) = ip_req_rx.recv().await {
            let conn = ip_conn_rx.borrow().clone();
            let key = conn.runner.key();
            let res = async {
                let raw = docker::fetch_inspects(&conn.runner, &conn.docker, &ids).await?;
                let v =
                    serde_json::from_str::<Value>(&raw).context("inspect output was not JSON")?;
                let arr = v
                    .as_array()
                    .context("inspect output was not a JSON array")?;
                let mut map: HashMap<String, String> = HashMap::new();
                for item in arr {
                    let id = item
                        .get("Id")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| item.get("Id").map(|x| x.to_string()))
                        .unwrap_or_default();
                    if id.is_empty() {
                        continue;
                    }
                    if let Some(ip) = extract_container_ip(item) {
                        map.insert(id, ip);
                    }
                }
                Ok::<_, anyhow::Error>(map)
            }
            .await;
            let _ = ip_res_tx.send((key, res));
        }
    });

    let usage_conn_rx = conn_tx.subscribe();
    let usage_task = tokio::spawn(async move {
        let usage_conn_rx = usage_conn_rx;
        while let Some(ids) = usage_req_rx.recv().await {
            let conn = usage_conn_rx.borrow().clone();
            let key = conn.runner.key();
            let res = async {
                const CHUNK: usize = 40;
                let mut snapshot = UsageSnapshot::default();

                for chunk in ids.chunks(CHUNK) {
                    let raw = docker::fetch_inspects(&conn.runner, &conn.docker, chunk).await?;
                    let v = serde_json::from_str::<Value>(&raw)
                        .context("inspect output was not JSON")?;
                    let arr = v
                        .as_array()
                        .context("inspect output was not a JSON array")?;
                    for item in arr {
                        let id = item
                            .get("Id")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        if id.is_empty() {
                            continue;
                        }
                        if let Some(ip) = extract_container_ip(item) {
                            snapshot.ip_by_container_id.insert(id.clone(), ip);
                        }

                        let running = item
                            .pointer("/State/Running")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false);

                        let image_id = item
                            .get("Image")
                            .and_then(|x| x.as_str())
                            .map(normalize_image_id)
                            .unwrap_or_default();
                        if !image_id.is_empty() {
                            *snapshot
                                .image_ref_count_by_id
                                .entry(image_id.clone())
                                .or_insert(0) += 1;
                            if running {
                                *snapshot
                                    .image_run_count_by_id
                                    .entry(image_id.clone())
                                    .or_insert(0) += 1;
                            }
                        }

                        let cname = item
                            .get("Name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.trim_start_matches('/').to_string())
                            .unwrap_or_else(|| "-".to_string());

                        if let Some(mounts) = item.get("Mounts").and_then(|x| x.as_array()) {
                            for m in mounts {
                                let ty = m.get("Type").and_then(|x| x.as_str()).unwrap_or("");
                                if ty != "volume" {
                                    continue;
                                }
                                let name = m.get("Name").and_then(|x| x.as_str()).unwrap_or("");
                                if name.trim().is_empty() {
                                    continue;
                                }
                                let name = name.trim().to_string();
                                *snapshot
                                    .volume_ref_count_by_name
                                    .entry(name.clone())
                                    .or_insert(0) += 1;
                                if running {
                                    *snapshot
                                        .volume_run_count_by_name
                                        .entry(name.clone())
                                        .or_insert(0) += 1;
                                }
                                snapshot
                                    .volume_containers_by_name
                                    .entry(name)
                                    .or_default()
                                    .push(cname.clone());
                            }
                        }
                    }
                }

                for v in snapshot.volume_containers_by_name.values_mut() {
                    v.sort();
                    v.dedup();
                }

                Ok::<_, anyhow::Error>(snapshot)
            }
            .await;
            let _ = usage_res_tx.send((key, res));
        }
    });

    let _ = refresh_tx.send(());

    loop {
        if app.should_quit {
            break;
        }
        // Avoid stale "in-progress" markers if the background action result gets lost.
        let now = Instant::now();
        app.action_inflight.retain(|_, m| now < m.until);
        app.image_action_inflight.retain(|_, m| now < m.until);
        app.volume_action_inflight.retain(|_, m| now < m.until);
        app.network_action_inflight.retain(|_, m| now < m.until);

        while let Ok((key, res)) = result_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            match res {
                Ok((containers, images, volumes, networks)) => {
                    app.images = images;
                    app.volumes = volumes;
                    app.networks = networks;
                    app.images_selected = app
                        .images_selected
                        .min(app.images_visible_len().saturating_sub(1));
                    app.volumes_selected = app
                        .volumes_selected
                        .min(app.volumes_visible_len().saturating_sub(1));
                    app.networks_selected = app
                        .networks_selected
                        .min(app.networks.len().saturating_sub(1));

                    app.set_containers(containers);
                    app.prune_image_marks();
                    app.prune_volume_marks();
                    app.prune_network_marks();
                    app.usage_refresh_needed = true;
                    app.reconcile_noncontainer_action_markers();
                    app.last_refresh = Some(Instant::now());
                    app.clear_conn_error();
                    app.clear_last_error();
                }
                Err(e) => {
                    app.loading = false;
                    app.loading_since = None;
                    app.set_conn_error(format!("{:#}", e));
                }
            }
        }

        while let Ok((key, res)) = ip_res_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            match res {
                Ok(map) => {
                    let now = Instant::now();
                    for (id, ip) in map {
                        app.ip_cache.insert(id, (ip, now));
                    }
                }
                Err(e) => {
                    // Non-fatal; keep the table responsive.
                    app.set_warn(format!("ip lookup failed: {:#}", e));
                }
            }
        }

        while let Ok((key, res)) = usage_res_rx.try_recv() {
            if key != app.current_target {
                continue;
            }
            app.usage_loading = false;
            match res {
                Ok(snap) => {
                    // Apply IPs as a bonus (so the container table can show IP faster).
                    let now = Instant::now();
                    for (id, ip) in snap.ip_by_container_id {
                        app.ip_cache.insert(id, (ip, now));
                    }

                    // Images by ImageID.
                    app.image_referenced_by_id.clear();
                    app.image_referenced_count_by_id.clear();
                    app.image_running_count_by_id.clear();
                    for img in &app.images {
                        let id = normalize_image_id(&img.id);
                        let refs = snap.image_ref_count_by_id.get(&id).copied().unwrap_or(0);
                        let runs = snap.image_run_count_by_id.get(&id).copied().unwrap_or(0);
                        app.image_referenced_by_id.insert(img.id.clone(), refs > 0);
                        app.image_referenced_count_by_id
                            .insert(img.id.clone(), refs);
                        app.image_running_count_by_id.insert(img.id.clone(), runs);
                    }

                    // Volumes by name.
                    app.volume_referenced_by_name.clear();
                    app.volume_referenced_count_by_name.clear();
                    app.volume_running_count_by_name.clear();
                    app.volume_containers_by_name.clear();
                    for v in &app.volumes {
                        let refs = snap
                            .volume_ref_count_by_name
                            .get(&v.name)
                            .copied()
                            .unwrap_or(0);
                        let runs = snap
                            .volume_run_count_by_name
                            .get(&v.name)
                            .copied()
                            .unwrap_or(0);
                        let ctrs = snap
                            .volume_containers_by_name
                            .get(&v.name)
                            .cloned()
                            .unwrap_or_default();
                        app.volume_referenced_by_name
                            .insert(v.name.clone(), refs > 0);
                        app.volume_referenced_count_by_name
                            .insert(v.name.clone(), refs);
                        app.volume_running_count_by_name
                            .insert(v.name.clone(), runs);
                        app.volume_containers_by_name.insert(v.name.clone(), ctrs);
                    }

                    // Clamp selections in case the unused-only toggles depend on usage.
                    app.images_selected = app
                        .images_selected
                        .min(app.images_visible_len().saturating_sub(1));
                    app.volumes_selected = app
                        .volumes_selected
                        .min(app.volumes_visible_len().saturating_sub(1));
                    app.usage_refresh_needed = false;
                }
                Err(e) => {
                    app.set_warn(format!("usage lookup failed: {:#}", e));
                }
            }
        }

        // Kick off IP refresh opportunistically after container list updates.
        if app.ip_refresh_needed && !app.containers.is_empty() {
            const TTL: Duration = Duration::from_secs(60);
            const MAX_IDS: usize = 40;
            let now = Instant::now();
            let mut ids: Vec<String> = Vec::new();
            for c in &app.containers {
                if is_container_stopped(&c.status) {
                    continue;
                }
                let expired = app
                    .ip_cache
                    .get(&c.id)
                    .map(|(_, at)| now.duration_since(*at) > TTL)
                    .unwrap_or(true);
                if expired {
                    ids.push(c.id.clone());
                    if ids.len() >= MAX_IDS {
                        break;
                    }
                }
            }
            if !ids.is_empty() {
                let _ = ip_req_tx.send(ids);
            }
            app.ip_refresh_needed = false;
        }

        // Kick off usage refresh after overview updates (accurate image/volume usage).
        if app.usage_refresh_needed && !app.containers.is_empty() {
            const MAX_IDS: usize = 200;
            let ids: Vec<String> = app
                .containers
                .iter()
                .take(MAX_IDS)
                .map(|c| c.id.clone())
                .collect();
            if !ids.is_empty() {
                app.usage_loading = true;
                let _ = usage_req_tx.send(ids);
            }
            app.usage_refresh_needed = false;
        }

        while let Ok((id, res)) = inspect_res_rx.try_recv() {
            if app.inspect_for_id.as_deref() != Some(&id) {
                continue;
            }
            app.inspect_loading = false;
            match res {
                Ok(value) => {
                    app.inspect_value = Some(value);
                    app.inspect_error = None;
                    app.rebuild_inspect_lines();
                }
                Err(e) => {
                    app.inspect_value = None;
                    let msg = format!("{:#}", e);
                    app.inspect_error = Some(msg.clone());
                    app.log_msg(MsgLevel::Error, format!("inspect failed: {msg}"));
                    app.rebuild_inspect_lines();
                }
            }
        }

        while let Ok((req, res)) = action_res_rx.try_recv() {
            match res {
                Ok(out) => {
                    app.clear_last_error();
                    if let ActionRequest::TemplateDeploy { name, .. } = &req {
                        app.template_deploy_inflight.remove(name);
                        app.set_info(format!("deployed template {name}"));
                    }
                    if let ActionRequest::NetTemplateDeploy { name, .. } = &req {
                        app.net_template_deploy_inflight.remove(name);
                        if out.trim() == "exists" {
                            app.set_warn(format!(
                                "network '{name}' already exists (use :nettemplate deploy! to recreate)"
                            ));
                        } else {
                            app.set_info(format!("deployed network template {name}"));
                        }
                    }
                    // Keep container "in-flight" markers for a short time; the next refresh will
                    // replace the status. For other kinds we just refresh.
                    let _ = refresh_tx.send(());
                }
                Err(e) => {
                    match &req {
                        ActionRequest::Container { id, .. } => {
                            app.action_inflight.remove(id);
                        }
                        ActionRequest::TemplateDeploy { name, .. } => {
                            app.template_deploy_inflight.remove(name);
                            app.set_error(format!("deploy failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::NetTemplateDeploy { name, .. } => {
                            app.net_template_deploy_inflight.remove(name);
                            app.set_error(format!("deploy failed for {name}: {:#}", e));
                            continue;
                        }
                        ActionRequest::ImageUntag { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                        }
                        ActionRequest::ImageForceRemove { marker_key, .. } => {
                            app.image_action_inflight.remove(marker_key);
                        }
                        ActionRequest::VolumeRemove { name } => {
                            app.volume_action_inflight.remove(name);
                        }
                        ActionRequest::NetworkRemove { id } => {
                            app.network_action_inflight.remove(id);
                        }
                    }
                    app.set_error(format!("{:#}", e));
                }
            }
        }

        while let Ok((id, res)) = logs_res_rx.try_recv() {
            if app.logs_for_id.as_deref() != Some(&id) {
                continue;
            }
            app.logs_loading = false;
            match res {
                Ok(text) => {
                    app.logs_max_width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                    app.logs_text = Some(text);
                    app.logs_error = None;
                    if app.logs_cursor >= app.logs_total_lines() {
                        app.logs_cursor = app.logs_total_lines().saturating_sub(1);
                    }
                    app.logs_rebuild_matches();
                }
                Err(e) => {
                    app.logs_text = None;
                    let msg = format!("{:#}", e);
                    app.logs_error = Some(msg.clone());
                    app.log_msg(MsgLevel::Error, format!("logs failed: {msg}"));
                    app.logs_cursor = 0;
                    app.logs_hscroll = 0;
                    app.logs_max_width = 0;
                    app.logs_rebuild_matches();
                }
            }
        }

        let refresh_display = Duration::from_secs(app.refresh_secs.max(1));
        terminal.draw(|f| draw(f, &mut app, refresh_display))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    handle_shell_key(
                        &mut app,
                        key,
                        &conn_tx,
                        &refresh_tx,
                        &refresh_interval_tx,
                        &inspect_req_tx,
                        &logs_req_tx,
                        &action_req_tx,
                    );
                    if let Some(req) = app.shell_pending_interactive.take() {
                        let runner = current_runner_from_app(&app);
                        restore_terminal(&mut terminal, mouse_enabled)?;
                        let res = match req {
                            ShellInteractive::RunCommand { cmd } => {
                                run_interactive_command(&runner, &cmd)
                            }
                            ShellInteractive::RunLocalCommand { cmd } => {
                                run_interactive_local_command(&cmd)
                            }
                        };
                        terminal = setup_terminal(mouse_enabled)?;
                        if let Some(name) = app.templates_refresh_after_edit.take() {
                            app.refresh_templates();
                            if let Some(idx) = app.templates.iter().position(|t| t.name == name) {
                                app.templates_selected = idx;
                            }
                        }
                        if let Some(name) = app.net_templates_refresh_after_edit.take() {
                            app.refresh_net_templates();
                            if let Some(idx) = app.net_templates.iter().position(|t| t.name == name)
                            {
                                app.net_templates_selected = idx;
                            }
                        }
                        if let Some(name) = app.theme_refresh_after_edit.take() {
                            commands::theme_cmd::reload_active_theme_after_edit(&mut app, &name);
                        }
                        if let Err(e) = res {
                            app.set_error(format!("{:#}", e));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fetch_task.abort();
    inspect_task.abort();
    action_task.abort();
    logs_task.abort();
    ip_task.abort();
    usage_task.abort();
    restore_terminal(&mut terminal, mouse_enabled).context("failed to restore terminal")?;
    Ok(())
}

fn setup_terminal(mouse_enabled: bool) -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse_enabled {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}
fn run_interactive_command(runner: &Runner, cmd: &str) -> anyhow::Result<()> {
    match runner {
        Runner::Ssh(ssh) => {
            let mut c = StdCommand::new("ssh");
            // Allocate a tty for interactive docker exec.
            c.arg("-t");
            if let Some(port) = ssh.port {
                c.arg("-p").arg(port.to_string());
            }
            if let Some(identity) = &ssh.identity {
                c.arg("-i").arg(identity);
            }
            c.arg(&ssh.target).arg("--").arg(cmd);
            c.stdin(Stdio::inherit());
            c.stdout(Stdio::inherit());
            c.stderr(Stdio::inherit());
            let status = c.status().context("failed to run ssh")?;
            if !status.success() {
                anyhow::bail!("ssh exited with {}", status);
            }
            Ok(())
        }
        Runner::Local => {
            let status = StdCommand::new("sh")
                .arg("-lc")
                .arg(cmd)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .context("failed to run local command")?;
            if !status.success() {
                anyhow::bail!("local command exited with {}", status);
            }
            Ok(())
        }
    }
}

fn run_interactive_local_command(cmd: &str) -> anyhow::Result<()> {
    let status = StdCommand::new("sh")
        .arg("-lc")
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run local command")?;
    if !status.success() {
        anyhow::bail!("local command exited with {}", status);
    }
    Ok(())
}

fn shell_single_quote(s: &str) -> String {
    // Produce a POSIX-shell-safe single-quoted string literal.
    // Example: abc'd -> 'abc'"'"'d'
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn current_runner_from_app(app: &App) -> Runner {
    if let Some(name) = &app.active_server {
        if let Some(s) = app.servers.iter().find(|x| &x.name == name) {
            if s.target == "local" {
                return Runner::Local;
            }
            return Runner::Ssh(Ssh {
                target: s.target.clone(),
                identity: s.identity.clone(),
                port: s.port,
            });
        }
    }
    if app.current_target == "local" {
        Runner::Local
    } else {
        Runner::Ssh(Ssh {
            target: app.current_target.clone(),
            identity: None,
            port: None,
        })
    }
}

fn current_docker_cmd_from_app(app: &App) -> String {
    if let Some(name) = &app.active_server {
        if let Some(s) = app.servers.iter().find(|x| &x.name == name) {
            return s.docker_cmd.clone();
        }
    }
    "docker".to_string()
}

fn current_server_label(app: &App) -> String {
    app.active_server
        .as_deref()
        .unwrap_or_else(|| app.current_target.as_str())
        .to_string()
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mouse_enabled: bool,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    if mouse_enabled {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw(f: &mut ratatui::Frame, app: &mut App, refresh: Duration) {
    draw_shell(f, app, refresh);
}

fn draw_shell(f: &mut ratatui::Frame, app: &mut App, refresh: Duration) {
    // Shell UI: header + sidebar + main + footer + command line. No overlays/dialogs.
    let area = f.area();
    let bg = app.theme.background.to_style();
    f.render_widget(Block::default().style(bg), area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),    // body
            Constraint::Length(1), // footer
            Constraint::Length(1), // cmdline
        ])
        .split(area);

    draw_shell_header(f, app, refresh, rows[0]);
    draw_shell_body(f, app, rows[1]);
    draw_shell_footer(f, app, rows[2]);
    draw_shell_cmdline(f, app, rows[3]);
}

fn shell_sidebar_items(app: &App) -> Vec<ShellSidebarItem> {
    let mut items: Vec<ShellSidebarItem> = Vec::new();
    for i in 0..app.servers.len() {
        items.push(ShellSidebarItem::Server(i));
    }
    items.push(ShellSidebarItem::Separator);
    items.push(ShellSidebarItem::Module(ShellView::Containers));
    items.push(ShellSidebarItem::Module(ShellView::Images));
    items.push(ShellSidebarItem::Module(ShellView::Volumes));
    items.push(ShellSidebarItem::Module(ShellView::Networks));
    items.push(ShellSidebarItem::Module(ShellView::Inspect));
    items.push(ShellSidebarItem::Module(ShellView::Logs));
    items.push(ShellSidebarItem::Gap);
    items.push(ShellSidebarItem::Module(ShellView::Templates));
    // Help is accessible via :? / :help (not a module entry).

    let actions: Vec<ShellAction> = match app.shell_view {
        ShellView::Containers => vec![
            ShellAction::Start,
            ShellAction::Stop,
            ShellAction::Restart,
            ShellAction::Delete,
            ShellAction::Console,
        ],
        ShellView::Images => vec![ShellAction::ImageUntag, ShellAction::ImageForceRemove],
        ShellView::Volumes => vec![ShellAction::VolumeRemove],
        ShellView::Networks => vec![ShellAction::NetworkRemove],
        ShellView::Templates => vec![
            ShellAction::TemplateEdit,
            ShellAction::TemplateNew,
            ShellAction::TemplateDelete,
            ShellAction::TemplateDeploy,
        ],
        ShellView::Inspect | ShellView::Logs | ShellView::Help => vec![],
        ShellView::Messages => vec![],
    };
    if !actions.is_empty() {
        items.push(ShellSidebarItem::Separator);
        for a in actions {
            items.push(ShellSidebarItem::Action(a));
        }
    }
    items
}

fn shell_is_selectable(item: ShellSidebarItem) -> bool {
    !matches!(item, ShellSidebarItem::Separator | ShellSidebarItem::Gap)
}

fn shell_move_sidebar(app: &mut App, dir: i32) {
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

fn shell_cycle_focus(app: &mut App) {
    app.shell_focus = match app.shell_focus {
        ShellFocus::Sidebar => ShellFocus::List,
        ShellFocus::List => ShellFocus::Details,
        ShellFocus::Details => ShellFocus::Sidebar,
    };
}

fn shell_sidebar_select_item(app: &mut App, target: ShellSidebarItem) {
    let items = shell_sidebar_items(app);
    if let Some((idx, _)) = items
        .iter()
        .enumerate()
        .find(|(_, it)| **it == target && shell_is_selectable(**it))
    {
        app.shell_sidebar_selected = idx;
    }
}

fn shell_set_main_view(app: &mut App, view: ShellView) {
    app.shell_view = view;
    if !matches!(
        view,
        ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages
    ) {
        app.shell_last_main_view = view;
    }
    app.shell_focus = ShellFocus::List;
    app.active_view = match view {
        ShellView::Containers => ActiveView::Containers,
        ShellView::Images => ActiveView::Images,
        ShellView::Volumes => ActiveView::Volumes,
        ShellView::Networks => ActiveView::Networks,
        ShellView::Templates => app.active_view,
        ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages => {
            app.active_view
        }
    };
    if view == ShellView::Templates {
        app.refresh_templates();
        app.refresh_net_templates();
    }
    if let Some(mode) = app.get_view_split_mode(view) {
        app.shell_split_mode = mode;
    }
}

fn shell_first_container_id(app: &mut App) -> Option<String> {
    if let Some(c) = app.selected_container() {
        return Some(c.id.clone());
    }
    if app.active_view != ActiveView::Containers {
        app.active_view = ActiveView::Containers;
    }
    if app.containers.is_empty() {
        return None;
    }
    if app.list_mode == ListMode::Tree {
        app.ensure_view();
        if let Some((idx, ViewEntry::Container { id, .. })) = app
            .view
            .iter()
            .enumerate()
            .find(|(_, e)| matches!(e, ViewEntry::Container { .. }))
        {
            app.selected = idx;
            return Some(id.clone());
        }
    }
    app.selected = app.selected.min(app.containers.len().saturating_sub(1));
    Some(app.containers.get(app.selected)?.id.clone())
}

fn shell_enter_logs(app: &mut App, logs_req_tx: &mpsc::UnboundedSender<(String, usize)>) {
    // Logs are container-only; always use the containers selection.
    shell_set_main_view(app, ShellView::Containers);
    app.shell_view = ShellView::Logs;
    app.shell_focus = ShellFocus::List;
    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Logs));

    let Some(id) = shell_first_container_id(app) else {
        app.logs_loading = false;
        app.logs_error = Some("no container selected".to_string());
        app.logs_text = None;
        return;
    };
    app.open_logs_state(id.clone());
    let _ = logs_req_tx.send((id, app.logs_tail.max(1)));
}

fn shell_enter_inspect(app: &mut App, inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>) {
    // Inspect follows the current main view selection.
    if matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
        app.shell_view = app.shell_last_main_view;
    }
    app.shell_view = ShellView::Inspect;
    app.shell_focus = ShellFocus::List;
    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Inspect));

    let Some(target) = app.selected_inspect_target() else {
        app.inspect_loading = false;
        app.inspect_error = Some("nothing selected".to_string());
        app.inspect_value = None;
        app.inspect_lines.clear();
        return;
    };
    app.open_inspect_state(target.clone());
    let _ = inspect_req_tx.send(target);
}

fn shell_back_from_full(app: &mut App) {
    if matches!(
        app.shell_view,
        ShellView::Logs | ShellView::Inspect | ShellView::Help | ShellView::Messages
    ) {
        // Full-screen views should never keep command-line mode active in the background.
        app.shell_cmd_mode = false;
        app.shell_confirm = None;
        app.shell_view = if app.shell_view == ShellView::Help {
            app.shell_help_return
        } else if app.shell_view == ShellView::Messages {
            app.shell_msgs_return
        } else {
            app.shell_last_main_view
        };
        app.shell_focus = ShellFocus::List;
        shell_sidebar_select_item(app, ShellSidebarItem::Module(app.shell_view));
    }
}

fn shell_switch_server(
    app: &mut App,
    idx: usize,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
) {
    let Some(s) = app.servers.get(idx).cloned() else {
        return;
    };
    app.server_selected = idx;
    app.active_server = Some(s.name.clone());
    app.clear_all_marks();
    app.action_inflight.clear();
    app.image_action_inflight.clear();
    app.volume_action_inflight.clear();
    app.network_action_inflight.clear();

    let runner = if s.target == "local" {
        Runner::Local
    } else {
        Runner::Ssh(Ssh {
            target: s.target.clone(),
            identity: s.identity.clone(),
            port: s.port,
        })
    };
    app.current_target = runner.key();
    app.clear_conn_error();
    app.start_loading(true);
    let _ = conn_tx.send(Connection {
        runner,
        docker: DockerCfg {
            docker_cmd: s.docker_cmd,
        },
    });

    // Persist last_server only; no secrets stored.
    app.persist_config();
    let _ = refresh_tx.send(());

    shell_set_main_view(app, ShellView::Containers);
    shell_sidebar_select_item(app, ShellSidebarItem::Server(idx));
}

fn shell_refresh(app: &mut App, refresh_tx: &mpsc::UnboundedSender<()>) {
    app.start_loading(true);
    let _ = refresh_tx.send(());
}

impl App {
    fn persist_config(&mut self) {
        let cfg = ContainrConfig {
            version: 9,
            last_server: self.active_server.clone(),
            refresh_secs: self.refresh_secs.max(1),
            logs_tail: self.logs_tail.max(1),
            cmd_history_max: self.cmd_history_max_effective(),
            cmd_history: self.shell_cmd_history.entries.clone(),
            active_theme: self.theme_name.clone(),
            templates_dir: self.templates_dir.to_string_lossy().to_string(),
            view_layout: self
                .shell_split_by_view
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            ShellSplitMode::Horizontal => "horizontal".to_string(),
                            ShellSplitMode::Vertical => "vertical".to_string(),
                        },
                    )
                })
                .collect(),
            keymap: self.keymap.clone(),
            servers: self.servers.clone(),
        };
        if let Err(e) = config::save(&self.config_path, &cfg) {
            self.set_error(format!("failed to save config: {:#}", e));
        }
    }
}

fn find_server_by_name(servers: &[ServerEntry], name: &str) -> Option<usize> {
    servers.iter().position(|s| s.name == name)
}

fn ensure_unique_server_name(servers: &[ServerEntry], desired: &str) -> Option<String> {
    let desired = desired.trim();
    if desired.is_empty() {
        return None;
    }
    if !servers.iter().any(|s| s.name == desired) {
        return Some(desired.to_string());
    }
    None
}

fn create_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let dir = stacks_dir.join(name);
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;
    let compose = dir.join("compose.yaml");
    let skeleton = r#"# Stack template (docker compose)
# description: REPLACE_WITH_A_SHORT_DESCRIPTION
#
# Tips:
# - Keep values simple and edit after creation.
# - Add more services as needed.
# - Use named volumes for persistent data.
#
# Docs: https://docs.docker.com/compose/compose-file/

name: REPLACE_STACK_NAME

services:
  app:
    image: REPLACE_IMAGE:latest
    container_name: REPLACE_CONTAINER_NAME
    restart: unless-stopped

    # Optional: publish ports (host:container)
    ports:
      - "8080:80"

    # Optional: environment variables
    environment:
      TZ: "UTC"
      EXAMPLE: "value"

    # Optional: bind-mounts or named volumes
    volumes:
      - app_data:/var/lib/app

    # Optional: networks (useful when you run multiple services)
    networks:
      - app_net

    # Optional: healthcheck
    healthcheck:
      test: ["CMD", "sh", "-lc", "curl -fsS http://localhost/ || exit 1"]
      interval: 30s
      timeout: 5s
      retries: 3

    # Optional: labels (containr can add its own labels during deploy later)
    labels:
      com.example.stack: "REPLACE_STACK_NAME"

volumes:
  app_data:
    driver: local

networks:
  app_net:
    driver: bridge
"#;
    fs::write(&compose, skeleton)?;
    Ok(())
}

fn create_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let root = app.net_templates_dir();
    fs::create_dir_all(&root)?;
    let dir = root.join(name);
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;

    let cfg = dir.join("network.json");
    let skeleton = format!(
        r#"{{
  "description": "Shared network template (edit me)",
  "name": "{name}",
  "driver": "ipvlan",
  "parent": "eth0.10",
  "ipvlan_mode": "l2",
  "ipv4": {{
    "subnet": "192.168.10.0/24",
    "gateway": "192.168.10.1",
    "ip_range": null
  }},
  "internal": null,
  "attachable": null,
  "options": {{}},
  "labels": {{}}
}}
"#
    );
    fs::write(&cfg, skeleton)?;
    Ok(())
}

fn deploy_remote_dir_for(name: &str) -> String {
    format!("~/.config/containr/apps/{name}")
}

fn deploy_remote_net_dir_for(name: &str) -> String {
    format!("~/.config/containr/networks/{name}")
}

fn delete_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let dir = stacks_dir.join(name);
    anyhow::ensure!(dir.exists(), "template does not exist: {}", dir.display());

    let root = fs::canonicalize(&stacks_dir)?;
    let target = fs::canonicalize(&dir)?;
    anyhow::ensure!(
        target.starts_with(&root),
        "refusing to delete outside templates dir"
    );

    fs::remove_dir_all(&target)?;
    Ok(())
}

fn delete_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");

    let root = app.net_templates_dir();
    fs::create_dir_all(&root)?;
    let dir = root.join(name);
    anyhow::ensure!(dir.exists(), "template does not exist: {}", dir.display());

    let root_can = fs::canonicalize(&root)?;
    let dir_can = fs::canonicalize(&dir)?;
    anyhow::ensure!(
        dir_can.starts_with(&root_can),
        "refusing to delete outside templates dir"
    );

    fs::remove_dir_all(&dir_can)?;
    Ok(())
}

fn parse_kv_args(
    mut it: impl Iterator<Item = String>,
) -> (Option<u16>, Option<String>, Option<String>, Vec<String>) {
    // Supports: -p <port>  -i <identity>  --cmd <docker_cmd>
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut docker_cmd: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "-p" => {
                if let Some(v) = it.next() {
                    port = v.parse::<u16>().ok();
                }
            }
            "-i" => {
                if let Some(v) = it.next() {
                    identity = Some(v);
                }
            }
            "--cmd" => {
                if let Some(v) = it.next() {
                    docker_cmd = Some(v);
                }
            }
            _ => rest.push(tok),
        }
    }
    (port, identity, docker_cmd, rest)
}

fn extract_template_description(path: &PathBuf) -> Option<String> {
    // Heuristic: find a "# description: ..." (or "# desc: ...") line near the top of compose.yaml.
    let data = fs::read_to_string(path).ok()?;
    for line in data.lines().take(40) {
        let l = line.trim_start();
        if !l.starts_with('#') {
            // Stop early once we hit non-comment content.
            if !l.is_empty() {
                break;
            }
            continue;
        }
        let body = l.trim_start_matches('#').trim_start();
        let low = body.to_ascii_lowercase();
        let key = if low.starts_with("description:") {
            "description:"
        } else if low.starts_with("desc:") {
            "desc:"
        } else {
            continue;
        };
        let value = body[key.len()..].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn extract_net_template_description(path: &PathBuf) -> Option<String> {
    let data = fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;
    let d = v.get("description")?.as_str()?.trim();
    if d.is_empty() {
        None
    } else {
        Some(d.to_string())
    }
}

fn shell_is_safe_token(s: &str) -> bool {
    // For interactive shells we only accept simple command tokens.
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
}

fn shell_escape_double_quoted(s: &str) -> String {
    // Escape for inclusion inside double quotes in a POSIX shell script.
    // We escape: backslash, double quote, dollar, backtick.
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' | '"' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn shell_open_console(app: &mut App, user: Option<&str>, shell: &str) {
    let Some(c) = app.selected_container() else {
        app.set_warn("no container selected");
        return;
    };
    if is_container_stopped(&c.status) {
        app.set_warn("container is not running");
        return;
    }
    if !shell_is_safe_token(shell) {
        app.set_warn("invalid shell");
        return;
    }
    let docker_cmd = current_docker_cmd_from_app(app);
    let id = shell_single_quote(&c.id);
    let server = current_server_label(app);
    // Bash interprets prompt escapes like \\e and needs \\[ \\] wrappers for correct line editing.
    let ps1_bash = format!(
        "\\[\\e[37m\\]docker:\\[\\e[0m\\]\\[\\e[32m\\]{}\\[\\e[37m\\]@{}\\[\\e[0m\\]$ ",
        c.name, server
    );
    let ps1_bash = shell_single_quote(&ps1_bash);

    let user_part = user
        .filter(|u| !u.trim().is_empty())
        .map(|u| format!("-u {}", shell_single_quote(u.trim())))
        .unwrap_or_default();

    let shell_cmd = if shell == "bash" {
        format!("env PS1={ps1_bash} bash --noprofile --norc -i")
    } else if shell == "sh" {
        // POSIX sh typically does NOT interpret \\e-style escapes in PS1. We set PS1 via printf
        // using %b so that \\033 sequences become real ESC bytes, then exec an interactive sh.
        // Important: avoid nested single quotes here, because this command is embedded into other
        // shell layers (ssh/sh -lc).
        let ps1_sh_raw = format!(
            "\\033[37mdocker:\\033[0m\\033[32m{}\\033[37m@{}\\033[0m\\$ ",
            c.name, server
        );
        let ps1_sh = shell_escape_double_quoted(&ps1_sh_raw);
        format!("sh -lc 'export PS1=\"$(printf \"%b\" \"{ps1_sh}\")\"; exec sh -i'")
    } else {
        // Best-effort generic token. Prompt coloring depends on the shell.
        format!("env PS1={ps1_bash} {shell}")
    };

    let cmd = if user_part.is_empty() {
        format!("{docker_cmd} exec -it {id} {shell_cmd}")
    } else {
        format!("{docker_cmd} exec -it {user_part} {id} {shell_cmd}")
    };
    app.shell_pending_interactive = Some(ShellInteractive::RunCommand { cmd });
}

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

fn shell_execute_cmdline(
    app: &mut App,
    cmdline: &str,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    refresh_interval_tx: &watch::Sender<Duration>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return;
    }
    let cmdline = cmdline.trim_start_matches(':').trim();
    let cmdline_full = cmdline.to_string();

    let mut it = cmdline.split_whitespace();
    let Some(cmd_raw) = it.next() else {
        return;
    };
    let (cmd, force) = if cmd_raw == "!" {
        let Some(next) = it.next() else {
            app.set_warn("usage: :! <command>");
            return;
        };
        (next, true)
    } else if let Some(rest) = cmd_raw.strip_prefix('!') {
        if rest.is_empty() {
            app.set_warn("usage: :! <command>");
            return;
        }
        (rest, true)
    } else if let Some(stripped) = cmd_raw.strip_suffix('!') {
        (stripped, true)
    } else {
        (cmd_raw, false)
    };

    match cmd {
        "q" => {
            if force {
                app.should_quit = true;
            } else {
                app.shell_cmd_mode = true;
                app.shell_cmd_input.clear();
                app.shell_cmd_cursor = 0;
                app.shell_confirm = Some(ShellConfirm {
                    label: "quit".to_string(),
                    cmdline: cmdline_full,
                });
            }
            return;
        }
        "?" | "help" => {
            // Ensure we don't get "stuck" in command-line mode while the Help view is active.
            // Otherwise 'q' is treated as input and won't close Help.
            app.shell_cmd_mode = false;
            app.shell_confirm = None;
            app.shell_cmd_input.clear();
            app.shell_cmd_cursor = 0;
            app.shell_help_return = app.shell_view;
            app.shell_view = ShellView::Help;
            app.shell_focus = ShellFocus::List;
            app.shell_help_scroll = 0;
            return;
        }
        "messages" | "msgs" => {
            let sub = it.next().unwrap_or("");
            if sub == "copy" {
                app.messages_copy_selected();
                return;
            }
            // Messages is a full-screen view; leaving cmdline mode avoids confusing key handling.
            app.shell_cmd_mode = false;
            app.shell_confirm = None;
            app.shell_cmd_input.clear();
            app.shell_cmd_cursor = 0;
            if app.shell_view == ShellView::Messages {
                shell_back_from_full(app);
            } else {
                app.shell_msgs_return = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs_scroll = usize::MAX;
                app.shell_msgs_hscroll = 0;
            }
            return;
        }
        "refresh" => {
            if app.shell_view == ShellView::Templates {
                match app.templates_kind {
                    TemplatesKind::Stacks => app.refresh_templates(),
                    TemplatesKind::Networks => app.refresh_net_templates(),
                }
            } else {
                shell_refresh(app, refresh_tx);
            }
            return;
        }
        "theme" => {
            let sub = it.next().unwrap_or("");
            if sub.is_empty() || sub == "help" {
                app.set_info(format!("active theme: {}", app.theme_name));
                app.set_info("usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>");
                app.shell_msgs_return = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs_scroll = usize::MAX;
                return;
            }
            match sub {
                "list" => match theme::list_theme_names(&app.config_path) {
                    Ok(mut names) => {
                        if names.is_empty() {
                            app.set_info("no themes found");
                        } else {
                            // Ensure default is always visible.
                            if !names.iter().any(|n| n == "default") {
                                names.insert(0, "default".to_string());
                            }
                            app.set_info("Themes:");
                            for n in names {
                                if n == app.theme_name {
                                    app.set_info(format!("* {n} (active)"));
                                } else {
                                    app.set_info(format!("  {n}"));
                                }
                            }
                        }
                        app.shell_msgs_return = app.shell_view;
                        app.shell_view = ShellView::Messages;
                        app.shell_focus = ShellFocus::List;
                        app.shell_msgs_scroll = usize::MAX;
                    }
                    Err(e) => app.set_error(format!("theme list failed: {:#}", e)),
                },
                "use" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme use <name>");
                        return;
                    };
                    if let Err(e) = commands::theme_cmd::set_theme(app, name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                "new" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme new <name>");
                        return;
                    };
                    if let Err(e) = commands::theme_cmd::new_theme(app, name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                "edit" => {
                    let name = it
                        .next()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| app.theme_name.clone());
                    if let Err(e) = commands::theme_cmd::edit_theme(app, &name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                "rm" | "del" | "delete" => {
                    let Some(name) = it.next() else {
                        app.set_warn("usage: :theme rm <name>");
                        return;
                    };
                    if name == "default" {
                        app.set_warn("cannot delete default theme");
                        return;
                    }
                    if !force {
                        shell_begin_confirm(app, format!("theme rm {name}"), cmdline_full.clone());
                        return;
                    }
                    if let Err(e) = commands::theme_cmd::delete_theme(app, name) {
                        app.set_error(format!("{:#}", e));
                    }
                }
                _ => app.set_warn("usage: :theme list | :theme use <name> | :theme new <name> | :theme edit [name] | :theme rm <name>"),
            }
            return;
        }
        "map" => {
            let sub = it.next().unwrap_or("");
            if sub.is_empty() || sub == "help" {
                app.set_info(
                    "usage: :map [scope] <KEY> <COMMAND...>  |  :map list  |  :unmap [scope] <KEY>",
                );
                app.shell_msgs_return = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs_scroll = usize::MAX;
                return;
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
                    if !cmd.is_empty() && is_single_letter_without_modifiers(spec) && cmdline_is_destructive(&cmd) {
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
                app.shell_msgs_return = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs_scroll = usize::MAX;
                return;
            }

            // Syntax: :map [scope] <KEY> <CMD...>
            let (scope, key_str, cmd_rest) = if let Some(scope) = parse_scope(sub) {
                let Some(key_str) = it.next() else {
                    app.set_warn("usage: :map [scope] <KEY> <COMMAND...>");
                    return;
                };
                (scope, key_str, it.collect::<Vec<&str>>().join(" "))
            } else {
                (KeyScope::Global, sub, it.collect::<Vec<&str>>().join(" "))
            };
            if cmd_rest.trim().is_empty() {
                app.set_warn("usage: :map [scope] <KEY> <COMMAND...>");
                return;
            }
            let spec = match parse_key_spec(key_str) {
                Ok(s) => s,
                Err(e) => {
                    app.set_warn(format!("invalid key: {e}"));
                    return;
                }
            };
            let key_canon = format_key_spec(spec);
            let mut cmd_store = cmd_rest.trim().to_string();
            if !cmd_store.starts_with(':') {
                cmd_store = format!(":{cmd_store}");
            }
            if is_single_letter_without_modifiers(spec) && cmdline_is_destructive(&cmd_store) {
                app.set_warn(
                    "refusing to map destructive commands to a single letter without modifiers",
                );
                return;
            }

            let scope_str = scope_to_string(scope).to_string();
            app.keymap.retain(|kb| {
                parse_scope(&kb.scope) != Some(scope) || parse_key_spec(&kb.key).ok() != Some(spec)
            });
            app.keymap.push(KeyBinding {
                key: key_canon.clone(),
                scope: scope_str.clone(),
                cmd: cmd_store.clone(),
            });
            app.rebuild_keymap();
            app.persist_config();
            app.set_info(format!("mapped {scope_str} {key_canon} -> {cmd_store}"));
            app.shell_msgs_return = app.shell_view;
            app.shell_view = ShellView::Messages;
            app.shell_focus = ShellFocus::List;
            app.shell_msgs_scroll = usize::MAX;
            return;
        }
        "unmap" => {
            let Some(first) = it.next() else {
                app.set_warn("usage: :unmap [scope] <KEY>");
                return;
            };
            let (scope, key_str) = if let Some(scope) = parse_scope(first) {
                let Some(key_str) = it.next() else {
                    app.set_warn("usage: :unmap [scope] <KEY>");
                    return;
                };
                (scope, key_str)
            } else {
                (KeyScope::Global, first)
            };

            let spec = match parse_key_spec(key_str) {
                Ok(s) => s,
                Err(e) => {
                    app.set_warn(format!("invalid key: {e}"));
                    return;
                }
            };
            let scope_str = scope_to_string(scope).to_string();
            let key_canon = format_key_spec(spec);

            let mut removed = false;
            let before = app.keymap.len();
            app.keymap.retain(|kb| {
                let same = parse_scope(&kb.scope) == Some(scope)
                    && parse_key_spec(&kb.key).ok() == Some(spec);
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
            app.shell_msgs_return = app.shell_view;
            app.shell_view = ShellView::Messages;
            app.shell_focus = ShellFocus::List;
            app.shell_msgs_scroll = usize::MAX;
            return;
        }
        _ => {}
    }

    if cmd == "container" || cmd == "ctr" {
        let sub = it.next().unwrap_or("");
        match sub {
            "start" => shell_exec_container_action(app, ContainerAction::Start, action_req_tx),
            "stop" => shell_exec_container_action(app, ContainerAction::Stop, action_req_tx),
            "restart" => shell_exec_container_action(app, ContainerAction::Restart, action_req_tx),
            "rm" | "delete" | "remove" => {
                if force {
                    shell_exec_container_action(app, ContainerAction::Remove, action_req_tx)
                } else {
                    shell_begin_confirm(app, "container rm", cmdline_full.clone());
                }
            }
            "console" => {
                let mut user: Option<String> = None;
                let mut shell: Option<String> = None;
                while let Some(tok) = it.next() {
                    if tok == "-u" {
                        user = it.next().map(|s| s.to_string());
                        if user.is_none() {
                            app.set_warn("usage: :container console [-u USER] [bash|sh|SHELL]");
                            return;
                        }
                    } else if shell.is_none() {
                        shell = Some(tok.to_string());
                    } else {
                        app.set_warn("usage: :container console [-u USER] [bash|sh|SHELL]");
                        return;
                    }
                }
                let shell = shell.unwrap_or_else(|| "bash".to_string());
                let user = user.as_deref().or(Some("root"));
                shell_open_console(app, user, &shell);
            }
            "tree" => {
                app.active_view = ActiveView::Containers;
                let anchor_id = app.selected_container().map(|c| c.id.clone());
                app.list_mode = match app.list_mode {
                    ListMode::Flat => ListMode::Tree,
                    ListMode::Tree => ListMode::Flat,
                };
                app.view_dirty = true;
                app.ensure_view();
                if let Some(id) = anchor_id {
                    if app.list_mode == ListMode::Tree {
                        if let Some(idx) = app.view.iter().position(
                            |e| matches!(e, ViewEntry::Container { id: cid, .. } if cid == &id),
                        ) {
                            app.selected = idx;
                        }
                    } else if let Some(idx) = app.container_idx_by_id.get(&id).copied() {
                        app.selected = idx;
                    }
                }
            }
            _ => {
                app.set_warn(
                    "usage: :container (start|stop|restart|rm|console [bash|sh]|tree)  (uses selection/marked/stack)",
                );
            }
        }
        return;
    }

    if cmd == "image" || cmd == "img" {
        let sub = it.next().unwrap_or("");
        match sub {
            "untag" => {
                if force {
                    shell_exec_image_action(app, true, action_req_tx);
                } else {
                    shell_begin_confirm(app, "image untag", cmdline_full.clone());
                }
            }
            "rm" | "remove" | "delete" => {
                if force {
                    shell_exec_image_action(app, false, action_req_tx);
                } else {
                    shell_begin_confirm(app, "image rm", cmdline_full.clone());
                }
            }
            _ => app.set_warn("usage: :image untag | :image rm"),
        }
        return;
    }

    if cmd == "volume" || cmd == "vol" {
        let sub = it.next().unwrap_or("");
        match sub {
            "rm" | "remove" | "delete" => {
                if force {
                    shell_exec_volume_remove(app, action_req_tx);
                } else {
                    shell_begin_confirm(app, "volume rm", cmdline_full.clone());
                }
            }
            _ => app.set_warn("usage: :volume rm"),
        }
        return;
    }

    if cmd == "network" || cmd == "net" {
        let sub = it.next().unwrap_or("");
        match sub {
            "rm" | "remove" | "delete" => {
                if force {
                    shell_exec_network_remove(app, action_req_tx);
                } else {
                    // Avoid prompting when only system networks are selected/marked.
                    let any_removable = if !app.marked_networks.is_empty() {
                        app.marked_networks
                            .iter()
                            .any(|id| !app.is_system_network_id(id))
                    } else {
                        app.selected_network()
                            .map(|n| !App::is_system_network(n))
                            .unwrap_or(false)
                    };
                    if !any_removable {
                        app.set_warn("system networks cannot be modified");
                        return;
                    }
                    shell_begin_confirm(app, "network rm", cmdline_full.clone());
                }
            }
            _ => app.set_warn("usage: :network rm"),
        }
        return;
    }

    if cmd == "sidebar" {
        let sub = it.next().unwrap_or("");
        match sub {
            "toggle" => {
                app.shell_sidebar_hidden = !app.shell_sidebar_hidden;
                if app.shell_sidebar_hidden && app.shell_focus == ShellFocus::Sidebar {
                    app.shell_focus = ShellFocus::List;
                }
            }
            "compact" => app.shell_sidebar_collapsed = !app.shell_sidebar_collapsed,
            _ => app.set_warn("usage: :sidebar toggle|compact"),
        }
        return;
    }

    if cmd == "logs" {
        let sub = it.next().unwrap_or("");
        if sub == "reload" || sub == "refresh" {
            if let Some(id) = app.logs_for_id.clone() {
                app.logs_loading = true;
                let _ = logs_req_tx.send((id, app.logs_tail.max(1)));
            } else {
                app.set_warn("no logs target selected");
            }
            return;
        }
        if sub == "copy" {
            app.logs_copy_selection();
            return;
        }
        app.set_warn("usage: :logs reload|copy");
        return;
    }

    if cmd == "set" {
        let sub = it.next().unwrap_or("");
        if sub == "refresh" {
            let Some(v) = it.next() else {
                app.set_warn("usage: :set refresh <seconds>");
                return;
            };
            match v.parse::<u64>() {
                Ok(secs) if secs >= 1 && secs <= 3600 => {
                    app.refresh_secs = secs;
                    let _ = refresh_interval_tx.send(Duration::from_secs(secs));
                    app.persist_config();
                }
                _ => {
                    app.set_warn("refresh must be 1..3600");
                }
            }
            return;
        }
        if sub == "logtail" {
            let Some(v) = it.next() else {
                app.set_warn("usage: :set logtail <lines>");
                return;
            };
            match v.parse::<usize>() {
                Ok(n) if (1..=200_000).contains(&n) => {
                    app.logs_tail = n;
                    app.persist_config();
                    if app.shell_view == ShellView::Logs {
                        if let Some(id) = app.logs_for_id.clone() {
                            app.logs_loading = true;
                            let _ = logs_req_tx.send((id, app.logs_tail.max(1)));
                        }
                    }
                }
                _ => {
                    app.set_warn("logtail must be 1..200000");
                }
            }
            return;
        }
        if sub == "history" {
            let Some(v) = it.next() else {
                app.set_warn("usage: :set history <entries>");
                return;
            };
            match v.parse::<usize>() {
                Ok(n) if (1..=5000).contains(&n) => {
                    app.cmd_history_max = n;
                    // Trim existing history to the new limit.
                    let entries = app.shell_cmd_history.entries.clone();
                    app.set_cmd_history_entries(entries);
                    app.persist_config();
                }
                _ => app.set_warn("history must be 1..5000"),
            }
            return;
        }
        app.set_warn(
            "usage: :set refresh <seconds> | :set logtail <lines> | :set history <entries>",
        );
        return;
    }

    if cmd == "layout" {
        let sub = it.next().unwrap_or("toggle");
        let target_view = if matches!(
            app.shell_view,
            ShellView::Inspect | ShellView::Logs | ShellView::Help | ShellView::Messages
        ) {
            app.shell_last_main_view
        } else {
            app.shell_view
        };
        match sub.to_ascii_lowercase().as_str() {
            "h" | "hor" | "horizontal" => app.shell_split_mode = ShellSplitMode::Horizontal,
            "v" | "ver" | "vertical" => app.shell_split_mode = ShellSplitMode::Vertical,
            "toggle" => {
                app.shell_split_mode = match app.shell_split_mode {
                    ShellSplitMode::Horizontal => ShellSplitMode::Vertical,
                    ShellSplitMode::Vertical => ShellSplitMode::Horizontal,
                }
            }
            _ => {
                app.set_warn("usage: :layout [horizontal|vertical|toggle]");
                return;
            }
        }
        app.set_view_split_mode(target_view, app.shell_split_mode);
        app.persist_config();
        app.set_info(format!(
            "layout: {}",
            match app.shell_split_mode {
                ShellSplitMode::Horizontal => "horizontal",
                ShellSplitMode::Vertical => "vertical",
            }
        ));
        return;
    }

    if cmd == "templates" {
        let sub = it.next().unwrap_or("");
        if sub.is_empty() {
            app.set_info(format!(
                "templates kind: {}",
                match app.templates_kind {
                    TemplatesKind::Stacks => "stacks",
                    TemplatesKind::Networks => "networks",
                }
            ));
            return;
        }
        if sub == "toggle" {
            shell_execute_cmdline(
                app,
                "templates kind toggle",
                conn_tx,
                refresh_tx,
                refresh_interval_tx,
                logs_req_tx,
                action_req_tx,
            );
            return;
        }
        if sub == "kind" {
            // If no argument is provided, behave like "toggle" (convenient in command-line mode).
            let v = it.next().unwrap_or("toggle");
            match v.to_ascii_lowercase().as_str() {
                "stacks" | "stack" | "compose" => app.templates_kind = TemplatesKind::Stacks,
                "networks" | "network" | "net" => app.templates_kind = TemplatesKind::Networks,
                "toggle" => {
                    app.templates_kind = match app.templates_kind {
                        TemplatesKind::Stacks => TemplatesKind::Networks,
                        TemplatesKind::Networks => TemplatesKind::Stacks,
                    }
                }
                _ => {
                    app.set_warn("usage: :templates kind (stacks|networks|toggle)");
                    return;
                }
            }
            if app.templates_kind == TemplatesKind::Stacks {
                app.refresh_templates();
            } else {
                app.refresh_net_templates();
            }
            shell_set_main_view(app, ShellView::Templates);
            shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
            return;
        }
        app.set_warn("usage: :templates kind (stacks|networks|toggle)");
        return;
    }

    if cmd == "template" || cmd == "tpl" {
        let sub = it.next().unwrap_or("");
        // Alias for :templates kind ...
        if sub == "kind" {
            let v = it.next().unwrap_or("");
            shell_execute_cmdline(
                app,
                &format!("templates kind {v}"),
                conn_tx,
                refresh_tx,
                refresh_interval_tx,
                logs_req_tx,
                action_req_tx,
            );
            return;
        }
        // Convenience: :template toggle <=> :templates kind toggle
        if sub == "toggle" {
            shell_execute_cmdline(
                app,
                "templates kind toggle",
                conn_tx,
                refresh_tx,
                refresh_interval_tx,
                logs_req_tx,
                action_req_tx,
            );
            return;
        }
        if sub == "edit" {
            shell_set_main_view(app, ShellView::Templates);
            shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
            shell_edit_selected_template(app);
            return;
        }
        if sub == "new" {
            app.shell_cmd_mode = true;
            set_text_and_cursor(
                &mut app.shell_cmd_input,
                &mut app.shell_cmd_cursor,
                match app.templates_kind {
                    TemplatesKind::Stacks => "template add ".to_string(),
                    TemplatesKind::Networks => "nettemplate add ".to_string(),
                },
            );
            app.shell_confirm = None;
            return;
        }
        if sub == "add" || sub == "new" {
            let Some(name) = it.next() else {
                // Convenience: without name, open prompt for "template add".
                app.shell_cmd_mode = true;
                set_text_and_cursor(
                    &mut app.shell_cmd_input,
                    &mut app.shell_cmd_cursor,
                    match app.templates_kind {
                        TemplatesKind::Stacks => "template add ".to_string(),
                        TemplatesKind::Networks => "nettemplate add ".to_string(),
                    },
                );
                app.shell_confirm = None;
                return;
            };
            match app.templates_kind {
                TemplatesKind::Stacks => match create_template(app, name) {
                    Ok(()) => {
                        app.refresh_templates();
                        if let Some(idx) = app.templates.iter().position(|t| t.name == name) {
                            app.templates_selected = idx;
                        }
                        shell_set_main_view(app, ShellView::Templates);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                        shell_edit_selected_template(app);
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
                TemplatesKind::Networks => match create_net_template(app, name) {
                    Ok(()) => {
                        app.refresh_net_templates();
                        if let Some(idx) = app.net_templates.iter().position(|t| t.name == name) {
                            app.net_templates_selected = idx;
                        }
                        shell_set_main_view(app, ShellView::Templates);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                        shell_edit_selected_net_template(app);
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
            }
            return;
        }
        if sub == "deploy" {
            let name = if let Some(v) = it.next() {
                v.to_string()
            } else {
                match app.templates_kind {
                    TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                    TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
                }
                .unwrap_or_default()
            };
            if name.trim().is_empty() {
                app.set_warn("no template selected");
                return;
            }
            match app.templates_kind {
                TemplatesKind::Stacks => shell_deploy_template(app, &name, action_req_tx),
                TemplatesKind::Networks => shell_deploy_net_template(app, &name, force, action_req_tx),
            }
            return;
        }
        if sub == "rm" || sub == "del" || sub == "delete" {
            let name = if let Some(n) = it.next() {
                n.to_string()
            } else {
                match app.templates_kind {
                    TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                    TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
                }
                .unwrap_or_default()
            };
            if name.trim().is_empty() {
                app.set_warn("no template selected");
                return;
            }
            if !force {
                shell_begin_confirm(
                    app,
                    format!(
                        "{} rm {name}",
                        match app.templates_kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                    cmdline_full.clone(),
                );
                return;
            }
            match app.templates_kind {
                TemplatesKind::Stacks => match delete_template(app, &name) {
                    Ok(()) => {
                        app.refresh_templates();
                        app.set_info(format!("deleted template {name}"));
                        shell_set_main_view(app, ShellView::Templates);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
                TemplatesKind::Networks => match delete_net_template(app, &name) {
                    Ok(()) => {
                        app.refresh_net_templates();
                        app.set_info(format!("deleted network template {name}"));
                        shell_set_main_view(app, ShellView::Templates);
                        shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                    }
                    Err(e) => app.set_error(format!("{e:#}")),
                },
            }
            return;
        }
        app.set_warn("usage: :template add <name> | :template deploy[!] [name] | :template rm[!] [name] | :templates kind (stacks|networks|toggle)");
        return;
    }

    if matches!(cmd, "nettemplate" | "nettpl" | "ntpl" | "nt") {
        let sub = it.next().unwrap_or("");
        if sub == "edit" {
            app.templates_kind = TemplatesKind::Networks;
            shell_set_main_view(app, ShellView::Templates);
            shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
            shell_edit_selected_net_template(app);
            return;
        }
        if sub == "new" {
            app.shell_cmd_mode = true;
            set_text_and_cursor(
                &mut app.shell_cmd_input,
                &mut app.shell_cmd_cursor,
                "nettemplate add ".to_string(),
            );
            app.shell_confirm = None;
            return;
        }
        if sub == "add" || sub == "new" {
            let Some(name) = it.next() else {
                app.shell_cmd_mode = true;
                set_text_and_cursor(
                    &mut app.shell_cmd_input,
                    &mut app.shell_cmd_cursor,
                    "nettemplate add ".to_string(),
                );
                app.shell_confirm = None;
                return;
            };
            match create_net_template(app, name) {
                Ok(()) => {
                    app.refresh_net_templates();
                    if let Some(idx) = app.net_templates.iter().position(|t| t.name == name) {
                        app.net_templates_selected = idx;
                    }
                    app.templates_kind = TemplatesKind::Networks;
                    shell_set_main_view(app, ShellView::Templates);
                    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                    shell_edit_selected_net_template(app);
                }
                Err(e) => app.set_error(format!("{e:#}")),
            }
            return;
        }
        if sub == "deploy" {
            let name = if let Some(v) = it.next() {
                v.to_string()
            } else if let Some(t) = app.selected_net_template().map(|t| t.name.clone()) {
                t
            } else {
                app.set_warn("usage: :nettemplate deploy <name>");
                return;
            };
            shell_deploy_net_template(app, &name, force, action_req_tx);
            return;
        }
        if sub == "rm" || sub == "del" || sub == "delete" {
            let name = if let Some(n) = it.next() {
                n.to_string()
            } else if let Some(t) = app.selected_net_template().map(|t| t.name.clone()) {
                t
            } else {
                app.set_warn("usage: :nettemplate rm <name>");
                return;
            };
            if !force {
                shell_begin_confirm(app, format!("nettemplate rm {name}"), cmdline_full.clone());
                return;
            }
            match delete_net_template(app, &name) {
                Ok(()) => {
                    app.refresh_net_templates();
                    app.set_info(format!("deleted network template {name}"));
                    app.templates_kind = TemplatesKind::Networks;
                    shell_set_main_view(app, ShellView::Templates);
                    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Templates));
                }
                Err(e) => app.set_error(format!("{e:#}")),
            }
            return;
        }
        app.set_warn(
            "usage: :nettemplate add <name> | :nettemplate deploy <name> | :nettemplate rm <name>",
        );
        return;
    }

    if cmd == "server" {
        let sub = it.next().unwrap_or("");
        match sub {
            "list" => {
                if app.servers.is_empty() {
                    app.set_info("no servers configured");
                } else {
                    let lines: Vec<String> = app
                        .servers
                        .iter()
                        .map(|s| {
                            format!("server '{}' -> {} (cmd={})", s.name, s.target, s.docker_cmd)
                        })
                        .collect();
                    for line in lines {
                        app.set_info(line);
                    }
                }
                app.shell_msgs_return = app.shell_view;
                app.shell_view = ShellView::Messages;
                app.shell_focus = ShellFocus::List;
                app.shell_msgs_scroll = usize::MAX;
            }
            "use" => {
                let Some(name) = it.next() else {
                    app.set_warn("usage: :server use <name>");
                    return;
                };
                let Some(idx) = find_server_by_name(&app.servers, name) else {
                    app.set_warn(format!("unknown server: {name}"));
                    return;
                };
                shell_switch_server(app, idx, conn_tx, refresh_tx);
            }
            "rm" => {
                let Some(name) = it.next() else {
                    app.set_warn("usage: :server rm <name>");
                    return;
                };
                if !force {
                    shell_begin_confirm(app, format!("server rm {name}"), cmdline_full.clone());
                    return;
                }
                let Some(idx) = find_server_by_name(&app.servers, name) else {
                    app.set_warn(format!("unknown server: {name}"));
                    return;
                };
                let removed_active = app.active_server.as_deref() == Some(name);
                app.servers.remove(idx);
                app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
                if removed_active {
                    app.active_server = None;
                    app.server_selected = 0;
                    if !app.servers.is_empty() {
                        shell_switch_server(app, 0, conn_tx, refresh_tx);
                    } else {
                        app.persist_config();
                    }
                } else {
                    app.server_selected =
                        app.server_selected.min(app.servers.len().saturating_sub(1));
                    app.persist_config();
                }
            }
            "add" => {
                let Some(name) = it.next() else {
                    app.set_warn("usage: :server add <name> (ssh <target> | local) [opts]");
                    return;
                };
                let Some(name) = ensure_unique_server_name(&app.servers, name) else {
                    app.set_warn("server name already exists");
                    return;
                };
                let Some(kind) = it.next() else {
                    app.set_warn("usage: :server add <name> (ssh <target> | local) [opts]");
                    return;
                };
                let mut rest: Vec<String> = it.map(|s| s.to_string()).collect();
                let (port, identity, docker_cmd, tail) = parse_kv_args(rest.drain(..).into_iter());
                let docker_cmd = docker_cmd.unwrap_or_else(|| "docker".to_string());

                match kind {
                    "ssh" => {
                        let target = tail.get(0).cloned().unwrap_or_default();
                        if target.trim().is_empty() {
                            app.set_warn("usage: :server add <name> ssh <target> [opts]");
                            return;
                        }
                        app.servers.push(ServerEntry {
                            name,
                            target,
                            port,
                            identity,
                            docker_cmd,
                        });
                    }
                    "local" => {
                        app.servers.push(ServerEntry {
                            name,
                            target: "local".to_string(),
                            port: None,
                            identity: None,
                            docker_cmd,
                        });
                    }
                    _ => {
                        app.set_error(
                            "usage: :server add <name> (ssh <target> | local) [opts]".to_string(),
                        );
                        return;
                    }
                }
                app.shell_server_shortcuts = build_server_shortcuts(&app.servers);
                app.persist_config();
            }
            _ => {
                app.set_error("usage: :server (list|use|add|rm) ...".to_string());
            }
        }
        return;
    }

    app.set_error(format!("unknown command: {cmd}"));
    return;
}

fn shell_exec_container_action(
    app: &mut App,
    action: ContainerAction,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let ids: Vec<String> = if let Some(ids) = app.selected_stack_container_ids() {
        ids
    } else if !app.marked.is_empty() {
        app.marked.iter().cloned().collect()
    } else {
        app.selected_container()
            .map(|c| vec![c.id.clone()])
            .unwrap_or_default()
    };
    if ids.is_empty() {
        app.set_warn("no containers selected");
        return;
    }
    let now = Instant::now();
    for id in ids {
        app.action_inflight.insert(
            id.clone(),
            ActionMarker {
                action,
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::Container { action, id });
    }
}

fn shell_exec_image_action(
    app: &mut App,
    untag: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let keys: Vec<String> = if !app.marked_images.is_empty() {
        app.marked_images.iter().cloned().collect()
    } else {
        app.selected_image()
            .map(|img| vec![App::image_row_key(img)])
            .unwrap_or_default()
    };
    if keys.is_empty() {
        app.set_warn("no images selected");
        return;
    }

    let now = Instant::now();
    for key in keys {
        let (id, reference) = if let Some(ref_str) = key.strip_prefix("ref:") {
            let ref_str = ref_str.to_string();
            let id = app
                .images
                .iter()
                .find(|i| App::image_row_ref(i).as_deref() == Some(ref_str.as_str()))
                .map(|i| i.id.clone())
                .unwrap_or_default();
            (id, Some(ref_str))
        } else if let Some(id) = key.strip_prefix("id:") {
            (id.to_string(), None)
        } else {
            (key.clone(), None)
        };
        if id.is_empty() {
            app.set_error("failed to resolve image id");
            continue;
        }

        let marker_key = if untag {
            reference
                .as_ref()
                .map(|r| format!("ref:{}", r))
                .unwrap_or_else(|| format!("id:{}", id))
        } else {
            format!("id:{}", id)
        };
        app.image_action_inflight.insert(
            marker_key.clone(),
            SimpleMarker {
                until: now + Duration::from_secs(120),
            },
        );
        if untag {
            let Some(reference) = reference else {
                app.image_action_inflight.remove(&marker_key);
                app.set_warn("cannot untag by ID; select a repo:tag row or use Remove");
                continue;
            };
            let _ = action_req_tx.send(ActionRequest::ImageUntag {
                marker_key,
                reference,
            });
        } else {
            let _ = action_req_tx.send(ActionRequest::ImageForceRemove { marker_key, id });
        }
    }
}

fn shell_exec_volume_remove(app: &mut App, action_req_tx: &mpsc::UnboundedSender<ActionRequest>) {
    let names: Vec<String> = if !app.marked_volumes.is_empty() {
        app.marked_volumes.iter().cloned().collect()
    } else {
        app.selected_volume()
            .map(|v| vec![v.name.clone()])
            .unwrap_or_default()
    };
    if names.is_empty() {
        app.set_warn("no volumes selected");
        return;
    }
    let now = Instant::now();
    for name in names {
        app.volume_action_inflight.insert(
            name.clone(),
            SimpleMarker {
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::VolumeRemove { name });
    }
}

fn shell_exec_network_remove(app: &mut App, action_req_tx: &mpsc::UnboundedSender<ActionRequest>) {
    let ids: Vec<String> = if !app.marked_networks.is_empty() {
        app.marked_networks.iter().cloned().collect()
    } else {
        app.selected_network()
            .map(|n| vec![n.id.clone()])
            .unwrap_or_default()
    };
    let ids: Vec<String> = ids
        .into_iter()
        .filter(|id| !app.is_system_network_id(id))
        .collect();
    if ids.is_empty() {
        app.set_warn("no networks selected (system networks cannot be modified)");
        return;
    }
    let now = Instant::now();
    for id in ids {
        app.network_action_inflight.insert(
            id.clone(),
            SimpleMarker {
                until: now + Duration::from_secs(120),
            },
        );
        let _ = action_req_tx.send(ActionRequest::NetworkRemove { id });
    }
}

fn shell_execute_action(
    app: &mut App,
    a: ShellAction,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    match a {
        ShellAction::Start => {
            shell_exec_container_action(app, ContainerAction::Start, action_req_tx)
        }
        ShellAction::Stop => shell_exec_container_action(app, ContainerAction::Stop, action_req_tx),
        ShellAction::Restart => {
            shell_exec_container_action(app, ContainerAction::Restart, action_req_tx)
        }
        ShellAction::Delete => {
            shell_begin_confirm(app, "container rm", "container rm");
        }
        ShellAction::Console => {
            shell_open_console(app, Some("root"), "bash");
        }
        ShellAction::ImageUntag => shell_begin_confirm(app, "image untag", "image untag"),
        ShellAction::ImageForceRemove => shell_begin_confirm(app, "image rm", "image rm"),
        ShellAction::VolumeRemove => shell_begin_confirm(app, "volume rm", "volume rm"),
        ShellAction::NetworkRemove => {
            let any_removable = if !app.marked_networks.is_empty() {
                app.marked_networks
                    .iter()
                    .any(|id| !app.is_system_network_id(id))
            } else {
                app.selected_network()
                    .map(|n| !App::is_system_network(n))
                    .unwrap_or(false)
            };
            if !any_removable {
                app.set_warn("system networks cannot be modified");
                return;
            }
            shell_begin_confirm(app, "network rm", "network rm");
        }
        ShellAction::TemplateEdit => shell_edit_selected_template(app),
        ShellAction::TemplateNew => {
            app.shell_cmd_mode = true;
            set_text_and_cursor(
                &mut app.shell_cmd_input,
                &mut app.shell_cmd_cursor,
                match app.templates_kind {
                    TemplatesKind::Stacks => "template add ".to_string(),
                    TemplatesKind::Networks => "nettemplate add ".to_string(),
                },
            );
            app.shell_confirm = None;
        }
        ShellAction::TemplateDelete => {
            let name = match app.templates_kind {
                TemplatesKind::Stacks => app.selected_template().map(|t| t.name.clone()),
                TemplatesKind::Networks => app.selected_net_template().map(|t| t.name.clone()),
            };
            if let Some(name) = name {
                shell_begin_confirm(
                    app,
                    format!(
                        "{} rm {name}",
                        match app.templates_kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                    format!(
                        "{} rm {name}",
                        match app.templates_kind {
                            TemplatesKind::Stacks => "template",
                            TemplatesKind::Networks => "nettemplate",
                        }
                    ),
                );
            } else {
                app.set_warn("no template selected");
            }
        }
        ShellAction::TemplateDeploy => {
            match app.templates_kind {
                TemplatesKind::Stacks => {
                    if let Some(name) = app.selected_template().map(|t| t.name.clone()) {
                        shell_deploy_template(app, &name, action_req_tx);
                    } else {
                        app.set_warn("no template selected");
                    }
                }
                TemplatesKind::Networks => {
                    if let Some(name) = app.selected_net_template().map(|t| t.name.clone()) {
                        shell_deploy_net_template(app, &name, false, action_req_tx);
                    } else {
                        app.set_warn("no template selected");
                    }
                }
            }
        }
    }
}

fn shell_deploy_template(
    app: &mut App,
    name: &str,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if app.template_deploy_inflight.contains_key(name) {
        app.set_warn(format!("template '{name}' is already deploying"));
        return;
    }
    let Some(tpl) = app.templates.iter().find(|t| t.name == name).cloned() else {
        app.set_warn(format!("unknown template: {name}"));
        return;
    };
    if !tpl.has_compose {
        app.set_warn("template has no compose.yaml");
        return;
    }
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let _ = action_req_tx.send(ActionRequest::TemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_compose: tpl.compose_path.clone(),
    });
    app.template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.set_info(format!("deploying template {name}"));
}

fn shell_deploy_net_template(
    app: &mut App,
    name: &str,
    force: bool,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    if app.net_template_deploy_inflight.contains_key(name) {
        app.set_warn(format!("network template '{name}' is already deploying"));
        return;
    }
    let Some(tpl) = app.net_templates.iter().find(|t| t.name == name).cloned() else {
        app.set_warn(format!("unknown network template: {name}"));
        return;
    };
    if !tpl.has_cfg {
        app.set_warn("template has no network.json");
        return;
    }
    if app.active_server.is_none() {
        app.set_warn("no active server selected");
        return;
    }
    let runner = current_runner_from_app(app);
    let docker = DockerCfg {
        docker_cmd: current_docker_cmd_from_app(app),
    };
    let _ = action_req_tx.send(ActionRequest::NetTemplateDeploy {
        name: tpl.name.clone(),
        runner,
        docker,
        local_cfg: tpl.cfg_path.clone(),
        force,
    });
    app.net_template_deploy_inflight.insert(
        tpl.name.clone(),
        DeployMarker {
            started: Instant::now(),
        },
    );
    app.set_info(format!("deploying network template {name}"));
}

fn shell_edit_selected_template(app: &mut App) {
    match app.templates_kind {
        TemplatesKind::Stacks => {
            let Some((name, has_compose, compose_path, dir)) = app.selected_template().map(|t| {
                (
                    t.name.clone(),
                    t.has_compose,
                    t.compose_path.clone(),
                    t.dir.clone(),
                )
            }) else {
                app.set_warn("no template selected");
                return;
            };
            app.templates_refresh_after_edit = Some(name);
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            let target = if has_compose { compose_path } else { dir };
            let cmd = format!(
                "{} {}",
                editor,
                shell_escape_sh_arg(&target.to_string_lossy())
            );
            app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
        }
        TemplatesKind::Networks => shell_edit_selected_net_template(app),
    }
}

fn shell_edit_selected_net_template(app: &mut App) {
    let Some((name, has_cfg, cfg_path, dir)) = app.selected_net_template().map(|t| {
        (
            t.name.clone(),
            t.has_cfg,
            t.cfg_path.clone(),
            t.dir.clone(),
        )
    }) else {
        app.set_warn("no network template selected");
        return;
    };
    app.net_templates_refresh_after_edit = Some(name);
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let target = if has_cfg { cfg_path } else { dir };
    let cmd = format!(
        "{} {}",
        editor,
        shell_escape_sh_arg(&target.to_string_lossy())
    );
    app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
}

fn handle_shell_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    refresh_interval_tx: &watch::Sender<Duration>,
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
                    shell_execute_cmdline(
                        app,
                        &cmd,
                        conn_tx,
                        refresh_tx,
                        refresh_interval_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
            }
        }
    }

    if app.shell_cmd_mode {
        if let Some(confirm) = app.shell_confirm.clone() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Re-run the original command with the force modifier to auto-confirm.
                    let cmdline = format!("!{}", confirm.cmdline);
                    app.shell_confirm = None;
                    app.shell_cmd_mode = false;
                    app.shell_cmd_input.clear();
                    app.shell_cmd_cursor = 0;
                    app.shell_cmd_history.reset_nav();
                    shell_execute_cmdline(
                        app,
                        &cmdline,
                        conn_tx,
                        refresh_tx,
                        refresh_interval_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // Cancel.
                    app.shell_confirm = None;
                    app.shell_cmd_mode = false;
                    app.shell_cmd_input.clear();
                    app.shell_cmd_cursor = 0;
                    app.shell_cmd_history.reset_nav();
                    return;
                }
                _ => return,
            }
        }

        match key.code {
            KeyCode::Enter => {
                let cmdline = app.shell_cmd_input.trim().to_string();
                app.shell_cmd_mode = false;
                app.shell_cmd_input.clear();
                app.shell_cmd_cursor = 0;
                app.push_cmd_history(&cmdline);
                shell_execute_cmdline(
                    app,
                    &cmdline,
                    conn_tx,
                    refresh_tx,
                    refresh_interval_tx,
                    logs_req_tx,
                    action_req_tx,
                );
            }
            KeyCode::Esc => {
                app.shell_cmd_mode = false;
                app.shell_cmd_input.clear();
                app.shell_cmd_cursor = 0;
                app.shell_confirm = None;
                app.shell_cmd_history.reset_nav();
            }
            KeyCode::Up => {
                if let Some(s) = app.shell_cmd_history.prev(&app.shell_cmd_input) {
                    set_text_and_cursor(&mut app.shell_cmd_input, &mut app.shell_cmd_cursor, s);
                }
            }
            KeyCode::Down => {
                if let Some(s) = app.shell_cmd_history.next() {
                    set_text_and_cursor(&mut app.shell_cmd_input, &mut app.shell_cmd_cursor, s);
                }
            }
            KeyCode::Backspace => {
                backspace_at_cursor(&mut app.shell_cmd_input, &mut app.shell_cmd_cursor);
                app.shell_cmd_history.on_edit();
            }
            KeyCode::Delete => {
                delete_at_cursor(&mut app.shell_cmd_input, &mut app.shell_cmd_cursor);
                app.shell_cmd_history.on_edit();
            }
            KeyCode::Left => {
                app.shell_cmd_cursor = clamp_cursor_to_text(&app.shell_cmd_input, app.shell_cmd_cursor)
                    .saturating_sub(1);
            }
            KeyCode::Right => {
                let len = app.shell_cmd_input.chars().count();
                app.shell_cmd_cursor =
                    clamp_cursor_to_text(&app.shell_cmd_input, app.shell_cmd_cursor).saturating_add(1).min(len);
            }
            KeyCode::Home => app.shell_cmd_cursor = 0,
            KeyCode::End => app.shell_cmd_cursor = app.shell_cmd_input.chars().count(),
            KeyCode::Char(ch) => {
                // Common readline-like movement shortcuts.
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match ch {
                        'a' | 'A' => app.shell_cmd_cursor = 0,
                        'e' | 'E' => app.shell_cmd_cursor = app.shell_cmd_input.chars().count(),
                        'u' | 'U' => {
                            app.shell_cmd_input.clear();
                            app.shell_cmd_cursor = 0;
                            app.shell_cmd_history.on_edit();
                        }
                        _ => {}
                    }
                } else if !ch.is_control() {
                    insert_char_at_cursor(&mut app.shell_cmd_input, &mut app.shell_cmd_cursor, ch);
                    app.shell_cmd_history.on_edit();
                }
            }
            _ => {}
        }
        return;
    }

    // Input modes first (vim-like): when editing, do not treat keys as global shortcuts.
    if app.shell_view == ShellView::Logs {
        match app.logs_mode {
            LogsMode::Search => match key.code {
                KeyCode::Enter => app.logs_commit_search(),
                KeyCode::Esc => app.logs_cancel_search(),
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.logs_input, &mut app.logs_input_cursor);
                    app.logs_rebuild_matches();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.logs_input, &mut app.logs_input_cursor);
                    app.logs_rebuild_matches();
                }
                KeyCode::Left => {
                    app.logs_input_cursor =
                        clamp_cursor_to_text(&app.logs_input, app.logs_input_cursor).saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.logs_input.chars().count();
                    app.logs_input_cursor = clamp_cursor_to_text(&app.logs_input, app.logs_input_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.logs_input_cursor = 0,
                KeyCode::End => app.logs_input_cursor = app.logs_input.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        insert_char_at_cursor(&mut app.logs_input, &mut app.logs_input_cursor, ch);
                        app.logs_rebuild_matches();
                    }
                }
                _ => {}
            },
            LogsMode::Command => match key.code {
                KeyCode::Enter => {
                    // Minimal command mode for now.
                    let cmdline = app.logs_command.trim().to_string();
                    app.push_cmd_history(&cmdline);
                    if let Some(path) = cmdline.strip_prefix("save").map(str::trim) {
                        if path.is_empty() {
                            app.set_warn("usage: save <file>");
                        } else {
                            match app.logs_text.as_deref() {
                                None => app.set_warn("no logs loaded"),
                                Some(text) => match write_text_file(path, text) {
                                    Ok(p) => app.set_info(format!("saved logs to {}", p.display())),
                                    Err(e) => app.set_error(format!("save failed: {e:#}")),
                                },
                            }
                        }
                        app.logs_mode = LogsMode::Normal;
                        app.logs_command.clear();
                        app.logs_command_cursor = 0;
                        app.logs_rebuild_matches();
                        return;
                    }
                    let mut parts = cmdline.split_whitespace();
                    let cmd = parts.next().unwrap_or("");
                    match cmd {
                        "" => {}
                        "q" | "quit" => shell_back_from_full(app),
                        "j" => {
                            let Some(n) = parts.next() else {
                                app.set_warn("usage: j <line>");
                                // keep mode change below
                                app.logs_mode = LogsMode::Normal;
                                app.logs_command.clear();
                                app.logs_command_cursor = 0;
                                app.logs_rebuild_matches();
                                return;
                            };
                            match n.parse::<usize>() {
                                Ok(n) if n > 0 => {
                                    let total = app.logs_total_lines();
                                    app.logs_cursor =
                                        n.saturating_sub(1).min(total.saturating_sub(1));
                                }
                                _ => app.set_warn("usage: j <line>"),
                            }
                        }
                        "set" => match parts.next().unwrap_or("") {
                            "number" => app.logs_show_line_numbers = true,
                            "nonumber" => app.logs_show_line_numbers = false,
                            "logtail" => {
                                let Some(v) = parts.next() else {
                                    app.set_warn("usage: set logtail <lines>");
                                    app.logs_mode = LogsMode::Normal;
                                    app.logs_command.clear();
                                    app.logs_command_cursor = 0;
                                    app.logs_rebuild_matches();
                                    return;
                                };
                                match v.parse::<usize>() {
                                    Ok(n) if (1..=200_000).contains(&n) => {
                                        app.logs_tail = n;
                                        app.persist_config();
                                        if let Some(id) = app.logs_for_id.clone() {
                                            app.logs_loading = true;
                                            let _ = logs_req_tx.send((id, app.logs_tail.max(1)));
                                        }
                                    }
                                    _ => app.set_warn("logtail must be 1..200000"),
                                }
                            }
                            "regex" => {
                                app.logs_use_regex = true;
                                app.logs_rebuild_matches();
                            }
                            "noregex" => {
                                app.logs_use_regex = false;
                                app.logs_rebuild_matches();
                            }
                            x => app.set_warn(format!("unknown option: {x}")),
                        },
                        _ => app.set_warn(format!("unknown command: {cmdline}")),
                    }
                    app.logs_mode = LogsMode::Normal;
                    app.logs_command.clear();
                    app.logs_command_cursor = 0;
                    app.logs_rebuild_matches();
                }
                KeyCode::Esc => {
                    app.logs_mode = LogsMode::Normal;
                    app.logs_command.clear();
                    app.logs_command_cursor = 0;
                    app.logs_rebuild_matches();
                    app.logs_cmd_history.reset_nav();
                }
                KeyCode::Up => {
                    if let Some(s) = app.logs_cmd_history.prev(&app.logs_command) {
                        set_text_and_cursor(&mut app.logs_command, &mut app.logs_command_cursor, s);
                    }
                }
                KeyCode::Down => {
                    if let Some(s) = app.logs_cmd_history.next() {
                        set_text_and_cursor(&mut app.logs_command, &mut app.logs_command_cursor, s);
                    }
                }
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.logs_command, &mut app.logs_command_cursor);
                    app.logs_cmd_history.on_edit();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.logs_command, &mut app.logs_command_cursor);
                    app.logs_cmd_history.on_edit();
                }
                KeyCode::Left => {
                    app.logs_command_cursor =
                        clamp_cursor_to_text(&app.logs_command, app.logs_command_cursor).saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.logs_command.chars().count();
                    app.logs_command_cursor = clamp_cursor_to_text(&app.logs_command, app.logs_command_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.logs_command_cursor = 0,
                KeyCode::End => app.logs_command_cursor = app.logs_command.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() {
                        insert_char_at_cursor(
                            &mut app.logs_command,
                            &mut app.logs_command_cursor,
                            ch,
                        );
                        app.logs_cmd_history.on_edit();
                    }
                }
                _ => {}
            },
            LogsMode::Normal => {}
        }
        if app.logs_mode != LogsMode::Normal {
            return;
        }
    }

    if app.shell_view == ShellView::Inspect {
        match app.inspect_mode {
            InspectMode::Search => match key.code {
                KeyCode::Enter => app.inspect_commit_search(),
                KeyCode::Esc => app.inspect_exit_input(),
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.inspect_input, &mut app.inspect_input_cursor);
                    app.rebuild_inspect_lines();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.inspect_input, &mut app.inspect_input_cursor);
                    app.rebuild_inspect_lines();
                }
                KeyCode::Left => {
                    app.inspect_input_cursor =
                        clamp_cursor_to_text(&app.inspect_input, app.inspect_input_cursor).saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.inspect_input.chars().count();
                    app.inspect_input_cursor = clamp_cursor_to_text(&app.inspect_input, app.inspect_input_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.inspect_input_cursor = 0,
                KeyCode::End => app.inspect_input_cursor = app.inspect_input.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        insert_char_at_cursor(
                            &mut app.inspect_input,
                            &mut app.inspect_input_cursor,
                            ch,
                        );
                        app.rebuild_inspect_lines();
                    }
                }
                _ => {}
            },
            InspectMode::Command => match key.code {
                KeyCode::Enter => {
                    let cmd = app.inspect_input.trim().to_string();
                    app.push_cmd_history(&cmd);
                    if let Some(path) = cmd.strip_prefix("save").map(str::trim) {
                        if path.is_empty() {
                            app.inspect_error = Some("usage: save <file>".to_string());
                        } else {
                            match app.inspect_value.as_ref() {
                                None => {
                                    app.inspect_error = Some("no inspect data loaded".to_string())
                                }
                                Some(v) => match serde_json::to_string_pretty(v) {
                                    Ok(s) => match write_text_file(path, &s) {
                                        Ok(p) => app
                                            .set_info(format!("saved inspect to {}", p.display())),
                                        Err(e) => {
                                            app.inspect_error = Some(format!("save failed: {e:#}"))
                                        }
                                    },
                                    Err(e) => {
                                        app.inspect_error =
                                            Some(format!("failed to serialize inspect: {e:#}"))
                                    }
                                },
                            }
                        }
                        app.inspect_mode = InspectMode::Normal;
                        app.inspect_input.clear();
                        app.inspect_input_cursor = 0;
                        app.rebuild_inspect_lines();
                        return;
                    }
                    match cmd.as_str() {
                        "" => {}
                        "q" | "quit" => shell_back_from_full(app),
                        "e" | "expand" | "expandall" => app.inspect_expand_all(),
                        "c" | "collapse" | "collapseall" => app.inspect_collapse_all(),
                        "y" => app.inspect_copy_selected_value(true),
                        "p" => app.inspect_copy_selected_path(),
                        _ => app.inspect_error = Some(format!("unknown command: {cmd}")),
                    }
                    app.inspect_mode = InspectMode::Normal;
                    app.inspect_input.clear();
                    app.inspect_input_cursor = 0;
                    app.rebuild_inspect_lines();
                }
                KeyCode::Esc => {
                    app.inspect_mode = InspectMode::Normal;
                    app.inspect_input.clear();
                    app.inspect_input_cursor = 0;
                    app.rebuild_inspect_lines();
                    app.inspect_cmd_history.reset_nav();
                }
                KeyCode::Up => {
                    if let Some(s) = app.inspect_cmd_history.prev(&app.inspect_input) {
                        set_text_and_cursor(
                            &mut app.inspect_input,
                            &mut app.inspect_input_cursor,
                            s,
                        );
                    }
                }
                KeyCode::Down => {
                    if let Some(s) = app.inspect_cmd_history.next() {
                        set_text_and_cursor(
                            &mut app.inspect_input,
                            &mut app.inspect_input_cursor,
                            s,
                        );
                    }
                }
                KeyCode::Backspace => {
                    backspace_at_cursor(&mut app.inspect_input, &mut app.inspect_input_cursor);
                    app.inspect_cmd_history.on_edit();
                }
                KeyCode::Delete => {
                    delete_at_cursor(&mut app.inspect_input, &mut app.inspect_input_cursor);
                    app.inspect_cmd_history.on_edit();
                }
                KeyCode::Left => {
                    app.inspect_input_cursor = clamp_cursor_to_text(&app.inspect_input, app.inspect_input_cursor)
                        .saturating_sub(1);
                }
                KeyCode::Right => {
                    let len = app.inspect_input.chars().count();
                    app.inspect_input_cursor = clamp_cursor_to_text(&app.inspect_input, app.inspect_input_cursor)
                        .saturating_add(1)
                        .min(len);
                }
                KeyCode::Home => app.inspect_input_cursor = 0,
                KeyCode::End => app.inspect_input_cursor = app.inspect_input.chars().count(),
                KeyCode::Char(ch) => {
                    if !ch.is_control() {
                        insert_char_at_cursor(
                            &mut app.inspect_input,
                            &mut app.inspect_input_cursor,
                            ch,
                        );
                        app.inspect_cmd_history.on_edit();
                    }
                }
                _ => {}
            },
            InspectMode::Normal => {}
        }
        if app.inspect_mode != InspectMode::Normal {
            return;
        }
    }

    // Custom key bindings (outside of input modes).
    if let Some(spec) = key_spec_from_event(key) {
        if let Some(hit) = lookup_scoped_binding(app, spec) {
            match hit {
                BindingHit::Disabled => return,
                BindingHit::Cmd(cmd) => {
                    shell_execute_cmdline(
                        app,
                        &cmd,
                        conn_tx,
                        refresh_tx,
                        refresh_interval_tx,
                        logs_req_tx,
                        action_req_tx,
                    );
                    return;
                }
            }
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
                    app.logs_mode = LogsMode::Command;
                    app.logs_command.clear();
                    app.logs_command_cursor = 0;
                    app.logs_rebuild_matches();
                }
                ShellView::Inspect => app.inspect_enter_command(),
                _ => {
                    app.shell_cmd_mode = true;
                    app.shell_cmd_input.clear();
                    app.shell_cmd_cursor = 0;
                    app.shell_confirm = None;
                }
            }
            return;
        }
        KeyCode::Char('q') if key.modifiers.is_empty() => {
            shell_back_from_full(app);
            return;
        }
        _ => {}
    }

    // Direct shortcuts (servers/modules/actions).
    if key.modifiers.is_empty() {
        if let KeyCode::Char(mut ch) = key.code {
            // Servers: 1..9 and assigned letters.
            for (i, hint) in app.shell_server_shortcuts.iter().copied().enumerate() {
                if hint == '\0' {
                    continue;
                }
                if hint.is_ascii_alphabetic() {
                    ch = ch.to_ascii_uppercase();
                }
                if ch == hint {
                    shell_switch_server(app, i, conn_tx, refresh_tx);
                    return;
                }
            }
            // Modules (disabled in full-screen views like Logs/Inspect to avoid conflicts with
            // in-view navigation keys like n/N, j/k, etc.).
            if !matches!(app.shell_view, ShellView::Logs | ShellView::Inspect) {
                let ch_lc = ch.to_ascii_lowercase();
                for v in [
                    ShellView::Containers,
                    ShellView::Images,
                    ShellView::Volumes,
                    ShellView::Networks,
                    ShellView::Templates,
                    ShellView::Inspect,
                    ShellView::Logs,
                ] {
                    if ch_lc == shell_module_shortcut(v) {
                        match v {
                            ShellView::Inspect => shell_enter_inspect(app, inspect_req_tx),
                            ShellView::Logs => shell_enter_logs(app, logs_req_tx),
                            _ => {
                                shell_set_main_view(app, v);
                                shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                            }
                        }
                        return;
                    }
                }
            }
        }
    }

    // Focus-specific navigation / activation.
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
                    ShellSidebarItem::Server(i) => shell_switch_server(app, i, conn_tx, refresh_tx),
                    ShellSidebarItem::Module(v) => match v {
                        ShellView::Inspect => shell_enter_inspect(app, inspect_req_tx),
                        ShellView::Logs => shell_enter_logs(app, logs_req_tx),
                        _ => {
                            shell_set_main_view(app, v);
                            shell_sidebar_select_item(app, ShellSidebarItem::Module(v));
                        }
                    },
                    ShellSidebarItem::Action(a) => shell_execute_action(app, a, action_req_tx),
                    ShellSidebarItem::Separator => {}
                    ShellSidebarItem::Gap => {}
                }
            }
            _ => {}
        }
        return;
    }

    // Main list / view handling.
    match app.shell_view {
        ShellView::Containers | ShellView::Images | ShellView::Volumes | ShellView::Networks => {
            // Ensure active_view matches (used by the existing selection/mark logic).
            app.active_view = match app.shell_view {
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
                    ActiveView::Containers => app.selected = 0,
                    ActiveView::Images => app.images_selected = 0,
                    ActiveView::Volumes => app.volumes_selected = 0,
                    ActiveView::Networks => app.networks_selected = 0,
                },
                KeyCode::End => match app.active_view {
                    ActiveView::Containers => {
                        let max = app.view_len().saturating_sub(1);
                        app.selected = max;
                    }
                    ActiveView::Images => {
                        app.images_selected = app.images_visible_len().saturating_sub(1);
                    }
                    ActiveView::Volumes => {
                        app.volumes_selected = app.volumes_visible_len().saturating_sub(1);
                    }
                    ActiveView::Networks => {
                        app.networks_selected = app.networks.len().saturating_sub(1)
                    }
                },
                KeyCode::Char(' ') => {
                    if app.active_view == ActiveView::Containers
                        && app.list_mode == ListMode::Tree
                        && app.toggle_tree_expanded_selected()
                    {
                        // Stack header toggle.
                    } else {
                        app.toggle_mark_selected();
                    }
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.mark_all();
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.clear_marks();
                }
                KeyCode::Enter => {
                    if app.active_view == ActiveView::Containers
                        && app.list_mode == ListMode::Tree
                        && app.toggle_tree_expanded_selected()
                    {
                        // Stack header toggle.
                    }
                }
                _ => {}
            }
        }
        ShellView::Templates => {
            if app.shell_focus == ShellFocus::Details {
                match app.templates_kind {
                    TemplatesKind::Stacks => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.templates_details_scroll =
                                app.templates_details_scroll.saturating_sub(1)
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.templates_details_scroll += 1,
                        KeyCode::PageUp => {
                            app.templates_details_scroll =
                                app.templates_details_scroll.saturating_sub(10)
                        }
                        KeyCode::PageDown => app.templates_details_scroll += 10,
                        KeyCode::Home => app.templates_details_scroll = 0,
                        KeyCode::End => app.templates_details_scroll = usize::MAX,
                        _ => {}
                    },
                    TemplatesKind::Networks => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.net_templates_details_scroll =
                                app.net_templates_details_scroll.saturating_sub(1)
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.net_templates_details_scroll += 1,
                        KeyCode::PageUp => {
                            app.net_templates_details_scroll =
                                app.net_templates_details_scroll.saturating_sub(10)
                        }
                        KeyCode::PageDown => app.net_templates_details_scroll += 10,
                        KeyCode::Home => app.net_templates_details_scroll = 0,
                        KeyCode::End => app.net_templates_details_scroll = usize::MAX,
                        _ => {}
                    },
                }
            } else {
                match app.templates_kind {
                    TemplatesKind::Stacks => {
                        let before = app.templates_selected;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.templates_selected = app.templates_selected.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !app.templates.is_empty() {
                                    app.templates_selected =
                                        (app.templates_selected + 1).min(app.templates.len() - 1);
                                } else {
                                    app.templates_selected = 0;
                                }
                            }
                            KeyCode::PageUp => {
                                app.templates_selected = app.templates_selected.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                if !app.templates.is_empty() {
                                    app.templates_selected =
                                        (app.templates_selected + 10).min(app.templates.len() - 1);
                                } else {
                                    app.templates_selected = 0;
                                }
                            }
                            KeyCode::Home => app.templates_selected = 0,
                            KeyCode::End => {
                                app.templates_selected = app.templates.len().saturating_sub(1)
                            }
                            _ => {}
                        }
                        if app.templates_selected != before {
                            app.templates_details_scroll = 0;
                        }
                    }
                    TemplatesKind::Networks => {
                        let before = app.net_templates_selected;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.net_templates_selected =
                                    app.net_templates_selected.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if !app.net_templates.is_empty() {
                                    app.net_templates_selected = (app.net_templates_selected + 1)
                                        .min(app.net_templates.len() - 1);
                                } else {
                                    app.net_templates_selected = 0;
                                }
                            }
                            KeyCode::PageUp => {
                                app.net_templates_selected =
                                    app.net_templates_selected.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                if !app.net_templates.is_empty() {
                                    app.net_templates_selected = (app.net_templates_selected + 10)
                                        .min(app.net_templates.len() - 1);
                                } else {
                                    app.net_templates_selected = 0;
                                }
                            }
                            KeyCode::Home => app.net_templates_selected = 0,
                            KeyCode::End => {
                                app.net_templates_selected =
                                    app.net_templates.len().saturating_sub(1)
                            }
                            _ => {}
                        }
                        if app.net_templates_selected != before {
                            app.net_templates_details_scroll = 0;
                        }
                    }
                }
            }
        }
        ShellView::Logs => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.logs_move_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.logs_move_down(1),
            KeyCode::PageUp => app.logs_move_up(10),
            KeyCode::PageDown => app.logs_move_down(10),
            KeyCode::Left => app.logs_hscroll = app.logs_hscroll.saturating_sub(4),
            KeyCode::Right => app.logs_hscroll = app.logs_hscroll.saturating_add(4),
            KeyCode::Home => app.logs_cursor = 0,
            KeyCode::End => app.logs_cursor = app.logs_total_lines().saturating_sub(1),
            KeyCode::Esc => {
                if app.logs_select_anchor.is_some() {
                    app.logs_clear_selection();
                }
            }
            KeyCode::Char(' ') => app.logs_toggle_selection(),
            KeyCode::Char('m') => {
                app.logs_use_regex = !app.logs_use_regex;
                app.logs_rebuild_matches();
            }
            KeyCode::Char('l') => app.logs_show_line_numbers = !app.logs_show_line_numbers,
            KeyCode::Char('/') => {
                app.logs_mode = LogsMode::Search;
                app.logs_input = app.logs_query.clone();
                app.logs_input_cursor = app.logs_input.chars().count();
                app.logs_rebuild_matches();
            }
            KeyCode::Char(':') => {
                app.logs_mode = LogsMode::Command;
                app.logs_command.clear();
                app.logs_command_cursor = 0;
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
            KeyCode::Left => app.inspect_scroll = app.inspect_scroll.saturating_sub(4),
            KeyCode::Right => app.inspect_scroll = app.inspect_scroll.saturating_add(4),
            KeyCode::Home => {
                app.inspect_selected = 0;
                app.inspect_scroll = 0;
            }
            KeyCode::End => {
                if !app.inspect_lines.is_empty() {
                    app.inspect_selected = app.inspect_lines.len() - 1;
                } else {
                    app.inspect_selected = 0;
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
            KeyCode::Up | KeyCode::Char('k') => {
                app.shell_help_scroll = app.shell_help_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.shell_help_scroll = app.shell_help_scroll.saturating_add(1)
            }
            KeyCode::PageUp => app.shell_help_scroll = app.shell_help_scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_help_scroll = app.shell_help_scroll.saturating_add(10),
            KeyCode::Home => app.shell_help_scroll = 0,
            KeyCode::End => app.shell_help_scroll = usize::MAX,
            _ => {}
        },
        ShellView::Messages => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.shell_msgs_scroll = app.shell_msgs_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.shell_msgs_scroll = app.shell_msgs_scroll.saturating_add(1)
            }
            KeyCode::PageUp => app.shell_msgs_scroll = app.shell_msgs_scroll.saturating_sub(10),
            KeyCode::PageDown => app.shell_msgs_scroll = app.shell_msgs_scroll.saturating_add(10),
            KeyCode::Left => app.shell_msgs_hscroll = app.shell_msgs_hscroll.saturating_sub(4),
            KeyCode::Right => app.shell_msgs_hscroll = app.shell_msgs_hscroll.saturating_add(4),
            KeyCode::Home => app.shell_msgs_scroll = 0,
            KeyCode::End => app.shell_msgs_scroll = usize::MAX,
            _ => {}
        },
    }
}

fn draw_shell_header(
    f: &mut ratatui::Frame,
    app: &App,
    _refresh: Duration,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.header.to_style();
    f.render_widget(Block::default().style(bg), area);

    let server = current_server_label(app);
    let crumb = shell_breadcrumbs(app);
    let conn = if app.conn_error.is_some() {
        "○"
    } else {
        "●"
    };
    let conn_style = if app.conn_error.is_some() {
        app.theme
            .text_error
            .to_style()
            .bg(theme::parse_color(&app.theme.header.bg))
    } else {
        app.theme
            .text_ok
            .to_style()
            .bg(theme::parse_color(&app.theme.header.bg))
    };

    let left = " containr  ";
    let deploy = if let Some((name, marker)) = app.template_deploy_inflight.iter().next() {
        let secs = marker.started.elapsed().as_secs();
        let spin = spinner_char(marker.started, app.ascii_only);
        format!("  Deploy: {name} {spin} {secs}s")
    } else {
        String::new()
    };
    let mid = format!(
        "Server: {server}  {conn} connected  ⟳ {}s  View: {}{crumb}{deploy}",
        app.refresh_secs.max(1),
        app.shell_view.title(),
    );
    let right = "";

    let w = area.width.max(1) as usize;
    let mut line = String::new();
    line.push_str(left);
    line.push_str(&mid);
    let min_right = right.chars().count();
    let shown = truncate_end(&line, w.saturating_sub(min_right));
    let rem = w.saturating_sub(shown.chars().count());
    let right_shown = truncate_start(right, rem);

    let mut spans: Vec<Span> = Vec::new();
    // Bolden breadcrumb for better scanability.
    if !crumb.is_empty() && shown.contains(&crumb) {
        let mut parts = shown.splitn(2, &crumb);
        let before = parts.next().unwrap_or_default();
        let after = parts.next().unwrap_or_default();
        spans.push(Span::styled(before.to_string(), bg));
        spans.push(Span::styled(crumb.clone(), bg.add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(after.to_string(), bg));
    } else {
        spans.push(Span::styled(shown, bg));
    }
    // Color the connection dot to reflect current status.
    if spans.len() == 1 && spans[0].content.contains(conn) {
        // Not expected with current layout, but keep safe.
    }
    if spans
        .iter()
        .map(|s| s.content.clone())
        .collect::<String>()
        .contains(conn)
    {
        // If the conn symbol is inside existing spans, split the last span that contains it.
        let mut updated: Vec<Span> = Vec::new();
        for s in spans.into_iter() {
            if s.content.contains(conn) {
                let parts: Vec<&str> = s.content.split(conn).collect();
                if parts.len() == 2 {
                    updated.push(Span::styled(parts[0].to_string(), s.style));
                    updated.push(Span::styled(conn.to_string(), conn_style));
                    updated.push(Span::styled(parts[1].to_string(), s.style));
                } else {
                    updated.push(s);
                }
            } else {
                updated.push(s);
            }
        }
        spans = updated;
    }
    if !right_shown.is_empty() {
        spans.push(Span::styled(right_shown, bg.fg(Color::Gray)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn shell_breadcrumbs(app: &App) -> String {
    match app.shell_view {
        ShellView::Containers => {
            if let Some((name, ..)) = app.selected_stack() {
                return format!("/{name}");
            }
            if let Some(c) = app.selected_container() {
                if let Some(stack) = stack_name_from_labels(&c.labels) {
                    format!("/{stack}/{}", c.name)
                } else {
                    format!("/{}", c.name)
                }
            } else {
                String::new()
            }
        }
        ShellView::Images => app
            .selected_image()
            .map(|i| format!("/{}", i.name()))
            .unwrap_or_default(),
        ShellView::Volumes => app
            .selected_volume()
            .map(|v| format!("/{}", v.name))
            .unwrap_or_default(),
        ShellView::Networks => app
            .selected_network()
            .map(|n| format!("/{}", n.name))
            .unwrap_or_default(),
        ShellView::Templates => match app.templates_kind {
            TemplatesKind::Stacks => app
                .selected_template()
                .map(|t| format!("/{}", t.name))
                .unwrap_or_default(),
            TemplatesKind::Networks => app
                .selected_net_template()
                .map(|t| format!("/{}", t.name))
                .unwrap_or_default(),
        },
        ShellView::Inspect => app
            .inspect_target
            .as_ref()
            .map(|t| format!("/{}", t.label))
            .unwrap_or_default(),
        ShellView::Logs => app
            .logs_for_id
            .as_ref()
            .and_then(|_| app.selected_container().map(|c| c.name.clone()))
            .map(|n| format!("/{n}"))
            .unwrap_or_default(),
        ShellView::Help => String::new(),
        ShellView::Messages => String::new(),
    }
}

fn draw_shell_body(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    if app.shell_sidebar_hidden {
        draw_shell_main(f, app, area);
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if app.shell_sidebar_collapsed { 18 } else { 28 }),
            Constraint::Min(1),
        ])
        .split(area);
    draw_shell_sidebar(f, app, cols[0]);
    draw_shell_main(f, app, cols[1]);
}

fn draw_shell_sidebar(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Sidebar {
        Style::default()
            .bg(Color::Rgb(26, 26, 30))
            .fg(Color::Rgb(220, 220, 220))
    } else {
        Style::default()
            .bg(Color::Rgb(20, 20, 20))
            .fg(Color::Rgb(220, 220, 220))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner_area = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let inner_w = inner_area.width.max(1) as usize;

    let items = shell_sidebar_items(app);
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
                rendered.push(ListItem::new(Line::from(Span::styled(
                    "─".repeat(inner_w),
                    bg.fg(Color::Rgb(45, 45, 45)),
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
                        bg.fg(Color::Rgb(140, 140, 140))
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
                        shell_row_highlight(app).fg(Color::White)
                    } else {
                        bg.fg(Color::Rgb(140, 140, 140))
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
                    bg.fg(Color::Rgb(200, 200, 200))
                };
                if app.shell_sidebar_collapsed {
                    rendered.push(ListItem::new(Line::from(Span::styled(base, base_style))));
                } else {
                    // Show action chords as Ctrl-based hints.
                    let hint = format!("[{}]", a.ctrl_hint());
                    let hint_len = hint.chars().count();
                    let left_max = inner_w.saturating_sub(hint_len.saturating_add(1)).max(1);
                    let base_shown = truncate_end(&base, left_max);
                    let base_len = base_shown.chars().count();
                    let gap = inner_w.saturating_sub(base_len.saturating_add(hint_len));
                    let hint_style = if selected {
                        shell_row_highlight(app).fg(Color::White)
                    } else {
                        bg.fg(Color::Rgb(140, 140, 140))
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
}

fn draw_shell_main(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);

    let is_full = matches!(app.shell_view, ShellView::Logs | ShellView::Inspect);
    let is_split_view = matches!(
        app.shell_view,
        ShellView::Containers
            | ShellView::Images
            | ShellView::Volumes
            | ShellView::Networks
            | ShellView::Templates
    );

    if is_split_view && app.shell_split_mode == ShellSplitMode::Vertical {
        let parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Length(1),
                Constraint::Percentage(50),
            ])
            .split(area);
        draw_shell_main_list(f, app, parts[0]);
        draw_shell_vr(f, parts[1]);
        draw_shell_main_details(f, app, parts[2]);
        return;
    }

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            if matches!(
                app.shell_view,
                ShellView::Logs | ShellView::Inspect | ShellView::Messages | ShellView::Help
            ) {
                // Keep the meta area compact (3 lines) and centered.
                [
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                ]
            } else if is_full {
                [
                    Constraint::Percentage(85),
                    Constraint::Length(1),
                    Constraint::Percentage(15),
                ]
            } else {
                [
                    Constraint::Percentage(62),
                    Constraint::Length(1),
                    Constraint::Percentage(38),
                ]
            },
        )
        .split(area);

    draw_shell_main_list(f, app, parts[0]);
    draw_shell_hr(f, parts[1]);
    draw_shell_main_details(f, app, parts[2]);
}

fn draw_shell_hr(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let st = Style::default()
        .fg(Color::Rgb(45, 45, 45))
        .bg(Color::Rgb(16, 16, 16));
    let line = "─".repeat(area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(line).style(st).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_vr(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let st = Style::default()
        .fg(Color::Rgb(45, 45, 45))
        .bg(Color::Rgb(16, 16, 16));
    let line = "│".repeat(area.height.max(1) as usize);
    f.render_widget(
        Paragraph::new(line).style(st).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_title(
    f: &mut ratatui::Frame,
    app: &App,
    title: &str,
    count: usize,
    area: ratatui::layout::Rect,
) {
    // Subtle focus indication: highlight the list title when list has focus.
    let bg = if app.shell_focus == ShellFocus::List {
        Style::default().bg(Color::Rgb(30, 30, 36)).fg(Color::White)
    } else {
        Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White)
    };
    f.render_widget(Block::default().style(bg), area);
    let left = format!(" {title} ({count})");
    let shown = truncate_end(&left, area.width.max(1) as usize);
    f.render_widget(
        Paragraph::new(shown)
            .style(bg.fg(if app.shell_focus == ShellFocus::List {
                Color::Rgb(235, 235, 235)
            } else {
                Color::Rgb(200, 200, 200)
            }))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_main_list(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let title_area = chunks[0];
    let content_area = chunks[1];

    match app.shell_view {
        ShellView::Containers => {
            draw_shell_title(f, app, "Containers", app.containers.len(), title_area);
            draw_shell_containers_table(f, app, content_area);
        }
        ShellView::Images => {
            draw_shell_title(f, app, "Images", app.images_visible_len(), title_area);
            draw_shell_images_table(f, app, content_area);
        }
        ShellView::Volumes => {
            draw_shell_title(f, app, "Volumes", app.volumes_visible_len(), title_area);
            draw_shell_volumes_table(f, app, content_area);
        }
        ShellView::Networks => {
            draw_shell_title(f, app, "Networks", app.networks.len(), title_area);
            draw_shell_networks_table(f, app, content_area);
        }
        ShellView::Templates => {
            match app.templates_kind {
                TemplatesKind::Stacks => {
                    draw_shell_title(f, app, "Templates: Stacks", app.templates.len(), title_area);
                }
                TemplatesKind::Networks => {
                    draw_shell_title(
                        f,
                        app,
                        "Templates: Networks",
                        app.net_templates.len(),
                        title_area,
                    );
                }
            }
            draw_shell_templates_table(f, app, content_area);
        }
        ShellView::Logs => {
            draw_shell_title(f, app, "Logs", app.logs_total_lines(), title_area);
            draw_shell_logs_view(f, app, content_area);
        }
        ShellView::Inspect => {
            draw_shell_title(f, app, "Inspect", app.inspect_lines.len(), title_area);
            draw_shell_inspect_view(f, app, content_area);
        }
        ShellView::Help => {
            draw_shell_title(f, app, "Help", 0, title_area);
            draw_shell_help_view(f, app, content_area);
        }
        ShellView::Messages => {
            draw_shell_title(f, app, "Messages", app.session_msgs.len(), title_area);
            draw_shell_messages_view(f, app, content_area);
        }
    }
}

fn draw_shell_main_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.shell_view {
        ShellView::Containers => draw_shell_container_details(f, app, area),
        ShellView::Images => draw_shell_image_details(f, app, area),
        ShellView::Volumes => draw_shell_volume_details(f, app, area),
        ShellView::Networks => draw_shell_network_details(f, app, area),
        ShellView::Templates => draw_shell_template_details(f, app, area),
        ShellView::Logs => draw_shell_logs_meta(f, app, area),
        ShellView::Inspect => draw_shell_inspect_meta(f, app, area),
        ShellView::Help => draw_shell_help_meta(f, app, area),
        ShellView::Messages => draw_shell_messages_meta(f, app, area),
    }
}

fn draw_shell_footer(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = app.theme.footer.to_style();
    f.render_widget(Block::default().style(bg), area);

    let hint = match app.shell_view {
        ShellView::Containers => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Images | ShellView::Volumes | ShellView::Networks => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Templates => {
            " F1 help  b sidebar  ^p layout  :q quit"
        }
        ShellView::Logs => {
            " F1 help  / search  : cmd  n/N match  m regex  l numbers  q back  :q quit"
        }
        ShellView::Inspect => {
            " F1 help  / search  : cmd  n/N match  m regex  Enter expand  q back  :q quit"
        }
        ShellView::Help => " F1 help  Up/Down scroll  PageUp/PageDown  q back  :q quit",
        ShellView::Messages => {
            " F1 help  Up/Down select  Left/Right hscroll  PgUp/PgDn  ^c copy  ^g toggle  q back  :q quit"
        }
    };

    let w = area.width.max(1) as usize;
    let line = Line::from(vec![Span::styled(
        truncate_end(hint, w),
        bg.fg(theme::parse_color(&app.theme.footer.fg)),
    )]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_shell_cmdline(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = Style::default()
        .bg(Color::Rgb(16, 16, 16))
        .fg(Color::Rgb(220, 220, 220));
    f.render_widget(Block::default().style(bg), area);

    let (mode, prefix, input, cursor, show_cursor): (&str, &str, String, usize, bool) =
        if app.shell_cmd_mode {
            if let Some(confirm) = &app.shell_confirm {
                ("CONFIRM", ":", format!("{} (y/n)", confirm.label), 0, false)
            } else {
                (
                    "COMMAND",
                    ":",
                    app.shell_cmd_input.clone(),
                    app.shell_cmd_cursor,
                    true,
                )
            }
        } else {
            match app.shell_view {
                ShellView::Logs => match app.logs_mode {
                    LogsMode::Normal => ("NORMAL", "", String::new(), 0, false),
                    LogsMode::Search => ("SEARCH", "/", app.logs_input.clone(), app.logs_input_cursor, true),
                    LogsMode::Command => (
                        "COMMAND",
                        ":",
                        app.logs_command.clone(),
                        app.logs_command_cursor,
                        true,
                    ),
                },
                ShellView::Inspect => match app.inspect_mode {
                    InspectMode::Normal => ("NORMAL", "", String::new(), 0, false),
                    InspectMode::Search => (
                        "SEARCH",
                        "/",
                        app.inspect_input.clone(),
                        app.inspect_input_cursor,
                        true,
                    ),
                    InspectMode::Command => (
                        "COMMAND",
                        ":",
                        app.inspect_input.clone(),
                        app.inspect_input_cursor,
                        true,
                    ),
                },
                _ => ("NORMAL", "", String::new(), 0, false),
            }
        };

    let w = area.width.max(1) as usize;
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(
        format!(" {mode} "),
        Style::default()
            .fg(Color::Rgb(160, 160, 160))
            .bg(Color::Rgb(16, 16, 16)),
    ));

    if !prefix.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

        let fixed_len = format!(" {mode} ").chars().count() + 1 + prefix.chars().count();
        let avail = w.saturating_sub(fixed_len).max(1);
        if show_cursor {
            let input_w = avail.saturating_sub(1).max(1);
            let (before, at, after) = input_window_with_cursor(&input, cursor, input_w);
            spans.push(Span::styled(before, bg));
            spans.push(Span::styled(
                at,
                Style::default().fg(Color::Black).bg(Color::Rgb(220, 220, 220)),
            ));
            spans.push(Span::styled(after, bg));
        } else {
            spans.push(Span::styled(truncate_end(&input, avail), bg.fg(Color::Rgb(180, 180, 180))));
        }
    } else {
        spans.push(Span::styled(
            "  (press : for commands)",
            Style::default().fg(Color::Rgb(120, 120, 120)),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .style(bg)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn shell_row_highlight(app: &App) -> Style {
    // Keep selection color consistent across all lists (sidebar/table).
    // Focus is indicated elsewhere (list title / details background).
    // Do not force foreground color so marked rows (yellow) stay visible when selected.
    app.theme.list_selected.to_style()
}

fn shell_header_style(app: &App) -> Style {
    app.theme.table_header.to_style()
}

fn draw_shell_containers_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Reuse existing container row computation logic, but render without outer borders.
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);

    app.ensure_view();
    if app.containers.is_empty() {
        let msg = if app.loading {
            let spinner = loading_spinner(app.loading_since);
            format!("Loading... {spinner}")
        } else if app.last_error.is_some() {
            "Failed to load (see status)".to_string()
        } else {
            "No containers".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(
                    Style::default()
                        .fg(Color::Rgb(140, 140, 140))
                        .bg(Color::Rgb(16, 16, 16)),
                )
                .wrap(Wrap { trim: true }),
            area.inner(ratatui::layout::Margin {
                vertical: 0,
                horizontal: 1,
            }),
        );
        return;
    }

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let header = Row::new(vec![
        Cell::from("NAME"),
        Cell::from("IMAGE"),
        Cell::from("CPU"),
        Cell::from("MEM"),
        Cell::from("STATUS"),
        Cell::from("IP"),
    ])
        .style(shell_header_style(app));

    let mut rows: Vec<Row> = Vec::new();

    let make_container_row = |c: &ContainerRow, name_prefix: &str| -> Row {
        let stopped = is_container_stopped(&c.status);
        let marked = app.is_marked(&c.id);
        let row_style = if marked {
            app.theme.marked.to_style()
        } else if stopped {
            Style::default()
                .fg(Color::Rgb(120, 120, 120))
                .add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };

        let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
        let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
        let ip = app
            .ip_cache
            .get(&c.id)
            .map(|(ip, _)| ip.as_str())
            .unwrap_or("-");
        let status = if let Some(marker) = app.action_inflight.get(&c.id) {
            action_status_prefix(marker.action).to_string()
        } else {
            c.status.clone()
        };

        let name = format!("{name_prefix}{}", c.name);
        Row::new(vec![
            Cell::from(truncate_end(&name, 22)).style(row_style),
            Cell::from(truncate_end(&c.image, 40)).style(row_style),
            Cell::from(cpu).style(row_style),
            Cell::from(mem).style(row_style),
            Cell::from(status).style(row_style),
            Cell::from(truncate_end(ip, 15)).style(row_style),
        ])
        .style(row_style)
    };

    if app.list_mode == ListMode::Tree {
        for e in &app.view {
            match e {
                ViewEntry::StackHeader {
                    name,
                    total,
                    running,
                    expanded,
                } => {
                    let st = if *running == 0 {
                        Style::default()
                            .fg(Color::Rgb(110, 110, 110))
                            .add_modifier(Modifier::BOLD)
                    } else if *running == *total {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD)
                    };
                    let glyph = if *expanded { "▾" } else { "▸" };
                    rows.push(
                        Row::new(vec![
                            Cell::from(format!("{glyph} {name}")).style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                        ])
                        .style(st),
                    );
                }
                ViewEntry::UngroupedHeader { total, running } => {
                    let st = Style::default()
                        .fg(Color::Rgb(180, 180, 180))
                        .add_modifier(Modifier::BOLD);
                    rows.push(
                        Row::new(vec![
                            Cell::from("Ungrouped").style(st),
                            Cell::from(format!("{running}/{total}")).style(st),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                            Cell::from(""),
                        ])
                        .style(st),
                    );
                }
                ViewEntry::Container { id, indent, .. } => {
                    if let Some(idx) = app.container_idx_by_id.get(id).copied() {
                        if let Some(c) = app.containers.get(idx) {
                            let prefix = "  ".repeat(*indent);
                            rows.push(make_container_row(c, &prefix));
                        }
                    }
                }
            }
        }
    } else {
        for c in &app.containers {
            rows.push(make_container_row(c, ""));
        }
    }

    // Keep the same column widths as before; only remove the visual separators.
    let widths = [
        Constraint::Length(22),
        Constraint::Min(20),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(22),
        Constraint::Length(15),
    ];

    let mut state = TableState::default();
    state.select(Some(app.selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(rows, widths)
        .header(header)
        .style(bg)
        .column_spacing(1)
        .row_highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_images_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    const REF_TEXT_MAX: usize = 62;
    const ID_TEXT_MAX: usize = 50;
    const SIZE_W: usize = 10;
    const REF_MIN_W: usize = 24;
    const ID_MIN_W: usize = 10;

    let size_cell = |s: &str| -> String {
        // SIZE values are ASCII (e.g. "294MB", "2.06GB"), so fixed-width padding is fine.
        if s.chars().count() >= SIZE_W {
            truncate_end(s, SIZE_W)
        } else {
            format!("{:>width$}", s, width = SIZE_W)
        }
    };

    // Keep columns compact: size REF/ID to the actual visible content (capped),
    // instead of stretching REF to fill the entire view.
    let mut max_ref = 0usize;
    let mut max_id = 0usize;
    let mut rows: Vec<Row> = Vec::new();
    for img in app
        .images
        .iter()
        .filter(|img| !app.images_unused_only || !app.image_referenced(img))
    {
        let reference_full = img.name();
        let reference = truncate_end(&reference_full, REF_TEXT_MAX);
        let id = truncate_end(&img.id, ID_TEXT_MAX);
        let marked = app.is_image_marked(&App::image_row_key(img));
        let row_style = if marked {
            app.theme.marked.to_style()
        } else {
            Style::default()
        };
        max_ref = max_ref.max(reference.chars().count());
        max_id = max_id.max(id.chars().count());
        rows.push(
            Row::new(vec![
                Cell::from(reference),
                Cell::from(id),
                Cell::from(size_cell(&img.size)),
            ])
            .style(row_style),
        );
    }
    let ref_w = max_ref.clamp(REF_MIN_W, REF_TEXT_MAX);
    let id_w = max_id.clamp(ID_MIN_W, ID_TEXT_MAX);

    let mut state = TableState::default();
    state.select(Some(app.images_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Length(ref_w as u16),
            Constraint::Length(id_w as u16),
            Constraint::Length(SIZE_W as u16),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("REF"),
            Cell::from("ID"),
            Cell::from(size_cell("SIZE")),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_volumes_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let rows: Vec<Row> = app
        .volumes
        .iter()
        .filter(|v| !app.volumes_unused_only || !app.volume_referenced(v))
        .map(|v| {
            let used = app
                .volume_referenced_count_by_name
                .get(&v.name)
                .copied()
                .unwrap_or(0);
            let marked = app.is_volume_marked(&v.name);
            let st = if marked {
                app.theme.marked.to_style()
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(v.name.clone()),
                Cell::from(v.driver.clone()),
                Cell::from(if used == 0 {
                    "unused".to_string()
                } else {
                    format!("{used} ctr")
                }),
            ])
            .style(st)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(app.volumes_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Min(22),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("DRIVER"),
            Cell::from("USED"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_networks_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let rows: Vec<Row> = app
        .networks
        .iter()
        .map(|n| {
            let marked = app.is_network_marked(&n.id);
            let st = if marked {
                app.theme.marked.to_style()
            } else if App::is_system_network(n) {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(n.name.clone()),
                Cell::from(n.id.clone()),
                Cell::from(n.driver.clone()),
                Cell::from(n.scope.clone()),
            ])
            .style(st)
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.networks_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            // Keep NAME compact so ID can expand.
            Constraint::Length(16),
            Constraint::Min(16),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("ID"),
            Cell::from("DRIVER"),
            Cell::from("SCOPE"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_stack_templates_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if let Some(err) = &app.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(
                    Style::default()
                        .fg(Color::Rgb(220, 120, 120))
                        .bg(Color::Rgb(16, 16, 16)),
                )
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if app.templates.is_empty() {
        let msg = format!("No templates in {}", app.stack_templates_dir().display());
        f.render_widget(
            Paragraph::new(msg)
                .style(
                    Style::default()
                        .fg(Color::Rgb(140, 140, 140))
                        .bg(Color::Rgb(16, 16, 16)),
                )
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let now = Instant::now();
    let rows: Vec<Row> = app
        .templates
        .iter()
        .map(|t| {
            let state = if let Some(m) = app.template_deploy_inflight.get(&t.name) {
                let secs = now.duration_since(m.started).as_secs();
                format!("deploy {secs}s")
            } else {
                String::new()
            };
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_compose { "yes" } else { "no" }),
                Cell::from(state),
                Cell::from(t.desc.clone()),
            ])
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.templates_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("COMPOSE"),
            Cell::from("STATE"),
            Cell::from("DESC"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_templates_table(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.templates_kind {
        TemplatesKind::Stacks => draw_shell_stack_templates_table(f, app, area),
        TemplatesKind::Networks => draw_shell_net_templates_table(f, app, area),
    }
}

fn draw_shell_net_templates_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if let Some(err) = &app.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(
                    Style::default()
                        .fg(Color::Rgb(220, 120, 120))
                        .bg(Color::Rgb(16, 16, 16)),
                )
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    if app.net_templates.is_empty() {
        let msg = format!("No network templates in {}", app.net_templates_dir().display());
        f.render_widget(
            Paragraph::new(msg)
                .style(
                    Style::default()
                        .fg(Color::Rgb(140, 140, 140))
                        .bg(Color::Rgb(16, 16, 16)),
                )
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let now = Instant::now();
    let rows: Vec<Row> = app
        .net_templates
        .iter()
        .map(|t| {
            let state = if let Some(m) = app.net_template_deploy_inflight.get(&t.name) {
                let secs = now.duration_since(m.started).as_secs();
                format!("deploy {secs}s")
            } else {
                String::new()
            };
            Row::new(vec![
                Cell::from(t.name.clone()),
                Cell::from(if t.has_cfg { "yes" } else { "no" }),
                Cell::from(state),
                Cell::from(t.desc.clone()),
            ])
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.net_templates_selected
            .min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("NAME"),
            Cell::from("CFG"),
            Cell::from("STATE"),
            Cell::from("DESC"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}

fn draw_shell_container_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default().bg(Color::Rgb(24, 24, 30)).fg(Color::White)
    } else {
        Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White)
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let Some(c) = app.selected_container() else {
        f.render_widget(
            Paragraph::new("Select a container to see details.")
                .style(bg.fg(Color::Rgb(140, 140, 140)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let key = Style::default()
        .fg(Color::Rgb(140, 140, 140))
        .bg(Color::Rgb(16, 16, 16));
    let val = Style::default().fg(Color::White).bg(Color::Rgb(16, 16, 16));
    let kv = |k: &str, v: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("{k}: "), key),
            Span::styled(v, val),
        ])
    };
    let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
    let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
    let ip = app
        .ip_cache
        .get(&c.id)
        .map(|(ip, _)| ip.clone())
        .unwrap_or_else(|| "-".to_string());
    let lines = vec![
        kv("Name", c.name.clone()),
        kv("ID", c.id.clone()),
        kv("Image", c.image.clone()),
        kv("Status", c.status.clone()),
        kv("CPU / MEM", format!("{cpu} / {mem}")),
        kv("IP", ip),
        kv("Ports", c.ports.clone()),
    ];
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_image_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default().bg(Color::Rgb(24, 24, 30)).fg(Color::White)
    } else {
        Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White)
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let Some(img) = app.selected_image() else {
        return;
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Ref: ", Style::default().fg(Color::Gray)),
            Span::raw(img.name()),
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(img.id.clone()),
        ]),
        Line::from(vec![
            Span::styled("Size: ", Style::default().fg(Color::Gray)),
            Span::raw(img.size.clone()),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_volume_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default().bg(Color::Rgb(24, 24, 30)).fg(Color::White)
    } else {
        Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White)
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let Some(v) = app.selected_volume() else {
        return;
    };
    let used_by = app
        .volume_containers_by_name
        .get(&v.name)
        .map(|xs| xs.join(", "))
        .unwrap_or_else(|| "-".to_string());
    let lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Gray)),
            Span::raw(v.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Driver: ", Style::default().fg(Color::Gray)),
            Span::raw(v.driver.clone()),
        ]),
        Line::from(vec![
            Span::styled("Used by: ", Style::default().fg(Color::Gray)),
            Span::raw(used_by),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_network_details(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default().bg(Color::Rgb(24, 24, 30)).fg(Color::White)
    } else {
        Style::default().bg(Color::Rgb(16, 16, 16)).fg(Color::White)
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let Some(n) = app.selected_network() else {
        return;
    };
    let is_system = App::is_system_network(n);
    let lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Gray)),
            Span::raw(n.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Gray)),
            if is_system {
                Span::styled(
                    "System",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    "User",
                    Style::default()
                        .fg(Color::White)
                        ,
                )
            },
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Gray)),
            Span::raw(n.id.clone()),
        ]),
        Line::from(vec![
            Span::styled("Driver: ", Style::default().fg(Color::Gray)),
            Span::raw(n.driver.clone()),
        ]),
        Line::from(vec![
            Span::styled("Scope: ", Style::default().fg(Color::Gray)),
            Span::raw(n.scope.clone()),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_stack_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default()
            .bg(Color::Rgb(24, 24, 30))
            .fg(Color::Rgb(200, 200, 200))
    } else {
        Style::default()
            .bg(Color::Rgb(16, 16, 16))
            .fg(Color::Rgb(200, 200, 200))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });

    if let Some(err) = &app.templates_error {
        f.render_widget(
            Paragraph::new(format!("Templates error: {err}"))
                .style(bg.fg(Color::Rgb(220, 120, 120)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_template() else {
        f.render_widget(
            Paragraph::new("No template selected.")
                .style(bg.fg(Color::Rgb(140, 140, 140)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    if !t.has_compose {
        f.render_widget(
            Paragraph::new("compose.yaml not found in template directory.")
                .style(bg.fg(Color::Rgb(220, 120, 120)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let content =
        fs::read_to_string(&t.compose_path).unwrap_or_else(|e| format!("read failed: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let lnw = lines.len().max(1).to_string().len();
    let view_h = inner.height.max(1) as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.templates_details_scroll = app.templates_details_scroll.min(max_scroll);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len().max(1));
    let ln_style = Style::default()
        .fg(Color::Rgb(110, 110, 110))
        .bg(bg.bg.unwrap_or(Color::Reset));

    for (i, l) in lines.iter().enumerate() {
        let ln = format!("{:>lnw$} ", i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(ln, ln_style));
        spans.extend(yaml_highlight_line(l, bg));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(format!("{:>lnw$} ", 1), ln_style)));
    }

    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.templates_details_scroll.min(u16::MAX as usize) as u16,
            0,
        )),
        inner,
    );
}

fn draw_shell_template_details(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.templates_kind {
        TemplatesKind::Stacks => draw_shell_stack_template_details(f, app, area),
        TemplatesKind::Networks => draw_shell_net_template_details(f, app, area),
    }
}

fn draw_shell_net_template_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default()
            .bg(Color::Rgb(24, 24, 30))
            .fg(Color::Rgb(200, 200, 200))
    } else {
        Style::default()
            .bg(Color::Rgb(16, 16, 16))
            .fg(Color::Rgb(200, 200, 200))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });

    if let Some(err) = &app.net_templates_error {
        f.render_widget(
            Paragraph::new(format!("Net templates error: {err}"))
                .style(bg.fg(Color::Rgb(220, 120, 120)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let Some(t) = app.selected_net_template() else {
        f.render_widget(
            Paragraph::new("No network template selected.")
                .style(bg.fg(Color::Rgb(140, 140, 140)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };

    if !t.has_cfg {
        f.render_widget(
            Paragraph::new("network.json not found in template directory.")
                .style(bg.fg(Color::Rgb(220, 120, 120)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let content = fs::read_to_string(&t.cfg_path).unwrap_or_else(|e| format!("read failed: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let lnw = lines.len().max(1).to_string().len();
    let view_h = inner.height.max(1) as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.net_templates_details_scroll = app.net_templates_details_scroll.min(max_scroll);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len().max(1));
    let ln_style = Style::default()
        .fg(Color::Rgb(110, 110, 110))
        .bg(bg.bg.unwrap_or(Color::Reset));

    for (i, l) in lines.iter().enumerate() {
        let ln = format!("{:>lnw$} ", i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(ln, ln_style));
        spans.extend(json_highlight_line(l, bg));
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(format!("{:>lnw$} ", 1), ln_style)));
    }

    f.render_widget(
        Paragraph::new(Text::from(out)).style(bg).scroll((
            app.net_templates_details_scroll
                .min(u16::MAX as usize) as u16,
            0,
        )),
        inner,
    );
}

fn yaml_highlight_line(line: &str, base: Style) -> Vec<Span<'static>> {
    // Very small YAML-ish highlighter:
    // - comments: dim
    // - mapping keys: light blue
    let normal = base.fg(Color::Rgb(200, 200, 200));
    let comment = base.fg(Color::Rgb(120, 120, 120));
    let key_style = base.fg(Color::Rgb(140, 190, 255));

    let (code, comment_part) = split_yaml_comment(line);
    let mut spans: Vec<Span<'static>> = Vec::new();

    if code.trim().is_empty() {
        if !code.is_empty() {
            spans.push(Span::styled(code.to_string(), normal));
        }
    } else if let Some((prefix, key, rest)) = split_yaml_key(code) {
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix.to_string(), normal));
        }
        spans.push(Span::styled(key.to_string(), key_style));
        if !rest.is_empty() {
            spans.push(Span::styled(rest.to_string(), normal));
        }
    } else {
        spans.push(Span::styled(code.to_string(), normal));
    }

    if let Some(c) = comment_part {
        spans.push(Span::styled(c.to_string(), comment));
    }
    spans
}

fn json_highlight_line(line: &str, base: Style) -> Vec<Span<'static>> {
    // Minimal JSON-ish highlighter:
    // - keys ("...":) in light blue
    let normal = base.fg(Color::Rgb(200, 200, 200));
    let key_style = base.fg(Color::Rgb(140, 190, 255));

    let mut spans: Vec<Span<'static>> = Vec::new();
    let Some(start) = line.find('"') else {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    };
    let rest = &line[start + 1..];
    let Some(end_rel) = rest.find('"') else {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    };
    let end = start + 1 + end_rel;
    let after = &line[end + 1..];
    // Only treat it as a key if a ':' follows (allow whitespace).
    let after_trim = after.trim_start();
    if !after_trim.starts_with(':') {
        spans.push(Span::styled(line.to_string(), normal));
        return spans;
    }

    let prefix = &line[..start];
    let key = &line[start..=end];
    let rest = &line[end + 1..];
    if !prefix.is_empty() {
        spans.push(Span::styled(prefix.to_string(), normal));
    }
    spans.push(Span::styled(key.to_string(), key_style));
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), normal));
    }
    spans
}

fn split_yaml_comment(line: &str) -> (&str, Option<&str>) {
    // Find a '#' that is not inside single/double quotes.
    let mut in_s = false;
    let mut in_d = false;
    let mut prev_bs = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => {
                in_s = !in_s;
                prev_bs = false;
            }
            '"' if !in_s && !prev_bs => {
                in_d = !in_d;
                prev_bs = false;
            }
            '\\' if in_d => {
                prev_bs = !prev_bs;
            }
            '#' if !in_s && !in_d => {
                return (&line[..i], Some(&line[i..]));
            }
            _ => prev_bs = false,
        }
    }
    (line, None)
}

fn split_yaml_key(line: &str) -> Option<(&str, &str, &str)> {
    // Attempts to split "<prefix><key>:<rest>" where key is outside quotes.
    let mut in_s = false;
    let mut in_d = false;
    let mut prev_bs = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_d => {
                in_s = !in_s;
                prev_bs = false;
            }
            '"' if !in_s && !prev_bs => {
                in_d = !in_d;
                prev_bs = false;
            }
            '\\' if in_d => {
                prev_bs = !prev_bs;
            }
            ':' if !in_s && !in_d => {
                let (left, _right) = line.split_at(i);
                // Walk back to find key token (support "- key:" too).
                let bytes = left.as_bytes();
                let mut j = bytes.len();
                while j > 0 && bytes[j - 1].is_ascii_whitespace() {
                    j -= 1;
                }
                let key_end = j;
                while j > 0 {
                    let b = bytes[j - 1];
                    if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                        j -= 1;
                    } else {
                        break;
                    }
                }
                let key_start = j;
                if key_start == key_end {
                    return None;
                }
                let prefix = &left[..key_start];
                let key = &left[key_start..key_end];
                let rest = &line[key_end..];
                return Some((prefix, key, rest));
            }
            _ => prev_bs = false,
        }
    }
    None
}

fn draw_shell_logs_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Reuse the underlying log renderer, but in a borderless main view.
    let bg = Style::default().bg(Color::Rgb(12, 12, 12)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);

    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let content = cols[0];
    let vbar_area = cols[1];

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(content);
    let list_area = rows[0];
    let hbar_area = rows[1];

    let effective_query = match app.logs_mode {
        LogsMode::Search => app.logs_input.trim(),
        LogsMode::Normal | LogsMode::Command => app.logs_query.trim(),
    };

    let view_height = list_area.height.max(1) as usize;
    let total_lines = app.logs_total_lines();
    let max_scroll = total_lines.saturating_sub(view_height);
    let cursor = if total_lines == 0 {
        0usize
    } else {
        app.logs_cursor.min(total_lines.saturating_sub(1))
    };
    let mut scroll_top = app.logs_scroll_top.min(max_scroll);
    if cursor < scroll_top {
        scroll_top = cursor;
    } else if cursor >= scroll_top.saturating_add(view_height) {
        scroll_top = cursor
            .saturating_add(1)
            .saturating_sub(view_height)
            .min(max_scroll);
    }
    app.logs_scroll_top = scroll_top;

    if app.logs_loading || app.logs_error.is_some() || app.logs_text.is_none() {
        let msg = if app.logs_loading {
            "Loading…".to_string()
        } else if let Some(e) = &app.logs_error {
            format!("error: {e}")
        } else {
            "No logs loaded.".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(
                    Style::default()
                        .fg(Color::Rgb(140, 140, 140))
                        .bg(Color::Rgb(12, 12, 12)),
                )
                .wrap(Wrap { trim: true }),
            list_area,
        );
        return;
    }

    let Some(txt) = &app.logs_text else {
        return;
    };
    let total = total_lines.max(1);
    let digits = total.to_string().len().max(1);
    let start = scroll_top;
    let end = (start + view_height).min(total_lines);
    let prefix_w = if app.logs_show_line_numbers {
        digits.saturating_add(1)
    } else {
        0
    };
    let avail_w = list_area.width.max(1) as usize;
    let body_w = avail_w.saturating_sub(prefix_w).max(1);
    let max_hscroll = app.logs_max_width.saturating_sub(body_w);
    app.logs_hscroll = app.logs_hscroll.min(max_hscroll);

    let q = effective_query;
    let sel = app.logs_selection_range();
    let mut items: Vec<ListItem> = Vec::with_capacity(end.saturating_sub(start));
    for (idx, line) in txt.lines().enumerate().take(end).skip(start) {
        let visible = slice_window(line, app.logs_hscroll, body_w);
        let mut l = if app.logs_use_regex {
            let matcher = if q.is_empty() || app.logs_regex_error.is_some() {
                None
            } else {
                app.logs_regex.as_ref()
            };
            highlight_log_line_regex(&visible, matcher)
        } else {
            highlight_log_line_literal(&visible, q)
        };
        if app.logs_show_line_numbers {
            let prefix = format!("{:>width$} ", idx + 1, width = digits);
            l.spans.insert(
                0,
                Span::styled(
                    prefix,
                    Style::default()
                        .fg(Color::Rgb(140, 140, 140))
                        .bg(Color::Rgb(12, 12, 12)),
                ),
            );
        }
        let selected = sel.map(|(a, b)| idx >= a && idx <= b).unwrap_or(false);
        let item_style = if selected {
            app.theme.marked.to_style()
        } else {
            Style::default()
        };
        items.push(ListItem::new(l).style(item_style));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }
    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(start)));
    f.render_stateful_widget(list, list_area, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        scroll_top,
        max_scroll,
        total_lines,
        view_height,
        app.ascii_only,
    );
    draw_shell_scrollbar_h(
        f,
        hbar_area,
        app.logs_hscroll,
        max_hscroll,
        app.logs_max_width,
        body_w,
        app.ascii_only,
    );
}

fn draw_shell_logs_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default()
            .bg(Color::Rgb(24, 24, 30))
            .fg(Color::Rgb(200, 200, 200))
    } else {
        Style::default()
            .bg(Color::Rgb(16, 16, 16))
            .fg(Color::Rgb(200, 200, 200))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let q = app.logs_query.trim();
    let matches = if q.is_empty() {
        "Matches: -".to_string()
    } else if app.logs_use_regex && app.logs_regex_error.is_some() {
        "Regex: invalid".to_string()
    } else {
        format!("Matches: {}", app.logs_match_lines.len())
    };
    let re = if app.logs_use_regex {
        "regex:on"
    } else {
        "regex:off"
    };
    let pos = format!(
        "Line: {}/{}",
        app.logs_cursor.saturating_add(1),
        app.logs_total_lines().max(1)
    );
    let line = Line::from(vec![
        Span::styled(matches, Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("Query: ", Style::default().fg(Color::Gray)),
        Span::styled(
            if q.is_empty() { "-" } else { q },
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled(re, Style::default().fg(Color::Gray)),
        Span::raw("   "),
        Span::styled(pos, Style::default().fg(Color::Gray)),
    ]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_inspect_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Reuse inspect tree lines computed in app.inspect_lines.
    let bg = Style::default().bg(Color::Rgb(12, 12, 12)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let content = cols[0];
    let vbar_area = cols[1];

    let view_height = content.height.max(1) as usize;
    let total_lines = app.inspect_lines.len();
    let max_scroll = total_lines.saturating_sub(view_height);
    let cursor = app.inspect_selected.min(total_lines.saturating_sub(1));
    let mut scroll_top = app.inspect_scroll_top.min(max_scroll);
    if cursor < scroll_top {
        scroll_top = cursor;
    } else if cursor >= scroll_top.saturating_add(view_height) {
        scroll_top = cursor
            .saturating_add(1)
            .saturating_sub(view_height)
            .min(max_scroll);
    }
    app.inspect_scroll_top = scroll_top;

    let start = scroll_top;
    let end = (start + view_height).min(total_lines);
    let avail_w = content.width.max(1) as usize;

    // Clamp horizontal scroll so it does not "virtually" exceed the content width.
    let mut max_len: usize = 0;
    for l in &app.inspect_lines {
        let label_len = l.label.chars().count();
        let summary_len = l.summary.chars().count();
        let line_len = l.depth.saturating_mul(2)
            + 2
            + label_len
            + if summary_len > 0 { 2 + summary_len } else { 0 };
        max_len = max_len.max(line_len);
    }
    let max_hscroll = max_len.saturating_sub(avail_w);
    app.inspect_scroll = app.inspect_scroll.min(max_hscroll);

    let q = app.inspect_query.trim();
    let mut items: Vec<ListItem> = Vec::with_capacity(end.saturating_sub(start));
    for l in app.inspect_lines.iter().take(end).skip(start) {
        let indent = "  ".repeat(l.depth);
        let glyph = if l.expandable {
            if l.expanded { "▾ " } else { "▸ " }
        } else {
            "  "
        };
        let mut text = format!("{indent}{glyph}{}", l.label);
        if !l.summary.is_empty() {
            text.push_str(": ");
            text.push_str(&l.summary);
        }
        let visible = slice_window(&text, app.inspect_scroll, avail_w);
        let line = if app.inspect_mode == InspectMode::Search && !q.is_empty() {
            highlight_log_line_literal(&visible, q)
        } else {
            if l.matches {
                highlight_log_line_literal(&visible, q)
            } else {
                Line::from(visible)
            }
        };
        items.push(ListItem::new(line));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }

    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(start)));
    f.render_stateful_widget(list, content, &mut state);

    draw_shell_scrollbar_v(
        f,
        vbar_area,
        scroll_top,
        max_scroll,
        total_lines,
        view_height,
        app.ascii_only,
    );
}

fn draw_shell_inspect_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default()
            .bg(Color::Rgb(24, 24, 30))
            .fg(Color::Rgb(200, 200, 200))
    } else {
        Style::default()
            .bg(Color::Rgb(16, 16, 16))
            .fg(Color::Rgb(200, 200, 200))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let (cur, total) = current_match_pos(app);
    let matches = if app.inspect_query.trim().is_empty() {
        "Matches: -".to_string()
    } else {
        format!("Matches: {cur}/{total}")
    };
    let q = app.inspect_query.trim();
    let path = app
        .inspect_lines
        .get(app.inspect_selected)
        .map(|l| l.path.clone())
        .unwrap_or_else(|| "-".to_string());
    let line = Line::from(vec![
        Span::styled(matches, Style::default().fg(Color::White)),
        Span::raw("   "),
        Span::styled("Query: ", Style::default().fg(Color::Gray)),
        Span::styled(
            if q.is_empty() { "-" } else { q },
            Style::default().fg(Color::White),
        ),
        Span::raw("   "),
        Span::styled("Path: ", Style::default().fg(Color::Gray)),
        Span::styled(
            truncate_end(&path, inner.width.max(1) as usize / 2),
            Style::default().fg(Color::White),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line).style(bg).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shell_help_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = Style::default().bg(Color::Rgb(12, 12, 12)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    let lines = shell_help_lines();
    let total = lines.len().max(1);
    let view_h = inner.height.max(1) as usize;
    let max_scroll = total.saturating_sub(view_h);
    let top = if app.shell_help_scroll == usize::MAX {
        max_scroll
    } else {
        app.shell_help_scroll.min(max_scroll)
    };
    app.shell_help_scroll = top;
    let shown: Vec<Line> = lines.into_iter().skip(top).take(view_h).collect();
    f.render_widget(
        Paragraph::new(shown).style(bg).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_shell_help_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default()
            .bg(Color::Rgb(24, 24, 30))
            .fg(Color::Rgb(200, 200, 200))
    } else {
        Style::default()
            .bg(Color::Rgb(16, 16, 16))
            .fg(Color::Rgb(200, 200, 200))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let hint = "Use Up/Down/PageUp/PageDown to scroll. Press q to return.";
    f.render_widget(
        Paragraph::new(hint)
            .alignment(Alignment::Center)
            .style(bg.fg(Color::Rgb(160, 160, 160)))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn format_session_ts(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn draw_shell_messages_view(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let bg = Style::default().bg(Color::Rgb(12, 12, 12)).fg(Color::White);
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let list_area = cols[0];
    let vbar_area = cols[1];

    let total_msgs = app.session_msgs.len();
    let total = total_msgs.max(1);
    let view_h = list_area.height.max(1) as usize;
    let max_scroll = total.saturating_sub(view_h);
    let w = list_area.width.max(1) as usize;
    let cursor = if total_msgs == 0 {
        0usize
    } else if app.shell_msgs_scroll == usize::MAX {
        total_msgs.saturating_sub(1)
    } else {
        app.shell_msgs_scroll.min(total_msgs.saturating_sub(1))
    };
    if total_msgs > 0 {
        app.shell_msgs_scroll = cursor;
    }
    let top = cursor.saturating_sub(view_h / 2).min(max_scroll);

    // Clamp horizontal scroll to the selected message width.
    if let Some(m) = app.session_msgs.get(cursor) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let ts = format_session_ts(m.at);
        let fixed_len = format!("{ts} {lvl} ").chars().count();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let max_h = m.text.chars().count().saturating_sub(msg_w);
        app.shell_msgs_hscroll = app.shell_msgs_hscroll.min(max_h);
    } else {
        app.shell_msgs_hscroll = 0;
    }

    let mut items: Vec<ListItem> = Vec::new();
    for m in app.session_msgs.iter().skip(top).take(view_h) {
        let lvl = match m.level {
            MsgLevel::Info => "INFO ",
            MsgLevel::Warn => "WARN ",
            MsgLevel::Error => "ERROR",
        };
        let lvl_style = match m.level {
            MsgLevel::Info => Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .bg(Color::Rgb(12, 12, 12)),
            MsgLevel::Warn => Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(12, 12, 12)),
            MsgLevel::Error => Style::default().fg(Color::Red).bg(Color::Rgb(12, 12, 12)),
        };
        let ts = format_session_ts(m.at);
        let ts_style = Style::default()
            .fg(Color::Rgb(120, 120, 120))
            .bg(Color::Rgb(12, 12, 12));
        let fixed = format!("{ts} {lvl} ");
        let fixed_len = fixed.chars().count();
        let msg_w = w.saturating_sub(fixed_len).max(1);
        let msg = window_hscroll(&m.text, app.shell_msgs_hscroll, msg_w);

        let line = Line::from(vec![
            Span::styled(ts, ts_style),
            Span::raw(" "),
            Span::styled(lvl.to_string(), lvl_style),
            Span::raw(" "),
            Span::styled(
                msg,
                Style::default().fg(Color::White).bg(Color::Rgb(12, 12, 12)),
            ),
        ]);
        items.push(ListItem::new(line));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("")));
    }
    let list = List::new(items)
        .style(bg)
        .highlight_style(shell_row_highlight(app))
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(cursor.saturating_sub(top)));
    f.render_stateful_widget(list, list_area, &mut state);

    draw_shell_scrollbar_v(f, vbar_area, top, max_scroll, total, view_h, app.ascii_only);
}

fn window_hscroll(s: &str, start: usize, max: usize) -> String {
    let max = max.max(1);
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max <= 3 {
        let start = start.min(chars.len().saturating_sub(1));
        return chars.into_iter().skip(start).take(max).collect();
    }

    let mut start = start.min(chars.len().saturating_sub(1));
    let show_prefix = start > 0;
    // Reserve space for ellipsis markers.
    let mut avail = max;
    if show_prefix {
        avail = avail.saturating_sub(3);
    }

    let remaining = chars.len().saturating_sub(start);
    let show_suffix = remaining > avail;
    if show_suffix {
        avail = avail.saturating_sub(3);
    }
    if avail == 0 {
        // Fallback: show as much as possible.
        avail = 1;
    }

    // Clamp start so we can fill the window.
    if chars.len() > avail {
        start = start.min(chars.len().saturating_sub(avail));
    } else {
        start = 0;
    }

    let mid: String = chars.iter().copied().skip(start).take(avail).collect();
    let mut out = String::new();
    if show_prefix {
        out.push_str("...");
    }
    out.push_str(&mid);
    if show_suffix {
        out.push_str("...");
    }
    truncate_end(&out, max)
}

fn draw_shell_messages_meta(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let bg = if app.shell_focus == ShellFocus::Details {
        Style::default()
            .bg(Color::Rgb(24, 24, 30))
            .fg(Color::Rgb(200, 200, 200))
    } else {
        Style::default()
            .bg(Color::Rgb(16, 16, 16))
            .fg(Color::Rgb(200, 200, 200))
    };
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let hint = "Up/Down select  Left/Right hscroll  PageUp/PageDown  Home/End  ^c copy  q back";
    f.render_widget(
        Paragraph::new(hint)
            .style(bg.fg(Color::Rgb(160, 160, 160)))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn shell_help_lines() -> Vec<Line<'static>> {
    let h = |title: &str| -> Line<'static> {
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let item = |scope: &str, syntax: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{scope:<10} "),
                Style::default().fg(Color::Rgb(140, 140, 140)),
            ),
            Span::styled(format!("{syntax:<22} "), Style::default().fg(Color::White)),
            Span::styled(
                desc.to_string(),
                Style::default().fg(Color::Rgb(200, 200, 200)),
            ),
        ])
    };

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(h("General"));
    out.push(item("Always", "F1", "Open help"));
    out.push(item("Global", ":q", "Quit (prompts y/n)"));
    out.push(item("Global", ":q!", "Quit immediately (! auto-confirms)"));
    out.push(item("Global", ":! <cmd>", "Run command with auto-confirm (! modifier)"));
    out.push(item(
        "Note",
        "confirm",
        "Destructive commands prompt y/n; add ! to auto-confirm",
    ));
    out.push(item("Global", ":?", "Open help"));
    out.push(item("Global", ":help", "Open help"));
    out.push(item("Global", ":messages", "Toggle messages view (session log)"));
    out.push(item("Global", ":refresh", "Trigger immediate refresh"));
    out.push(item(
        "Global",
        ":sidebar (toggle|compact)",
        "Show/hide sidebar or compact it",
    ));
    out.push(item(
        "Global",
        ":layout [horizontal|vertical|toggle]",
        "Set list/details split for current module",
    ));
    out.push(item(
        "Note",
        "aliases",
        ":ctr, :tpl, :img, :vol, :net (logs has no alias)",
    ));
    out.push(item(
        "Global",
        ":set refresh <sec>",
        "Set refresh interval (1..3600), saved to config",
    ));
    out.push(item(
        "Global",
        ":set logtail <n>",
        "Set docker logs --tail (1..200000), saved to config",
    ));
    out.push(item(
        "Global",
        ":set history <n>",
        "Set command history size (1..5000), saved to config",
    ));
    out.push(Line::from(""));

    out.push(h("Keymap"));
    out.push(item("Note", "^x", "Means Ctrl-x (caret notation)"));
    out.push(item(
        "Keymap",
        "Scopes",
        "always, global, view:<name> (e.g. view:logs)",
    ));
    out.push(item(
        "Keymap",
        "Precedence",
        "always -> view:<current> -> global",
    ));
    out.push(item(
        "Keymap",
        "Disable",
        ":unmap inserts a disable marker that overrides defaults",
    ));
    out.push(item(
        "Global",
        ":map [scope] <KEY> <CMD...>",
        "Bind (e.g. :map always F1 :help, :map view:logs ^l :logs reload)",
    ));
    out.push(item(
        "Global",
        ":unmap [scope] <KEY>",
        "Disable binding or remove override (restore defaults)",
    ));
    out.push(item(
        "Global",
        ":map list",
        "List effective bindings (* = configured/overridden)",
    ));
    out.push(item(
        "Keymap",
        "Safety",
        "Destructive commands cannot be mapped to plain single letters",
    ));
    out.push(Line::from(""));

    out.push(h("Theme"));
    out.push(item("Global", ":theme list", "List available themes"));
    out.push(item("Global", ":theme use <name>", "Switch active theme (persisted)"));
    out.push(item("Global", ":theme new <name>", "Create a new theme from default and open $EDITOR"));
    out.push(item("Global", ":theme edit [name]", "Edit theme file via $EDITOR (creates if missing)"));
    out.push(item("Global", ":theme rm[!] <name>", "Delete theme (! skips confirmation)"));
    out.push(Line::from(""));

    out.push(h("Servers"));
    out.push(item("Global", ":server list", "List configured servers"));
    out.push(item("Global", ":server use <name>", "Switch active server"));
    out.push(item("Global", ":server rm <name>", "Remove server"));
    out.push(item(
        "Global",
        ":server add <name> ssh <target> [-p <port>] [-i <identity>] [--cmd <docker|podman>]",
        "Add SSH server entry",
    ));
    out.push(item(
        "Global",
        ":server add <name> local [--cmd <docker|podman>]",
        "Add local engine entry",
    ));
    out.push(Line::from(""));

    out.push(h("Templates"));
    out.push(item(
        "Templates",
        ":templates kind (stacks|networks|toggle)",
        "Switch between stack templates and network templates",
    ));
    out.push(item("Templates", "^t", "Toggle stacks/networks (default binding)"));
    out.push(item(
        "Templates",
        ":template/:tpl add <name>",
        "Create a new template",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl edit [name]",
        "Edit selected template (or by name)",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl rm [name]",
        "Delete selected template (or by name)",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl deploy [name]",
        "Deploy selected template (or by name) to active server",
    ));
    out.push(Line::from(""));
    out.push(item(
        "Templates",
        ":nettemplate/:nt deploy[!] [name]",
        "Create network on active server (! = recreate if already exists)",
    ));
    out.push(Line::from(""));

    out.push(h("Containers"));
    out.push(item(
        "Containers",
        ":container/:ctr (start|stop|restart|rm)",
        "Run action for selection/marks/stack",
    ));
    out.push(item(
        "Containers",
        ":container/:ctr console [-u USER] [bash|sh|SHELL]",
        "Open console for selected running container (default user: root)",
    ));
    out.push(item("Containers", ":container/:ctr tree", "Toggle stack (tree) view"));
    out.push(Line::from(""));

    out.push(h("Images"));
    out.push(item(
        "Images",
        ":image/:img untag",
        "Remove tag from selected/marked image",
    ));
    out.push(item("Images", ":image/:img rm", "Remove selected/marked image"));
    out.push(Line::from(""));

    out.push(h("Volumes"));
    out.push(item("Volumes", ":volume/:vol rm", "Remove selected/marked volume"));
    out.push(Line::from(""));

    out.push(h("Networks"));
    out.push(item(
        "Networks",
        ":network/:net rm",
        "Remove selected/marked network",
    ));
    out.push(item("Networks", "^d", "Remove (default binding)"));
    out.push(Line::from(""));

    out.push(h("Logs"));
    out.push(item("Logs", "^l", "Reload logs (default binding)"));
    out.push(item("Logs", "^c", "Copy selected lines to clipboard"));
    out.push(item("Logs", "m", "Toggle regex search"));
    out.push(item("Logs", "/", "Enter search mode"));
    out.push(item("Logs", ":", "Enter command mode"));
    out.push(item("Logs", "n/N", "Next/previous match"));
    out.push(item("Logs", "j/k", "Down/up"));
    out.push(item("Logs", "j <n>", "Jump to line n (1-based)"));
    out.push(item("Logs", "save <file>", "Save full logs to a file"));
    out.push(item("Logs", "set number", "Enable line numbers"));
    out.push(item("Logs", "set nonumber", "Disable line numbers"));
    out.push(item("Logs", "set regex", "Enable regex search"));
    out.push(item("Logs", "set noregex", "Disable regex search"));
    out.push(Line::from(""));

    out.push(h("Inspect"));
    out.push(item("Inspect", "/", "Enter search mode"));
    out.push(item("Inspect", ":", "Enter command mode"));
    out.push(item("Inspect", "Enter", "Expand/collapse selected node"));
    out.push(item("Inspect", "n/N", "Next/previous match"));
    out.push(item("Inspect", "expand", "Expand all"));
    out.push(item("Inspect", "collapse", "Collapse all"));
    out.push(item("Inspect", "save <file>", "Save full inspect JSON to a file"));
    out.push(item("Inspect", "y", "Copy selected value (pretty)"));
    out.push(item("Inspect", "p", "Copy selected JSON pointer path"));
    out
}

fn write_text_file(path: &str, text: &str) -> anyhow::Result<PathBuf> {
    let path = path.trim();
    anyhow::ensure!(!path.is_empty(), "missing file path");

    let path = expand_user_path(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&path, text)?;
    Ok(path)
}

fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn shell_escape_sh_arg(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
    {
        return text.to_string();
    }
    let escaped = text.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

fn draw_shell_scrollbar_v(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_top: usize,
    max_scroll: usize,
    total_lines: usize,
    view_height: usize,
    ascii_only: bool,
) {
    if area.height == 0 || total_lines == 0 {
        return;
    }
    let mapped_pos = if max_scroll == 0 || total_lines <= 1 {
        0
    } else {
        (scroll_top.min(max_scroll) * (total_lines - 1)) / max_scroll
    };
    let track = if ascii_only { "|" } else { "│" };
    let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(track))
        .thumb_symbol(track)
        .track_style(Style::default().fg(Color::Rgb(55, 55, 55)))
        .thumb_style(Style::default().fg(Color::White));
    let mut sb_state = ScrollbarState::new(total_lines)
        .position(mapped_pos)
        .viewport_content_length(view_height.max(1));
    f.render_stateful_widget(sb, area, &mut sb_state);
}

fn draw_shell_scrollbar_h(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    scroll_left: usize,
    max_scroll: usize,
    content_width: usize,
    view_width: usize,
    ascii_only: bool,
) {
    if area.height == 0 || area.width == 0 || content_width == 0 {
        return;
    }
    let mapped_pos = if max_scroll == 0 || content_width <= 1 {
        0
    } else {
        (scroll_left.min(max_scroll) * (content_width - 1)) / max_scroll
    };
    let track = if ascii_only { "-" } else { "─" };
    let sb = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some(track))
        .thumb_symbol(track)
        .track_style(Style::default().fg(Color::Rgb(55, 55, 55)))
        .thumb_style(Style::default().fg(Color::White));
    let mut sb_state = ScrollbarState::new(content_width)
        .position(mapped_pos)
        .viewport_content_length(view_width.max(1));
    f.render_stateful_widget(sb, area, &mut sb_state);
}
fn is_container_stopped(status: &str) -> bool {
    let s = status.trim();
    // docker ps STATUS values: "Up ...", "Exited (...) ...", "Created", "Dead"
    !(s.starts_with("Up") || s.starts_with("Restarting"))
}

fn loading_spinner(since: Option<Instant>) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let Some(since) = since else {
        return FRAMES[0];
    };
    let idx = (since.elapsed().as_millis() / 120) as usize % FRAMES.len();
    FRAMES[idx]
}

fn truncate_end(s: &str, max: usize) -> String {
    let max = max.max(1);
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s.chars().take(max).collect();
    }
    let mut out: String = s.chars().take(max - 3).collect();
    out.push_str("...");
    out
}

fn truncate_start(s: &str, max: usize) -> String {
    let max = max.max(1);
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s
            .chars()
            .rev()
            .take(max)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    let tail: String = s
        .chars()
        .rev()
        .take(max - 3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

fn spinner_char(started: Instant, ascii_only: bool) -> char {
    let ms = started.elapsed().as_millis() as u64;
    if ascii_only {
        let frames = ['|', '/', '-', '\\'];
        frames[((ms / 150) % frames.len() as u64) as usize]
    } else {
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        frames[((ms / 120) % frames.len() as u64) as usize]
    }
}
fn stack_name_from_labels(labels: &str) -> Option<String> {
    // docker ps --format exposes labels as a comma-separated "k=v" list.
    // Compose stacks typically set:
    // - com.docker.compose.project=<stack>
    // Swarm stacks often set:
    // - com.docker.stack.namespace=<stack>
    for part in labels.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        if k == "com.docker.compose.project" || k == "com.docker.stack.namespace" {
            return Some(v.to_string());
        }
    }
    None
}

fn action_status_prefix(action: ContainerAction) -> &'static str {
    match action {
        ContainerAction::Start => "Starting...",
        ContainerAction::Stop => "Stopping...",
        ContainerAction::Restart => "Restarting...",
        ContainerAction::Remove => "Removing...",
    }
}
fn slice_window(s: &str, start: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut it = s.chars();
    for _ in 0..start {
        if it.next().is_none() {
            return String::new();
        }
    }
    it.take(width).collect()
}
fn highlight_log_line_regex(line: &str, matcher: Option<&Regex>) -> Line<'static> {
    let Some(re) = matcher else {
        return Line::from(line.to_string());
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(line) {
        let start = m.start();
        let end = m.end();
        if end <= start {
            continue;
        }
        if start > last {
            spans.push(Span::raw(line[last..start].to_string()));
        }
        spans.push(Span::styled(
            line[start..end].to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        last = end;
    }
    if spans.is_empty() {
        return Line::from(line.to_string());
    }
    if last < line.len() {
        spans.push(Span::raw(line[last..].to_string()));
    }
    Line::from(spans)
}

fn highlight_log_line_literal(line: &str, query: &str) -> Line<'static> {
    let q = query.trim();
    if q.is_empty() {
        return Line::from(line.to_string());
    }

    let line_lc = line.to_ascii_lowercase();
    let q_lc = q.to_ascii_lowercase();
    if q_lc.is_empty() {
        return Line::from(line.to_string());
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut start = 0usize;
    while let Some(pos) = line_lc[start..].find(&q_lc) {
        let abs = start + pos;
        if abs > start {
            spans.push(Span::raw(line[start..abs].to_string()));
        }
        let end = abs + q_lc.len();
        spans.push(Span::styled(
            line[abs..end].to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        start = end;
    }
    if spans.is_empty() {
        return Line::from(line.to_string());
    }
    if start < line.len() {
        spans.push(Span::raw(line[start..].to_string()));
    }
    Line::from(spans)
}
fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    // macOS
    if let Ok(()) = pipe_to_cmd("pbcopy", &[], text) {
        return Ok(());
    }
    // Wayland
    if let Ok(()) = pipe_to_cmd("wl-copy", &[], text) {
        return Ok(());
    }
    // X11
    if let Ok(()) = pipe_to_cmd("xclip", &["-selection", "clipboard"], text) {
        return Ok(());
    }

    anyhow::bail!("no clipboard tool found (tried pbcopy, wl-copy, xclip)")
}

fn pipe_to_cmd(cmd: &str, args: &[&str], input: &str) -> anyhow::Result<()> {
    let mut child = StdCommand::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn {}", cmd))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin.write_all(input.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("{} exited with {}", cmd, status);
    }
    Ok(())
}

fn build_inspect_lines(
    root: Option<&Value>,
    expanded: &HashSet<String>,
    match_set: &HashSet<String>,
    query: &str,
) -> Vec<InspectLine> {
    let Some(root) = root else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    let mut buf = String::new();
    build_inspect_lines_inner(
        root,
        expanded,
        match_set,
        "",
        0,
        "$".to_string(),
        &q,
        &mut out,
        &mut buf,
    );
    out
}

fn build_inspect_lines_inner(
    value: &Value,
    expanded: &HashSet<String>,
    match_set: &HashSet<String>,
    path: &str,
    depth: usize,
    label: String,
    query: &str,
    out: &mut Vec<InspectLine>,
    scratch: &mut String,
) {
    let expanded_here = expanded.contains(path);
    let (summary, expandable) = summarize(value);

    scratch.clear();
    let _ = write!(scratch, "{} {} {}", path, label, summary);
    let hay = scratch.to_lowercase();
    let matches = !query.is_empty() && (match_set.contains(path) || hay.contains(query));

    out.push(InspectLine {
        path: path.to_string(),
        depth,
        label,
        summary,
        expandable,
        expanded: expanded_here,
        matches,
    });

    if !(expandable && expanded_here) {
        return;
    }

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child = &map[key];
                let child_path = join_pointer(path, key);
                build_inspect_lines_inner(
                    child,
                    expanded,
                    match_set,
                    &child_path,
                    depth + 1,
                    key.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &idx.to_string());
                build_inspect_lines_inner(
                    child,
                    expanded,
                    match_set,
                    &child_path,
                    depth + 1,
                    idx.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        _ => {}
    }
}

fn summarize(value: &Value) -> (String, bool) {
    match value {
        Value::Null => ("null".to_string(), false),
        Value::Bool(b) => (b.to_string(), false),
        Value::Number(n) => (n.to_string(), false),
        Value::String(s) => (format!("{:?}", s), false),
        Value::Array(arr) => (format!("[{}]", arr.len()), true),
        Value::Object(map) => (format!("{{{}}}", map.len()), true),
    }
}

fn collect_expandable_paths(root: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_expandable_paths_inner(root, "", &mut out);
    out
}

fn collect_expandable_paths_inner(value: &Value, path: &str, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            out.insert(path.to_string());
            for (k, v) in map {
                let p = join_pointer(path, k);
                collect_expandable_paths_inner(v, &p, out);
            }
        }
        Value::Array(arr) => {
            out.insert(path.to_string());
            for (idx, v) in arr.iter().enumerate() {
                let p = join_pointer(path, &idx.to_string());
                collect_expandable_paths_inner(v, &p, out);
            }
        }
        _ => {}
    }
}

fn join_pointer(parent: &str, token: &str) -> String {
    if parent.is_empty() {
        format!("/{}", escape_pointer_token(token))
    } else {
        format!("{}/{}", parent, escape_pointer_token(token))
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn collect_match_paths(root: Option<&Value>, query: &str) -> Vec<String> {
    let Some(root) = root else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut scratch = String::new();
    collect_match_paths_inner(root, "", "$", &q, &mut out, &mut scratch);
    out
}

fn collect_match_paths_inner(
    value: &Value,
    path: &str,
    label: &str,
    query: &str,
    out: &mut Vec<String>,
    scratch: &mut String,
) {
    let (summary, _expandable) = summarize(value);
    scratch.clear();
    let _ = write!(scratch, "{} {} {}", path, label, summary);
    if scratch.to_lowercase().contains(query) {
        out.push(path.to_string());
    }

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = join_pointer(path, key);
                collect_match_paths_inner(&map[key], &child_path, key, query, out, scratch);
            }
        }
        Value::Array(arr) => {
            for (idx, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &idx.to_string());
                collect_match_paths_inner(
                    child,
                    &child_path,
                    &idx.to_string(),
                    query,
                    out,
                    scratch,
                );
            }
        }
        _ => {}
    }
}

fn ancestors_of_pointer(pointer: &str) -> Vec<String> {
    if pointer.is_empty() {
        return vec!["".to_string()];
    }
    let mut out = vec!["".to_string()];
    let mut current = String::new();
    for token in pointer.split('/').skip(1) {
        current.push('/');
        current.push_str(token);
        out.push(current.clone());
    }
    out
}

fn collect_path_rank(root: Option<&Value>) -> HashMap<String, usize> {
    let Some(root) = root else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    let mut idx = 0usize;
    collect_path_rank_inner(root, "", &mut idx, &mut out);
    out
}

fn collect_path_rank_inner(
    value: &Value,
    path: &str,
    idx: &mut usize,
    out: &mut HashMap<String, usize>,
) {
    out.insert(path.to_string(), *idx);
    *idx += 1;

    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child_path = join_pointer(path, key);
                collect_path_rank_inner(&map[key], &child_path, idx, out);
            }
        }
        Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                let child_path = join_pointer(path, &i.to_string());
                collect_path_rank_inner(child, &child_path, idx, out);
            }
        }
        _ => {}
    }
}

fn current_match_pos(app: &App) -> (usize, usize) {
    let total = app.inspect_match_paths.len();
    if total == 0 {
        return (0, 0);
    }
    let path = app
        .inspect_lines
        .get(app.inspect_selected)
        .map(|l| l.path.as_str())
        .unwrap_or("");
    let idx = app
        .inspect_match_paths
        .iter()
        .position(|p| p == path)
        .map(|i| i + 1)
        .unwrap_or(0);
    (idx, total)
}
