use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::{ImageUpdateEntry, RateLimitEntry, RegistryTestEntry, TemplateDeployEntry};

pub(in crate::ui) fn image_updates_path() -> PathBuf {
    if let Ok(root) = std::env::var("XDG_STATE_HOME") {
        let root = PathBuf::from(root);
        return root.join("containr").join("state.json");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("containr")
            .join("state.json");
    }
    PathBuf::from("state.json")
}

pub(in crate::ui) fn load_local_state(
) -> (
    PathBuf,
    HashMap<String, ImageUpdateEntry>,
    HashMap<String, RateLimitEntry>,
    HashMap<String, Vec<TemplateDeployEntry>>,
    HashMap<String, Vec<TemplateDeployEntry>>,
    HashMap<String, RegistryTestEntry>,
) {
    let path = image_updates_path();
    let data = fs::read_to_string(&path).ok();
    let value = data
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    let image_updates = value
        .as_ref()
        .and_then(|v| v.get("image_updates"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let rate_limits = value
        .as_ref()
        .and_then(|v| v.get("rate_limits"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut template_deploys: HashMap<String, Vec<TemplateDeployEntry>> = HashMap::new();
    if let Some(v) = value.as_ref().and_then(|v| v.get("template_deploys")) {
        if let Some(obj) = v.as_object() {
            for (key, entry) in obj {
                if entry.is_array() {
                    if let Ok(list) = serde_json::from_value::<Vec<TemplateDeployEntry>>(entry.clone())
                    {
                        if !list.is_empty() {
                            template_deploys.insert(key.clone(), list);
                        }
                    }
                    continue;
                }
                if let Ok(single) = serde_json::from_value::<TemplateDeployEntry>(entry.clone()) {
                    template_deploys.insert(key.clone(), vec![single]);
                    continue;
                }
                let server_name = entry
                    .get("server_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = entry
                    .get("timestamp")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if !server_name.trim().is_empty() && timestamp > 0 {
                    template_deploys.insert(
                        key.clone(),
                        vec![TemplateDeployEntry {
                            server_name,
                            timestamp,
                            commit: None,
                        }],
                    );
                }
            }
        }
    }
    let mut net_template_deploys: HashMap<String, Vec<TemplateDeployEntry>> = HashMap::new();
    if let Some(v) = value.as_ref().and_then(|v| v.get("net_template_deploys")) {
        if let Some(obj) = v.as_object() {
            for (key, entry) in obj {
                if entry.is_array() {
                    if let Ok(list) = serde_json::from_value::<Vec<TemplateDeployEntry>>(entry.clone())
                    {
                        if !list.is_empty() {
                            net_template_deploys.insert(key.clone(), list);
                        }
                    }
                    continue;
                }
                if let Ok(single) = serde_json::from_value::<TemplateDeployEntry>(entry.clone()) {
                    net_template_deploys.insert(key.clone(), vec![single]);
                    continue;
                }
                let server_name = entry
                    .get("server_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = entry
                    .get("timestamp")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if !server_name.trim().is_empty() && timestamp > 0 {
                    net_template_deploys.insert(
                        key.clone(),
                        vec![TemplateDeployEntry {
                            server_name,
                            timestamp,
                            commit: None,
                        }],
                    );
                }
            }
        }
    }
    let registry_tests = value
        .as_ref()
        .and_then(|v| v.get("registry_tests"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    (
        path,
        image_updates,
        rate_limits,
        template_deploys,
        net_template_deploys,
        registry_tests,
    )
}
