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

mod actions;
mod cmd_history;
mod commands;
mod core;
mod features;
mod helpers;
mod input;
mod internal;
mod render;
mod state;
mod text_edit;
mod views;

pub mod theme;
pub use core::run::run_tui;

pub(in crate::ui) use internal::*;

#[cfg(all(test, feature = "integration"))]
#[path = "../tests/ui_integration_tests.rs"]
mod integration_tests;
#[cfg(test)]
#[path = "../tests/ui_tests.rs"]
mod tests;
