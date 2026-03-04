use crate::app_meta;
use std::fs;
use std::io::Write;
use std::path::Path;

use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};

pub(in crate::ui) fn template_id_from_labels(labels: &str) -> Option<String> {
    for part in labels.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k.trim() == app_meta::TEMPLATE_LABEL_ID {
            let value = v.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub(in crate::ui) fn template_commit_from_labels(labels: &str) -> Option<String> {
    for part in labels.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k.trim() == app_meta::TEMPLATE_LABEL_COMMIT {
            let value = v.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn add_label_mapping(map: &mut YamlMapping, key: &str, value: &str) {
    let k = YamlValue::String(key.to_string());
    let v = YamlValue::String(value.to_string());
    map.insert(k, v);
}

fn add_label_sequence(seq: &mut Vec<YamlValue>, key: &str, value: &str) {
    let needle = format!("{key}={value}");
    if seq
        .iter()
        .any(|v| v.as_str().map(|s| s == needle).unwrap_or(false))
    {
        return;
    }
    seq.push(YamlValue::String(needle));
}

fn inject_template_labels(
    value: &mut YamlValue,
    template_id: &str,
    template_commit: Option<&str>,
) -> anyhow::Result<()> {
    let obj = value
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("compose root is not a mapping"))?;
    for key in ["services", "networks", "volumes"] {
        let Some(section) = obj.get_mut(YamlValue::String(key.to_string())) else {
            continue;
        };
        let Some(items) = section.as_mapping_mut() else {
            continue;
        };
        for (_, item) in items.iter_mut() {
            let Some(item_map) = item.as_mapping_mut() else {
                continue;
            };
            let label_key = YamlValue::String("labels".to_string());
            if let Some(labels) = item_map.get_mut(&label_key) {
                match labels {
                    YamlValue::Mapping(m) => {
                        add_label_mapping(m, app_meta::TEMPLATE_LABEL_ID, template_id);
                        if let Some(commit) = template_commit {
                            add_label_mapping(m, app_meta::TEMPLATE_LABEL_COMMIT, commit);
                        }
                    }
                    YamlValue::Sequence(seq) => {
                        add_label_sequence(seq, app_meta::TEMPLATE_LABEL_ID, template_id);
                        if let Some(commit) = template_commit {
                            add_label_sequence(seq, app_meta::TEMPLATE_LABEL_COMMIT, commit);
                        }
                    }
                    _ => {
                        let mut m = YamlMapping::new();
                        add_label_mapping(&mut m, app_meta::TEMPLATE_LABEL_ID, template_id);
                        if let Some(commit) = template_commit {
                            add_label_mapping(&mut m, app_meta::TEMPLATE_LABEL_COMMIT, commit);
                        }
                        *labels = YamlValue::Mapping(m);
                    }
                }
            } else {
                let mut m = YamlMapping::new();
                add_label_mapping(&mut m, app_meta::TEMPLATE_LABEL_ID, template_id);
                if let Some(commit) = template_commit {
                    add_label_mapping(&mut m, app_meta::TEMPLATE_LABEL_COMMIT, commit);
                }
                item_map.insert(label_key, YamlValue::Mapping(m));
            }
        }
    }
    Ok(())
}

pub(in crate::ui) fn render_compose_with_template_id(
    path: &Path,
    template_id: &str,
    template_commit: Option<&str>,
) -> anyhow::Result<tempfile::TempPath> {
    let data = fs::read_to_string(path)?;
    let mut yaml: YamlValue =
        serde_yaml::from_str(&data).map_err(|e| anyhow::anyhow!("compose parse failed: {}", e))?;
    inject_template_labels(&mut yaml, template_id, template_commit)?;
    let rendered = serde_yaml::to_string(&yaml)
        .map_err(|e| anyhow::anyhow!("compose render failed: {}", e))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(app_meta::COMPOSE_TEMPFILE_PREFIX)
        .suffix(".yaml")
        .tempfile()?;
    tmp.write_all(rendered.as_bytes())?;
    Ok(tmp.into_temp_path())
}
