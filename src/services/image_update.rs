use crate::docker::{self, DockerCfg};
use crate::domain::image_refs::{
    NormalizedImageRef, image_repo_name, local_repo_digest, normalize_image_ref_for_updates,
};
use crate::runner::Runner;
use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct ImageInspect {
    #[serde(rename = "RepoDigests")]
    repo_digests: Option<Vec<String>>,
    #[serde(rename = "Architecture")]
    architecture: Option<String>,
    #[serde(rename = "Os")]
    os: Option<String>,
}

#[derive(Debug, Serialize)]
enum ImageUpdateKindPayload {
    UpToDate,
    UpdateAvailable,
    Error,
}

#[derive(Debug, Serialize)]
struct ImageUpdateEntryPayload {
    checked_at: i64,
    status: ImageUpdateKindPayload,
    local_digest: Option<String>,
    remote_digest: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ImageUpdateResultPayload {
    image: String,
    entry: ImageUpdateEntryPayload,
    debug: Option<String>,
}

pub struct ImageUpdateService<'a> {
    runner: &'a Runner,
    docker: &'a DockerCfg,
    debug: bool,
}

impl<'a> ImageUpdateService<'a> {
    pub fn new(runner: &'a Runner, docker: &'a DockerCfg, debug: bool) -> Self {
        Self {
            runner,
            docker,
            debug,
        }
    }

    pub async fn check_image_update(&self, image: &str) -> anyhow::Result<String> {
        if self.docker.docker_cmd.is_empty() {
            anyhow::bail!("no server configured");
        }
        let normalized = normalize_image_ref_for_updates(image)
            .ok_or_else(|| anyhow::anyhow!("invalid image reference"))?;
        let repo = image_repo_name(&normalized.reference);

        let inspect_raw =
            docker::fetch_image_inspect(self.runner, self.docker, &normalized.reference).await?;
        let inspect: ImageInspect =
            serde_json::from_str(&inspect_raw).context("image inspect output was not JSON")?;

        let repo_digests_len = inspect.repo_digests.as_ref().map(|v| v.len()).unwrap_or(0);
        let repo_digests_preview = match inspect.repo_digests.as_ref() {
            Some(list) => {
                let mut parts: Vec<String> = Vec::new();
                for item in list.iter().take(3) {
                    parts.push(item.clone());
                }
                if list.len() > 3 {
                    parts.push("...".to_string());
                }
                format!("[{}]", parts.join(", "))
            }
            None => "none".to_string(),
        };
        let local_digest = inspect
            .repo_digests
            .as_deref()
            .and_then(|list| local_repo_digest(list, &repo));

        let (status, remote_digest, error, debug_remote, debug_remote_digests, debug_local_index) =
            if let Some(digest) = normalized.digest.clone() {
                (
                    ImageUpdateKindPayload::UpToDate,
                    Some(digest),
                    None::<String>,
                    None::<String>,
                    None::<String>,
                    None::<String>,
                )
            } else {
                self.compare_remote(
                    &normalized,
                    &repo,
                    &inspect,
                    local_digest.clone(),
                    repo_digests_len,
                    &repo_digests_preview,
                )
                .await?
            };

        let debug_info = if self.debug {
            let arch = inspect.architecture.as_deref().unwrap_or("-");
            let os = inspect.os.as_deref().unwrap_or("-");
            let local = local_digest.as_deref().unwrap_or("-");
            let remote = remote_digest.as_deref().unwrap_or("-");
            let mut parts = vec![
                format!("image={}", normalized.reference),
                format!("repo={repo}"),
                format!("inspect_platform={arch}/{os}"),
                format!("local_digest={local}"),
                format!("remote_digest={remote}"),
                format!("repo_digests={repo_digests_len} {repo_digests_preview}"),
            ];
            if let Some(summary) = debug_remote.as_deref() {
                parts.push(format!("remote_platforms={summary}"));
            }
            if let Some(list) = debug_remote_digests.as_deref() {
                parts.push(format!("remote_digests={list}"));
            }
            if let Some(idx) = debug_local_index.as_deref() {
                parts.push(idx.to_string());
            }
            Some(parts.join(" | "))
        } else {
            None
        };

        let entry = ImageUpdateEntryPayload {
            checked_at: now_unix(),
            status,
            local_digest: local_digest.clone(),
            remote_digest: remote_digest.clone(),
            error: error.clone(),
        };
        let result = ImageUpdateResultPayload {
            image: normalized.reference.clone(),
            entry,
            debug: debug_info,
        };
        Ok(serde_json::to_string(&result)?)
    }

    #[allow(clippy::type_complexity)]
    async fn compare_remote(
        &self,
        normalized: &NormalizedImageRef,
        repo: &str,
        inspect: &ImageInspect,
        local_digest: Option<String>,
        repo_digests_len: usize,
        repo_digests_preview: &str,
    ) -> anyhow::Result<(
        ImageUpdateKindPayload,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> {
        let remote_fetch =
            docker::fetch_manifest_inspect(self.runner, self.docker, &normalized.reference).await;
        let (remote, summary, remote_digests) = match remote_fetch {
            Ok(raw) => {
                let summary = manifest_platform_summary(&raw);
                let remote_digests = manifest_remote_digests(&raw);
                let remote = if let (Some(arch), Some(os)) =
                    (inspect.architecture.as_deref(), inspect.os.as_deref())
                {
                    manifest_digest_for_platform(&raw, arch, os)
                        .or_else(|| manifest_descriptor_digest(&raw))
                } else {
                    manifest_descriptor_digest(&raw)
                };
                (remote, summary, remote_digests)
            }
            Err(e) => {
                let err = truncate_msg(&format!("{:#}", e), 200);
                return Ok((
                    ImageUpdateKindPayload::Error,
                    None,
                    Some(err),
                    None,
                    None,
                    None,
                ));
            }
        };

        let debug_remote = Some(summary.clone());
        let debug_remote_digests = Some(format_remote_digest_list(&remote_digests));

        match (local_digest, remote.clone()) {
            (Some(local), Some(remote_digest)) => {
                let inspect_arch = inspect.architecture.as_deref().unwrap_or("").to_string();
                let inspect_os = inspect.os.as_deref().unwrap_or("").to_string();
                let remote_matches_local = remote_digests.iter().any(|(d, arch, os)| {
                    if d != &local {
                        return false;
                    }
                    let arch = arch.as_deref().unwrap_or("");
                    let os = os.as_deref().unwrap_or("");
                    if arch.is_empty() || os.is_empty() {
                        return true;
                    }
                    if arch == "unknown" || os == "unknown" {
                        return true;
                    }
                    arch == inspect_arch && os == inspect_os
                });
                if remote_matches_local {
                    Ok((
                        ImageUpdateKindPayload::UpToDate,
                        Some(remote_digest),
                        None,
                        debug_remote,
                        debug_remote_digests,
                        None,
                    ))
                } else {
                    let idx_ref = format!("{}@{}", normalized.reference, local);
                    match docker::fetch_manifest_inspect(self.runner, self.docker, &idx_ref).await {
                        Ok(idx_raw) => {
                            let idx_digest = if !inspect_arch.is_empty() && !inspect_os.is_empty() {
                                manifest_digest_for_platform(&idx_raw, &inspect_arch, &inspect_os)
                                    .or_else(|| manifest_descriptor_digest(&idx_raw))
                            } else {
                                manifest_descriptor_digest(&idx_raw)
                            };
                            let idx_summary = manifest_platform_summary(&idx_raw);
                            let idx_debug = Some(format!(
                                "local_index_ref={idx_ref} local_index_platforms={idx_summary} local_index_digest={}",
                                idx_digest.as_deref().unwrap_or("-")
                            ));
                            if let Some(idx_digest) = idx_digest {
                                if idx_digest == remote_digest {
                                    Ok((
                                        ImageUpdateKindPayload::UpToDate,
                                        Some(remote_digest),
                                        None,
                                        debug_remote,
                                        debug_remote_digests,
                                        idx_debug,
                                    ))
                                } else {
                                    Ok((
                                        ImageUpdateKindPayload::UpdateAvailable,
                                        Some(remote_digest),
                                        None,
                                        debug_remote,
                                        debug_remote_digests,
                                        idx_debug,
                                    ))
                                }
                            } else {
                                Ok((
                                    ImageUpdateKindPayload::UpdateAvailable,
                                    Some(remote_digest),
                                    None,
                                    debug_remote,
                                    debug_remote_digests,
                                    idx_debug,
                                ))
                            }
                        }
                        Err(_) => Ok((
                            ImageUpdateKindPayload::UpdateAvailable,
                            Some(remote_digest),
                            None,
                            debug_remote,
                            debug_remote_digests,
                            None,
                        )),
                    }
                }
            }
            (None, _) => Ok((
                ImageUpdateKindPayload::Error,
                None,
                Some(format!(
                    "missing local digest (repo={repo}, repo_digests={repo_digests_len} {repo_digests_preview})"
                )),
                debug_remote,
                debug_remote_digests,
                None,
            )),
            (_, None) => Ok((
                ImageUpdateKindPayload::Error,
                None,
                Some(format!("missing remote digest (platforms={summary})")),
                debug_remote,
                debug_remote_digests,
                None,
            )),
        }
    }
}

fn parse_manifest_json(raw: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        return Some(value);
    }

    let starts: Vec<usize> = raw
        .char_indices()
        .filter_map(|(idx, ch)| matches!(ch, '{' | '[').then_some(idx))
        .collect();
    let ends: Vec<usize> = raw
        .char_indices()
        .filter_map(|(idx, ch)| matches!(ch, '}' | ']').then_some(idx + ch.len_utf8()))
        .collect();

    for start in starts {
        for end in ends.iter().rev().copied() {
            if end <= start {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&raw[start..end]) {
                return Some(value);
            }
        }
    }
    None
}

fn manifest_entries(raw: &str) -> Vec<Value> {
    parse_manifest_json(raw)
        .and_then(|val| match val {
            Value::Object(obj) => {
                if let Some(manifests) = get_ci(&obj, "manifests").and_then(|v| v.as_array()) {
                    Some(manifests.clone())
                } else {
                    Some(vec![Value::Object(obj)])
                }
            }
            Value::Array(items) => Some(items),
            _ => None,
        })
        .unwrap_or_default()
}

fn get_ci<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    map.get(key)
        .or_else(|| map.get(&key.to_ascii_lowercase()))
        .or_else(|| map.get(&key.to_ascii_uppercase()))
        .or_else(|| map.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v))
}

fn entry_descriptor_digest(obj: &Value) -> Option<String> {
    let Value::Object(obj) = obj else {
        return None;
    };
    get_ci(obj, "digest")
        .and_then(|v| v.as_str())
        .or_else(|| {
            get_ci(obj, "descriptor")
                .and_then(|v| v.as_object())
                .and_then(|descriptor| get_ci(descriptor, "digest"))
                .and_then(|v| v.as_str())
        })
        .map(|s| s.to_string())
}

fn entry_platform(obj: &Value) -> (Option<String>, Option<String>) {
    let Value::Object(obj) = obj else {
        return (None, None);
    };
    let platform = get_ci(obj, "platform")
        .and_then(|v| v.as_object())
        .or_else(|| {
            get_ci(obj, "descriptor")
                .and_then(|v| v.as_object())
                .and_then(|descriptor| get_ci(descriptor, "platform"))
                .and_then(|v| v.as_object())
        })
        .or_else(|| get_ci(obj, "Platform").and_then(|v| v.as_object()));
    let Some(platform) = platform else {
        return (None, None);
    };
    let arch = get_ci(platform, "architecture")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let os = get_ci(platform, "os")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (arch, os)
}

fn manifest_descriptor_digest(raw: &str) -> Option<String> {
    let entries = manifest_entries(raw);
    entries.iter().find_map(entry_descriptor_digest)
}

fn manifest_digest_for_platform(raw: &str, arch: &str, os: &str) -> Option<String> {
    let entries = manifest_entries(raw);
    let mut fallback: Option<String> = None;
    for entry in entries {
        let digest = entry_descriptor_digest(&entry);
        let (p_arch, p_os) = entry_platform(&entry);
        if let (Some(p_arch), Some(p_os), Some(digest)) = (p_arch, p_os, digest) {
            if p_arch == arch && p_os == os {
                return Some(digest);
            }
            if fallback.is_none()
                && p_arch != "unknown"
                && p_os != "unknown"
                && !p_arch.is_empty()
                && !p_os.is_empty()
            {
                fallback = Some(digest);
            }
        }
    }
    fallback
}

fn manifest_platform_summary(raw: &str) -> String {
    let entries = manifest_entries(raw);
    if entries.is_empty() {
        return "none".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for entry in entries {
        let (arch, os) = entry_platform(&entry);
        let arch = arch.as_deref().unwrap_or("?");
        let os = os.as_deref().unwrap_or("?");
        parts.push(format!("{arch}/{os}"));
    }
    parts.join(",")
}

fn manifest_remote_digests(raw: &str) -> Vec<(String, Option<String>, Option<String>)> {
    manifest_entries(raw)
        .into_iter()
        .filter_map(|entry| {
            let digest = entry_descriptor_digest(&entry)?;
            let (arch, os) = entry_platform(&entry);
            Some((digest, arch, os))
        })
        .collect()
}

fn format_remote_digest_list(items: &[(String, Option<String>, Option<String>)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (digest, arch, os) in items {
        let arch = arch.as_deref().unwrap_or("?");
        let os = os.as_deref().unwrap_or("?");
        parts.push(format!("{digest}@{arch}/{os}"));
    }
    parts.join(",")
}

fn truncate_msg(msg: &str, max: usize) -> String {
    if msg.len() <= max {
        msg.to_string()
    } else if max <= 3 {
        msg.chars().take(max).collect()
    } else {
        let mut out: String = msg.chars().take(max - 3).collect();
        out.push_str("...");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{
        manifest_descriptor_digest, manifest_digest_for_platform, manifest_platform_summary,
        manifest_remote_digests, parse_manifest_json,
    };

    #[test]
    fn manifest_parser_supports_single_manifest_verbose_output() {
        let raw = r#"{
  "Ref": "docker.io/library/redis:7-alpine",
  "Descriptor": {
    "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
    "digest": "sha256:remote-single",
    "size": 1234,
    "platform": {
      "architecture": "amd64",
      "os": "linux"
    }
  }
}"#;

        assert_eq!(
            manifest_descriptor_digest(raw).as_deref(),
            Some("sha256:remote-single")
        );
        assert_eq!(
            manifest_digest_for_platform(raw, "amd64", "linux").as_deref(),
            Some("sha256:remote-single")
        );
        assert_eq!(manifest_platform_summary(raw), "amd64/linux");
        assert_eq!(manifest_remote_digests(raw).len(), 1);
    }

    #[test]
    fn manifest_parser_supports_manifest_list_output() {
        let raw = r#"{
  "manifests": [
    {
      "digest": "sha256:amd64",
      "platform": {
        "architecture": "amd64",
        "os": "linux"
      }
    },
    {
      "digest": "sha256:arm64",
      "platform": {
        "architecture": "arm64",
        "os": "linux"
      }
    }
  ]
}"#;

        assert_eq!(
            manifest_digest_for_platform(raw, "arm64", "linux").as_deref(),
            Some("sha256:arm64")
        );
        assert_eq!(manifest_platform_summary(raw), "amd64/linux,arm64/linux");
        assert_eq!(manifest_remote_digests(raw).len(), 2);
    }

    #[test]
    fn manifest_parser_extracts_json_from_wrapped_output() {
        let raw = r#"warning: legacy output
{
  "Descriptor": {
    "digest": "sha256:wrapped",
    "platform": {
      "architecture": "amd64",
      "os": "linux"
    }
  }
}
"#;

        let parsed = parse_manifest_json(raw);
        assert!(parsed.is_some());
        assert_eq!(
            manifest_digest_for_platform(raw, "amd64", "linux").as_deref(),
            Some("sha256:wrapped")
        );
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
