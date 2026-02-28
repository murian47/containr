//! Input mode dispatch for modal views.
//!
//! Logs, inspect, and the theme selector all have mode-specific editing/search behavior. This
//! module decides whether a key press should be consumed by one of those modal handlers.

mod inspect;
mod logs;
mod theme_selector;

use super::context::InputCtx;
use crate::ui::state::app::App;
use crossterm::event::KeyEvent;

pub(super) fn handle_view_input_modes(app: &mut App, key: KeyEvent, ctx: &InputCtx<'_>) -> bool {
    if logs::handle_logs_mode(app, key, ctx) {
        return true;
    }
    if inspect::handle_inspect_mode(app, key) {
        return true;
    }
    theme_selector::handle_theme_selector_mode(app, key)
}
