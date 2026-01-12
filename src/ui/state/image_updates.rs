use crate::ui::{App, ImageUpdateKind};
use crate::ui::render::stacks::stack_name_from_labels;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageUpdateView {
    Unknown,
    Checking,
    UpToDate,
    UpdateAvailable,
    Error,
    RateLimited,
}

pub(crate) fn is_digest_only_image(image: &str) -> bool {
    if let Some((_, digest)) = image.split_once('@') {
        return digest.starts_with("sha256:");
    }
    if image.starts_with("sha256:") && !image.contains('/') {
        return image.chars().all(|c| c.is_ascii_hexdigit());
    }
    false
}

pub(crate) fn normalize_image_ref(image: &str) -> String {
    let image = image.trim();
    if image.is_empty() {
        return String::new();
    }
    if is_digest_only_image(image) {
        return image.to_string();
    }
    let (name, digest) = match image.split_once('@') {
        Some((name, digest)) => (name, Some(digest)),
        None => (image, None),
    };
    let (base, tag) = match name.rsplit_once(':') {
        Some((base, tag)) if !tag.contains('/') => (base, Some(tag)),
        _ => (name, None),
    };
    let is_unqualified = !base.contains('/');
    let base = if is_unqualified {
        format!("docker.io/library/{base}")
    } else {
        base.to_string()
    };
    if let Some(digest) = digest {
        return format!("{base}@{digest}");
    }
    let tag = tag.unwrap_or("latest");
    format!("{base}:{tag}")
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedImageRef {
    pub reference: String,
}

pub(crate) fn normalize_image_ref_for_updates(image: &str) -> Option<NormalizedImageRef> {
    if is_digest_only_image(image) {
        return None;
    }
    let normalized = normalize_image_ref(image);
    if normalized.is_empty() {
        return None;
    }
    Some(NormalizedImageRef { reference: normalized })
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

pub(crate) fn resolve_image_ref_for_updates(app: &App, image: &str) -> Option<NormalizedImageRef> {
    if image.trim().is_empty() {
        return None;
    }
    if is_digest_only_image(image) {
        let needle = normalize_image_id(image);
        for img in &app.images {
            if normalize_image_id(&img.id) == needle {
                if let Some(reference) = App::image_row_ref(img) {
                    return Some(NormalizedImageRef { reference });
                }
            }
        }
        return None;
    }
    normalize_image_ref_for_updates(image)
}

pub(crate) fn resolve_image_update_state(
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

pub(crate) fn resolve_stack_update_state(app: &App, stack_name: &str) -> ImageUpdateView {
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

pub(crate) fn is_rate_limit_error(err: Option<&str>) -> bool {
    let Some(err) = err else {
        return false;
    };
    let err = err.to_ascii_lowercase();
    err.contains("toomanyrequests")
        || err.contains("rate limit")
        || err.contains("429")
}

pub(crate) fn image_registry_for_ref(image_ref: &str) -> String {
    let name = image_ref
        .split_once('@')
        .map(|(n, _)| n)
        .unwrap_or(image_ref);
    let name = name.split_once(':').map(|(n, _)| n).unwrap_or(name);
    let first = name.split('/').next().unwrap_or("");
    let has_registry = first.contains('.') || first.contains(':') || first == "localhost";
    if has_registry {
        first.to_string()
    } else {
        "docker.io".to_string()
    }
}
