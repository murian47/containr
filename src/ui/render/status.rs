use crate::ui::render::format::format_action_ts;
use crate::ui::ContainerAction;
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

pub(crate) fn action_status_prefix(action: ContainerAction) -> &'static str {
    match action {
        ContainerAction::Start => "Starting...",
        ContainerAction::Stop => "Stopping...",
        ContainerAction::Restart => "Restarting...",
        ContainerAction::Remove => "Removing...",
    }
}
