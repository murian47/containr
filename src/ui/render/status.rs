use crate::docker::ContainerAction;
use crate::ui::core::types::{ActionErrorKind, LastActionError};
use crate::ui::render::format::format_action_ts;
use crate::ui::state::app::App;
use crate::ui::state::image_updates::ImageUpdateView;
use ratatui::style::Style;

pub(in crate::ui) fn action_error_label(err: &LastActionError) -> &'static str {
    match err.kind {
        ActionErrorKind::InUse => "in use",
        ActionErrorKind::Other => "error",
    }
}

pub(in crate::ui) fn action_error_details(err: &LastActionError) -> String {
    let ts = format_action_ts(err.at);
    if err.action.trim().is_empty() {
        ts
    } else {
        format!("{} {}", err.action, ts)
    }
}

pub(in crate::ui) fn action_status_prefix(action: ContainerAction) -> &'static str {
    match action {
        ContainerAction::Start => "Starting...",
        ContainerAction::Stop => "Stopping...",
        ContainerAction::Restart => "Restarting...",
        ContainerAction::Remove => "Removing...",
    }
}

pub(in crate::ui) fn image_update_indicator(
    app: &App,
    view: ImageUpdateView,
    bg: Style,
) -> (String, Style) {
    let (text, style) = match view {
        ImageUpdateView::UpToDate => (
            if app.ascii_only { "Y" } else { "●" },
            bg.patch(app.theme.text_ok.to_style()),
        ),
        ImageUpdateView::UpdateAvailable => (
            if app.ascii_only { "U" } else { "●" },
            bg.patch(app.theme.text_warn.to_style()),
        ),
        ImageUpdateView::Error => (
            if app.ascii_only { "!" } else { "●" },
            bg.patch(app.theme.text_error.to_style()),
        ),
        ImageUpdateView::RateLimited => (
            if app.ascii_only { "i" } else { "●" },
            bg.patch(app.theme.text_info.to_style()),
        ),
        ImageUpdateView::Checking => (
            if app.ascii_only { "*" } else { "⟳" },
            bg.patch(app.theme.text_warn.to_style()),
        ),
        ImageUpdateView::Unknown => (
            if app.ascii_only { "?" } else { "·" },
            bg.patch(app.theme.text_dim.to_style()),
        ),
    };
    (text.to_string(), style)
}
