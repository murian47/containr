use crate::ui::render::format::format_action_ts;
use crate::ui::{ActionErrorKind, LastActionError};

pub(crate) fn action_error_label(err: &LastActionError) -> &'static str {
    match err.kind {
        ActionErrorKind::InUse => "in use",
        ActionErrorKind::Other => "error",
    }
}

pub(crate) fn action_error_details(err: &LastActionError) -> String {
    let ts = format_action_ts(err.at);
    if err.action.trim().is_empty() {
        ts
    } else {
        format!("{} {}", err.action, ts)
    }
}
