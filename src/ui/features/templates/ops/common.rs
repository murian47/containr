use crate::ui::NetworkTemplateIpv4;
use anyhow::Context;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspect {
    #[serde(rename = "Name")]
    pub(super) name: String,
    #[serde(rename = "Config")]
    pub(super) config: Option<ContainerInspectConfig>,
    #[serde(rename = "HostConfig")]
    pub(super) host_config: Option<ContainerInspectHostConfig>,
    #[serde(rename = "NetworkSettings")]
    pub(super) network_settings: Option<ContainerInspectNetworkSettings>,
    #[serde(rename = "Mounts")]
    pub(super) mounts: Option<Vec<ContainerInspectMount>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectConfig {
    #[serde(rename = "Image")]
    pub(super) image: Option<String>,
    #[serde(rename = "Env")]
    pub(super) env: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    pub(super) cmd: Option<Vec<String>>,
    #[serde(rename = "Entrypoint")]
    pub(super) entrypoint: Option<Vec<String>>,
    #[serde(rename = "Labels")]
    pub(super) labels: Option<HashMap<String, String>>,
    #[serde(rename = "WorkingDir")]
    pub(super) working_dir: Option<String>,
    #[serde(rename = "User")]
    pub(super) user: Option<String>,
    #[serde(rename = "ExposedPorts")]
    pub(super) exposed_ports: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "Healthcheck")]
    pub(super) healthcheck: Option<ContainerInspectHealthcheck>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectHealthcheck {
    #[serde(rename = "Test")]
    pub(super) test: Option<Vec<String>>,
    #[serde(rename = "Interval")]
    pub(super) interval: Option<i64>,
    #[serde(rename = "Timeout")]
    pub(super) timeout: Option<i64>,
    #[serde(rename = "Retries")]
    pub(super) retries: Option<i64>,
    #[serde(rename = "StartPeriod")]
    pub(super) start_period: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectHostConfig {
    #[serde(rename = "RestartPolicy")]
    pub(super) restart_policy: Option<ContainerInspectRestartPolicy>,
    #[serde(rename = "PortBindings")]
    pub(super) port_bindings: Option<HashMap<String, Vec<ContainerInspectPortBinding>>>,
    #[serde(rename = "ReadonlyRootfs")]
    pub(super) readonly_rootfs: Option<bool>,
    #[serde(rename = "Privileged")]
    pub(super) privileged: Option<bool>,
    #[serde(rename = "ExtraHosts")]
    pub(super) extra_hosts: Option<Vec<String>>,
    #[serde(rename = "NetworkMode")]
    pub(super) network_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectRestartPolicy {
    #[serde(rename = "Name")]
    pub(super) name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectPortBinding {
    #[serde(rename = "HostIp")]
    pub(super) host_ip: Option<String>,
    #[serde(rename = "HostPort")]
    pub(super) host_port: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectNetworkSettings {
    #[serde(rename = "Networks")]
    pub(super) networks: Option<HashMap<String, ContainerInspectNetworkAttachment>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectNetworkAttachment {
    #[serde(rename = "Aliases")]
    pub(super) aliases: Option<Vec<String>>,
    #[serde(rename = "IPAddress")]
    pub(super) ip_address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContainerInspectMount {
    #[serde(rename = "Type")]
    pub(super) kind: Option<String>,
    #[serde(rename = "Name")]
    pub(super) name: Option<String>,
    #[serde(rename = "Source")]
    pub(super) source: Option<String>,
    #[serde(rename = "Destination")]
    pub(super) destination: Option<String>,
    #[serde(rename = "Driver")]
    pub(super) driver: Option<String>,
    #[serde(rename = "ReadOnly")]
    pub(super) read_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct NetworkInspect {
    #[serde(rename = "Name")]
    pub(super) name: String,
    #[serde(rename = "Driver")]
    pub(super) driver: Option<String>,
    #[serde(rename = "Internal")]
    pub(super) internal: Option<bool>,
    #[serde(rename = "Attachable")]
    pub(super) attachable: Option<bool>,
    #[serde(rename = "EnableIPv6")]
    pub(super) enable_ipv6: Option<bool>,
    #[serde(rename = "Options")]
    pub(super) options: Option<HashMap<String, String>>,
    #[serde(rename = "Labels")]
    pub(super) labels: Option<HashMap<String, String>>,
    #[serde(rename = "IPAM")]
    pub(super) ipam: Option<NetworkInspectIpam>,
}

#[derive(Debug, Deserialize)]
pub(super) struct NetworkInspectIpam {
    #[serde(rename = "Driver")]
    pub(super) driver: Option<String>,
    #[serde(rename = "Config")]
    pub(super) config: Option<Vec<NetworkInspectIpamConfig>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct NetworkInspectIpamConfig {
    #[serde(rename = "Subnet")]
    pub(super) subnet: Option<String>,
    #[serde(rename = "Gateway")]
    pub(super) gateway: Option<String>,
    #[serde(rename = "IPRange")]
    pub(super) ip_range: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ComposeService {
    pub(super) image: String,
    pub(super) container_name: Option<String>,
    pub(super) command: Vec<String>,
    pub(super) entrypoint: Vec<String>,
    pub(super) environment: Vec<String>,
    pub(super) ports: Vec<String>,
    pub(super) expose: Vec<String>,
    pub(super) volumes: Vec<String>,
    pub(super) tmpfs: Vec<String>,
    pub(super) networks: BTreeMap<String, ComposeServiceNetwork>,
    pub(super) labels: BTreeMap<String, String>,
    pub(super) restart: Option<String>,
    pub(super) working_dir: Option<String>,
    pub(super) user: Option<String>,
    pub(super) privileged: Option<bool>,
    pub(super) read_only: Option<bool>,
    pub(super) extra_hosts: Vec<String>,
    pub(super) healthcheck: Option<ComposeHealthcheck>,
    pub(super) network_mode: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ComposeServiceNetwork {
    pub(super) aliases: Vec<String>,
    pub(super) ipv4_address: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ComposeHealthcheck {
    pub(super) test: Vec<String>,
    pub(super) interval: Option<String>,
    pub(super) timeout: Option<String>,
    pub(super) retries: Option<i64>,
    pub(super) start_period: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ComposeNetwork {
    pub(super) name: String,
    pub(super) driver: Option<String>,
    pub(super) internal: Option<bool>,
    pub(super) attachable: Option<bool>,
    pub(super) enable_ipv6: Option<bool>,
    pub(super) ipam: Option<ComposeNetworkIpam>,
    pub(super) options: BTreeMap<String, String>,
    pub(super) labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(super) struct ComposeNetworkIpam {
    pub(super) driver: Option<String>,
    pub(super) config: Vec<ComposeNetworkIpamConfig>,
}

#[derive(Clone, Debug)]
pub(super) struct ComposeNetworkIpamConfig {
    pub(super) subnet: Option<String>,
    pub(super) gateway: Option<String>,
    pub(super) ip_range: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ComposeVolume {
    pub(super) driver: Option<String>,
}

pub(super) fn is_system_network_name(name: &str) -> bool {
    matches!(
        name,
        "bridge" | "host" | "none" | "ingress" | "docker_gwbridge"
    )
}

pub(super) fn stack_name_from_label_map(labels: &HashMap<String, String>) -> Option<String> {
    labels
        .get("com.docker.compose.project")
        .or_else(|| labels.get("com.docker.stack.namespace"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub(super) fn service_name_from_labels(
    labels: &HashMap<String, String>,
    stack_name: Option<&str>,
    container_name: &str,
) -> String {
    if let Some(name) = labels.get("com.docker.compose.service") {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    if let Some(name) = labels.get("com.docker.swarm.service.name") {
        let name = name.trim();
        if !name.is_empty() {
            if let Some(stack) = stack_name {
                for sep in ['_', '-', '.'] {
                    let prefix = format!("{stack}{sep}");
                    if name.starts_with(&prefix) {
                        return name[prefix.len()..].to_string();
                    }
                }
            }
            return name.to_string();
        }
    }
    let mut name = container_name.trim().trim_start_matches('/').to_string();
    if let Some(stack) = stack_name {
        for sep in ['_', '-', '.'] {
            let prefix = format!("{stack}{sep}");
            if name.starts_with(&prefix) {
                name = name[prefix.len()..].to_string();
                break;
            }
        }
    }
    if name.is_empty() {
        "service".to_string()
    } else {
        name
    }
}

pub(super) fn sanitize_compose_key(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("item");
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub(super) fn unique_compose_key(name: &str, used: &mut HashSet<String>) -> String {
    let base = sanitize_compose_key(name);
    if used.insert(base.clone()) {
        return base;
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{base}_{idx}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        idx = idx.saturating_add(1);
    }
}

pub(super) fn yaml_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(super) fn format_duration_ns(ns: i64) -> Option<String> {
    if ns <= 0 {
        return None;
    }
    const NS_PER_S: i64 = 1_000_000_000;
    const NS_PER_MS: i64 = 1_000_000;
    const NS_PER_US: i64 = 1_000;
    if ns % NS_PER_S == 0 {
        Some(format!("{}s", ns / NS_PER_S))
    } else if ns % NS_PER_MS == 0 {
        Some(format!("{}ms", ns / NS_PER_MS))
    } else if ns % NS_PER_US == 0 {
        Some(format!("{}us", ns / NS_PER_US))
    } else {
        Some(format!("{}ns", ns))
    }
}

pub(super) fn filter_labels(labels: &HashMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in labels {
        if k.starts_with("com.docker.compose.")
            || k.starts_with("com.docker.stack.")
            || k.starts_with("com.docker.swarm.")
        {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    out
}

pub(super) fn write_stack_template_compose(
    templates_dir: &PathBuf,
    name: &str,
    compose: &str,
) -> anyhow::Result<PathBuf> {
    validate_template_name(name)?;

    fs::create_dir_all(templates_dir)?;
    let dir = templates_dir.join(name.trim());
    if dir.exists() && !dir.is_dir() {
        anyhow::bail!(
            "template path exists but is not a directory: {}",
            dir.display()
        );
    }
    fs::create_dir_all(&dir)?;
    let compose_path = dir.join("compose.yaml");
    fs::write(&compose_path, compose)?;
    Ok(compose_path)
}

pub(super) fn write_net_template_cfg(
    templates_dir: &PathBuf,
    name: &str,
    cfg: &str,
) -> anyhow::Result<PathBuf> {
    validate_template_name(name)?;

    fs::create_dir_all(templates_dir)?;
    let dir = templates_dir.join(name.trim());
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;
    let cfg_path = dir.join("network.json");
    fs::write(&cfg_path, cfg)?;
    Ok(cfg_path)
}

pub(super) fn validate_template_name(name: &str) -> anyhow::Result<()> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "template name is empty");
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "template name must be [A-Za-z0-9._-]"
    );
    anyhow::ensure!(
        !name.starts_with('.'),
        "template name must not start with '.'"
    );
    anyhow::ensure!(name != "." && name != "..", "invalid template name");
    Ok(())
}

pub(super) fn hc_interval(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.interval)
}

pub(super) fn hc_timeout(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.timeout)
}

pub(super) fn hc_start_period(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.start_period)
}

pub(super) fn hc_retries(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.retries)
}

pub(super) fn hc_duration(value: Option<i64>) -> Option<String> {
    value.and_then(format_duration_ns)
}

#[derive(serde::Serialize)]
pub(super) struct NetworkTemplateSpecWrite {
    #[serde(default)]
    pub(super) description: Option<String>,
    pub(super) name: String,
    #[serde(default)]
    pub(super) driver: Option<String>,
    #[serde(default)]
    pub(super) parent: Option<String>,
    #[serde(default, rename = "ipvlan_mode")]
    pub(super) ipvlan_mode: Option<String>,
    #[serde(default)]
    pub(super) internal: Option<bool>,
    #[serde(default)]
    pub(super) attachable: Option<bool>,
    #[serde(default)]
    pub(super) ipv4: Option<NetworkTemplateIpv4>,
    #[serde(default)]
    pub(super) options: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub(super) labels: Option<BTreeMap<String, String>>,
}

pub(super) fn parse_container_inspects(raw: &str) -> anyhow::Result<Vec<ContainerInspect>> {
    serde_json::from_str(raw).context("inspect output was not JSON array")
}

pub(super) fn parse_network_inspect(raw: &str) -> anyhow::Result<NetworkInspect> {
    serde_json::from_str(raw).context("network inspect was not valid JSON")
}

pub(super) fn extract_template_description_from_file(path: &Path) -> Option<String> {
    let data = fs::read_to_string(path).ok()?;
    for line in data.lines().take(40) {
        let l = line.trim_start();
        if !l.starts_with('#') {
            if !l.is_empty() {
                break;
            }
            continue;
        }
        let body = l.trim_start_matches('#').trim_start();
        let low = body.to_ascii_lowercase();
        let key = if low.starts_with("description:") {
            "description:"
        } else if low.starts_with("desc:") {
            "desc:"
        } else {
            continue;
        };
        let value = body[key.len()..].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}
