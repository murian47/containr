//! Image update state helpers and normalization.
//!
//! This module keeps the UI-side interpretation of image update results isolated from the registry
//! fetching implementation. It is responsible for stable keys, display classification, and local
//! state integration.

use crate::domain::image_refs::{
    NormalizedImageRef, is_digest_only_image, normalize_image_ref_for_updates,
};
use crate::ui::core::types::{ImageUpdateEntry, ImageUpdateKind};
use crate::ui::render::stacks::stack_name_from_labels;
use crate::ui::state::app::App;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(in crate::ui) struct ImageUpdateResult {
    pub image: String,
    pub entry: ImageUpdateEntry,
    pub debug: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageUpdateView {
    Unknown,
    Checking,
    UpToDate,
    UpdateAvailable,
    Error,
    RateLimited,
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

pub(in crate::ui) fn resolve_image_ref_for_updates(
    app: &App,
    image: &str,
) -> Option<NormalizedImageRef> {
    if image.trim().is_empty() {
        return None;
    }
    if is_digest_only_image(image) {
        let needle = normalize_image_id(image);
        for img in &app.images {
            if normalize_image_id(&img.id) == needle
                && let Some(reference) = App::image_row_ref(img)
            {
                return Some(NormalizedImageRef {
                    reference,
                    digest: None,
                });
            }
        }
        return None;
    }
    normalize_image_ref_for_updates(image)
}

pub(in crate::ui) fn resolve_image_update_state(
    app: &App,
    image: &str,
) -> (Option<String>, ImageUpdateView) {
    let normalized = match resolve_image_ref_for_updates(app, image) {
        Some(n) => n,
        None => return (None, ImageUpdateView::Unknown),
    };
    let key = normalized.reference.clone();
    if app.image_updates_inflight.contains(&key) {
        return (Some(key), ImageUpdateView::Checking);
    }
    let Some(entry) = app.image_update_entry(&key) else {
        return (Some(key), ImageUpdateView::Unknown);
    };
    let view = match entry.status {
        ImageUpdateKind::UpToDate => ImageUpdateView::UpToDate,
        ImageUpdateKind::UpdateAvailable => ImageUpdateView::UpdateAvailable,
        ImageUpdateKind::Error => {
            if is_rate_limit_error(entry.error.as_deref()) {
                ImageUpdateView::RateLimited
            } else {
                ImageUpdateView::Error
            }
        }
    };
    (Some(key), view)
}

pub(in crate::ui) fn resolve_stack_update_state(app: &App, stack_name: &str) -> ImageUpdateView {
    let mut has_update = false;
    let mut has_error = false;
    let mut has_unknown = false;
    let mut has_checking = false;
    let mut has_rate_limit = false;
    let mut seen = false;
    for c in app
        .containers
        .iter()
        .filter(|c| stack_name_from_labels(&c.labels).as_deref() == Some(stack_name))
    {
        seen = true;
        let (_, view) = resolve_image_update_state(app, &c.image);
        match view {
            ImageUpdateView::UpdateAvailable => has_update = true,
            ImageUpdateView::Error => has_error = true,
            ImageUpdateView::Unknown => has_unknown = true,
            ImageUpdateView::Checking => has_checking = true,
            ImageUpdateView::RateLimited => has_rate_limit = true,
            ImageUpdateView::UpToDate => {}
        }
    }
    if !seen {
        return ImageUpdateView::Unknown;
    }
    if has_update {
        ImageUpdateView::UpdateAvailable
    } else if has_checking {
        ImageUpdateView::Checking
    } else if has_rate_limit {
        ImageUpdateView::RateLimited
    } else if has_error {
        ImageUpdateView::Error
    } else if has_unknown {
        ImageUpdateView::Unknown
    } else {
        ImageUpdateView::UpToDate
    }
}

pub(in crate::ui) fn is_rate_limit_error(err: Option<&str>) -> bool {
    let Some(err) = err else {
        return false;
    };
    let err = err.to_ascii_lowercase();
    err.contains("toomanyrequests") || err.contains("rate limit") || err.contains("429")
}
