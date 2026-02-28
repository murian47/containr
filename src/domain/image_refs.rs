#[derive(Clone, Debug)]
pub struct NormalizedImageRef {
    pub reference: String,
    pub digest: Option<String>,
}

pub fn is_digest_only_image(image: &str) -> bool {
    if let Some((_, digest)) = image.split_once('@') {
        return digest.starts_with("sha256:");
    }
    if image.starts_with("sha256:") && !image.contains('/') {
        return image.chars().all(|c| c.is_ascii_hexdigit());
    }
    false
}

pub fn normalize_image_ref(image: &str) -> String {
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

pub fn normalize_image_ref_for_updates(image: &str) -> Option<NormalizedImageRef> {
    if is_digest_only_image(image) {
        return None;
    }
    let normalized = normalize_image_ref(image);
    if normalized.is_empty() {
        return None;
    }
    let digest = normalized.split_once('@').map(|(_, d)| d.to_string());
    Some(NormalizedImageRef {
        reference: normalized,
        digest,
    })
}

pub fn image_registry_for_ref(image_ref: &str) -> String {
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

fn normalize_docker_hub_repo(name: &str) -> String {
    let mut name = name.trim().to_string();
    if let Some(rest) = name.strip_prefix("docker.io/") {
        name = rest.to_string();
    }
    if !name.contains('/') {
        name = format!("library/{name}");
    }
    name
}

pub fn local_repo_digest(repo_digests: &[String], repo: &str) -> Option<String> {
    let repo_docker_hub = normalize_docker_hub_repo(repo);
    for entry in repo_digests {
        let (name, digest) = entry.split_once('@')?;
        if name == repo || name == repo_docker_hub {
            return Some(digest.to_string());
        }
        let name_docker_hub = normalize_docker_hub_repo(name);
        if name_docker_hub == repo_docker_hub {
            return Some(digest.to_string());
        }
    }
    None
}

pub fn image_repo_name(image_ref: &str) -> String {
    let name = image_ref
        .split_once('@')
        .map(|(n, _)| n)
        .unwrap_or(image_ref);
    match name.rsplit_once(':') {
        Some((base, tag)) if !tag.contains('/') => base.to_string(),
        _ => name.to_string(),
    }
}
