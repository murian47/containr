//! Key binding types and parsing helpers.

use crossterm::event::{KeyCode, KeyModifiers};
use std::collections::HashMap;

use crate::ui::{App, ShellView};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) struct KeySpec {
    pub(in crate::ui) mods: u8, // bitmask: 1=Ctrl 2=Shift 4=Alt
    pub(in crate::ui) code: KeyCodeNorm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum KeyScope {
    Always,
    Global,
    View(ShellView),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum KeyCodeNorm {
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

include!("../keys.inc.rs");
