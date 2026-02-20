//! Template import/export helpers.

use crate::docker::{self, DockerCfg};
use crate::domain::image_refs::normalize_image_ref;
use crate::ui::render::highlight::split_yaml_comment;
use crate::ui::{extract_template_id, App, NetworkTemplateIpv4, Runner};
use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ContainerInspect {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Config")]
    config: Option<ContainerInspectConfig>,
    #[serde(rename = "HostConfig")]
    host_config: Option<ContainerInspectHostConfig>,
    #[serde(rename = "NetworkSettings")]
    network_settings: Option<ContainerInspectNetworkSettings>,
    #[serde(rename = "Mounts")]
    mounts: Option<Vec<ContainerInspectMount>>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectConfig {
    #[serde(rename = "Image")]
    image: Option<String>,
    #[serde(rename = "Env")]
    env: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    cmd: Option<Vec<String>>,
    #[serde(rename = "Entrypoint")]
    entrypoint: Option<Vec<String>>,
    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
    #[serde(rename = "WorkingDir")]
    working_dir: Option<String>,
    #[serde(rename = "User")]
    user: Option<String>,
    #[serde(rename = "ExposedPorts")]
    exposed_ports: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "Healthcheck")]
    healthcheck: Option<ContainerInspectHealthcheck>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectHealthcheck {
    #[serde(rename = "Test")]
    test: Option<Vec<String>>,
    #[serde(rename = "Interval")]
    interval: Option<i64>,
    #[serde(rename = "Timeout")]
    timeout: Option<i64>,
    #[serde(rename = "Retries")]
    retries: Option<i64>,
    #[serde(rename = "StartPeriod")]
    start_period: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectHostConfig {
    #[serde(rename = "RestartPolicy")]
    restart_policy: Option<ContainerInspectRestartPolicy>,
    #[serde(rename = "PortBindings")]
    port_bindings: Option<HashMap<String, Vec<ContainerInspectPortBinding>>>,
    #[serde(rename = "ReadonlyRootfs")]
    readonly_rootfs: Option<bool>,
    #[serde(rename = "Privileged")]
    privileged: Option<bool>,
    #[serde(rename = "ExtraHosts")]
    extra_hosts: Option<Vec<String>>,
    #[serde(rename = "NetworkMode")]
    network_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectRestartPolicy {
    #[serde(rename = "Name")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectPortBinding {
    #[serde(rename = "HostIp")]
    host_ip: Option<String>,
    #[serde(rename = "HostPort")]
    host_port: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectNetworkSettings {
    #[serde(rename = "Networks")]
    networks: Option<HashMap<String, ContainerInspectNetworkAttachment>>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectNetworkAttachment {
    #[serde(rename = "Aliases")]
    aliases: Option<Vec<String>>,
    #[serde(rename = "IPAddress")]
    ip_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContainerInspectMount {
    #[serde(rename = "Type")]
    kind: Option<String>,
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Source")]
    source: Option<String>,
    #[serde(rename = "Destination")]
    destination: Option<String>,
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "ReadOnly")]
    read_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct NetworkInspect {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "Internal")]
    internal: Option<bool>,
    #[serde(rename = "Attachable")]
    attachable: Option<bool>,
    #[serde(rename = "EnableIPv6")]
    enable_ipv6: Option<bool>,
    #[serde(rename = "Options")]
    options: Option<HashMap<String, String>>,
    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
    #[serde(rename = "IPAM")]
    ipam: Option<NetworkInspectIpam>,
}

#[derive(Debug, Deserialize)]
struct NetworkInspectIpam {
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "Config")]
    config: Option<Vec<NetworkInspectIpamConfig>>,
}

#[derive(Debug, Deserialize)]
struct NetworkInspectIpamConfig {
    #[serde(rename = "Subnet")]
    subnet: Option<String>,
    #[serde(rename = "Gateway")]
    gateway: Option<String>,
    #[serde(rename = "IPRange")]
    ip_range: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeService {
    image: String,
    container_name: Option<String>,
    command: Vec<String>,
    entrypoint: Vec<String>,
    environment: Vec<String>,
    ports: Vec<String>,
    expose: Vec<String>,
    volumes: Vec<String>,
    tmpfs: Vec<String>,
    networks: BTreeMap<String, ComposeServiceNetwork>,
    labels: BTreeMap<String, String>,
    restart: Option<String>,
    working_dir: Option<String>,
    user: Option<String>,
    privileged: Option<bool>,
    read_only: Option<bool>,
    extra_hosts: Vec<String>,
    healthcheck: Option<ComposeHealthcheck>,
    network_mode: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeServiceNetwork {
    aliases: Vec<String>,
    ipv4_address: Option<String>,
}

#[derive(Clone, Debug)]
struct ComposeHealthcheck {
    test: Vec<String>,
    interval: Option<String>,
    timeout: Option<String>,
    retries: Option<i64>,
    start_period: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeNetwork {
    name: String,
    driver: Option<String>,
    internal: Option<bool>,
    attachable: Option<bool>,
    enable_ipv6: Option<bool>,
    ipam: Option<ComposeNetworkIpam>,
    options: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct ComposeNetworkIpam {
    driver: Option<String>,
    config: Vec<ComposeNetworkIpamConfig>,
}

#[derive(Clone, Debug)]
struct ComposeNetworkIpamConfig {
    subnet: Option<String>,
    gateway: Option<String>,
    ip_range: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ComposeVolume {
    driver: Option<String>,
}

fn is_system_network_name(name: &str) -> bool {
    matches!(
        name,
        "bridge" | "host" | "none" | "ingress" | "docker_gwbridge"
    )
}

fn stack_name_from_label_map(labels: &HashMap<String, String>) -> Option<String> {
    labels
        .get("com.docker.compose.project")
        .or_else(|| labels.get("com.docker.stack.namespace"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn service_name_from_labels(
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

fn sanitize_compose_key(name: &str) -> String {
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

fn unique_compose_key(name: &str, used: &mut HashSet<String>) -> String {
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

fn yaml_quote(text: &str) -> String {
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

pub(in crate::ui) fn images_from_compose(path: &Path) -> Vec<String> {
    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let (code, _) = split_yaml_comment(line);
        let trimmed = code.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("image:") {
            let mut val = rest.trim().to_string();
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                val = val[1..val.len().saturating_sub(1)].to_string();
            }
            if !val.is_empty() {
                out.push(val);
            }
        }
    }
    out
}
fn format_duration_ns(ns: i64) -> Option<String> {
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

fn filter_labels(labels: &HashMap<String, String>) -> BTreeMap<String, String> {
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

pub(in crate::ui) fn write_stack_template_compose(
    templates_dir: &PathBuf,
    name: &str,
    compose: &str,
) -> anyhow::Result<PathBuf> {
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

    fs::create_dir_all(templates_dir)?;
    let dir = templates_dir.join(name);
    if dir.exists() && !dir.is_dir() {
        anyhow::bail!("template path exists but is not a directory: {}", dir.display());
    }
    fs::create_dir_all(&dir)?;
    let compose_path = dir.join("compose.yaml");
    fs::write(&compose_path, compose)?;
    Ok(compose_path)
}

pub(in crate::ui) fn write_net_template_cfg(
    templates_dir: &PathBuf,
    name: &str,
    cfg: &str,
) -> anyhow::Result<PathBuf> {
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

    fs::create_dir_all(templates_dir)?;
    let dir = templates_dir.join(name);
    anyhow::ensure!(
        !dir.exists(),
        "template already exists: {}",
        dir.display()
    );
    fs::create_dir_all(&dir)?;
    let cfg_path = dir.join("network.json");
    fs::write(&cfg_path, cfg)?;
    Ok(cfg_path)
}

pub(in crate::ui) async fn export_stack_template(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    source: &str,
    stack_name: Option<&str>,
    container_ids: &[String],
    templates_dir: &PathBuf,
) -> anyhow::Result<String> {
    anyhow::ensure!(!container_ids.is_empty(), "no containers selected");
    let raw = docker::fetch_inspects(runner, docker, container_ids).await?;
    let mut inspects: Vec<ContainerInspect> =
        serde_json::from_str(&raw).context("inspect output was not JSON array")?;
    if inspects.is_empty() {
        anyhow::bail!("no container inspect data returned");
    }

    inspects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let mut warnings: Vec<String> = Vec::new();
    let mut network_names: BTreeSet<String> = BTreeSet::new();
    for inspect in &inspects {
        if let Some(settings) = &inspect.network_settings {
            if let Some(nets) = &settings.networks {
                for name in nets.keys() {
                    if is_system_network_name(name) {
                        continue;
                    }
                    network_names.insert(name.clone());
                }
            }
        }
    }

    let mut networks: Vec<NetworkInspect> = Vec::new();
    for name in &network_names {
        match docker::fetch_network_inspect(runner, docker, name).await {
            Ok(raw) => match serde_json::from_str::<NetworkInspect>(&raw) {
                Ok(net) => networks.push(net),
                Err(e) => warnings.push(format!("network inspect parse failed for {name}: {e:#}")),
            },
            Err(e) => warnings.push(format!("network inspect failed for {name}: {e:#}")),
        }
    }

    let stack_name = stack_name.map(|s| s.to_string()).or_else(|| {
        inspects
            .first()
            .and_then(|inspect| inspect.config.as_ref())
            .and_then(|cfg| cfg.labels.as_ref())
            .and_then(stack_name_from_label_map)
    });
    let compose = build_compose_yaml(name, stack_name.as_deref(), source, &inspects, &networks);
    write_stack_template_compose(templates_dir, name, &compose)?;
    Ok(warnings.join("\n"))
}

#[derive(serde::Serialize)]
struct NetworkTemplateSpecWrite {
    #[serde(default)]
    description: Option<String>,
    name: String,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default, rename = "ipvlan_mode")]
    ipvlan_mode: Option<String>,
    #[serde(default)]
    internal: Option<bool>,
    #[serde(default)]
    attachable: Option<bool>,
    #[serde(default)]
    ipv4: Option<NetworkTemplateIpv4>,
    #[serde(default)]
    options: Option<BTreeMap<String, String>>,
    #[serde(default)]
    labels: Option<BTreeMap<String, String>>,
}

pub(in crate::ui) async fn export_net_template(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    source: &str,
    network_id: &str,
    templates_dir: &PathBuf,
) -> anyhow::Result<String> {
    let network_id = network_id.trim();
    anyhow::ensure!(!network_id.is_empty(), "no network selected");
    let raw = docker::fetch_network_inspect(runner, docker, network_id).await?;
    let net: NetworkInspect =
        serde_json::from_str(&raw).context("network inspect was not valid JSON")?;

    let driver = net
        .driver
        .clone()
        .unwrap_or_else(|| "bridge".to_string());
    let mut options_map: HashMap<String, String> = net.options.clone().unwrap_or_default();
    let mut parent = None;
    let mut ipvlan_mode = None;
    if driver == "ipvlan" {
        if let Some(value) = options_map.remove("parent") {
            if !value.trim().is_empty() {
                parent = Some(value);
            }
        }
        if let Some(value) = options_map.remove("ipvlan_mode") {
            if !value.trim().is_empty() {
                ipvlan_mode = Some(value);
            }
        }
    }
    let options = if options_map.is_empty() {
        None
    } else {
        let mut out = BTreeMap::new();
        for (k, v) in options_map {
            out.insert(k, v);
        }
        Some(out)
    };

    let labels = net.labels.as_ref().map(filter_labels).filter(|m| !m.is_empty());
    let mut ipv4 = None;
    if let Some(ipam) = net.ipam.as_ref() {
        if let Some(cfgs) = ipam.config.as_ref() {
            for cfg in cfgs {
                let Some(subnet) = cfg.subnet.as_ref() else {
                    continue;
                };
                if subnet.contains(':') {
                    continue;
                }
                ipv4 = Some(NetworkTemplateIpv4 {
                    subnet: Some(subnet.clone()),
                    gateway: cfg.gateway.clone(),
                    ip_range: cfg.ip_range.clone(),
                });
                break;
            }
        }
    }

    let spec = NetworkTemplateSpecWrite {
        description: Some(format!("Imported from {source}")),
        name: net.name,
        driver: Some(driver),
        parent,
        ipvlan_mode,
        internal: net.internal,
        attachable: net.attachable,
        ipv4,
        options,
        labels,
    };
    let cfg = serde_json::to_string_pretty(&spec)?;
    write_net_template_cfg(templates_dir, name, &cfg)?;
    Ok(String::new())
}

fn build_compose_yaml(
    template_name: &str,
    stack_name: Option<&str>,
    source: &str,
    inspects: &[ContainerInspect],
    networks: &[NetworkInspect],
) -> String {
    let mut service_counts: HashMap<String, usize> = HashMap::new();
    for inspect in inspects {
        let labels = inspect
            .config
            .as_ref()
            .and_then(|cfg| cfg.labels.as_ref())
            .cloned()
            .unwrap_or_default();
        let name = inspect.name.trim_start_matches('/').to_string();
        let stack_hint = stack_name
            .map(|v| v.to_string())
            .or_else(|| stack_name_from_label_map(&labels));
        let svc = service_name_from_labels(&labels, stack_hint.as_deref(), &name);
        *service_counts.entry(svc).or_insert(0) += 1;
    }

    let mut services: BTreeMap<String, ComposeService> = BTreeMap::new();
    let mut volume_defs: BTreeMap<String, ComposeVolume> = BTreeMap::new();
    let mut network_refs: BTreeSet<String> = BTreeSet::new();

    for inspect in inspects {
        let labels = inspect
            .config
            .as_ref()
            .and_then(|cfg| cfg.labels.as_ref())
            .cloned()
            .unwrap_or_default();
        let container_name = inspect.name.trim_start_matches('/').to_string();
        let stack_hint = stack_name
            .map(|v| v.to_string())
            .or_else(|| stack_name_from_label_map(&labels));
        let service_name = service_name_from_labels(&labels, stack_hint.as_deref(), &container_name);
        let entry = services.entry(service_name.clone()).or_default();

        if entry.image.is_empty() {
            entry.image = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.image.as_ref())
                .map(|v| normalize_image_ref(v))
                .unwrap_or_default();
            entry.container_name = if service_counts.get(&service_name).copied().unwrap_or(1) > 1 {
                None
            } else if container_name.is_empty() {
                None
            } else {
                Some(container_name.clone())
            };
            entry.command = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.cmd.clone())
                .unwrap_or_default();
            entry.entrypoint = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.entrypoint.clone())
                .unwrap_or_default();
            let mut env = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.env.clone())
                .unwrap_or_default();
            env.sort();
            entry.environment = env;
            entry.labels = filter_labels(&labels);
            entry.working_dir = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.working_dir.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            entry.user = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.user.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            entry.healthcheck = inspect
                .config
                .as_ref()
                .and_then(|cfg| cfg.healthcheck.as_ref())
                .and_then(|hc| hc.test.clone())
                .filter(|test| !test.is_empty())
                .map(|test| ComposeHealthcheck {
                    test,
                    interval: hc_duration(hc_interval(&inspect)),
                    timeout: hc_duration(hc_timeout(&inspect)),
                    retries: hc_retries(&inspect),
                    start_period: hc_duration(hc_start_period(&inspect)),
                });
            entry.restart = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.restart_policy.as_ref())
                .and_then(|rp| rp.name.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty() && v != "no");
            entry.read_only = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.readonly_rootfs);
            entry.privileged = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.privileged);
            let mut extra_hosts = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.extra_hosts.clone())
                .unwrap_or_default();
            extra_hosts.sort();
            entry.extra_hosts = extra_hosts;

            let network_mode = inspect
                .host_config
                .as_ref()
                .and_then(|cfg| cfg.network_mode.as_ref())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty() && v != "default" && v != "bridge");
            entry.network_mode = network_mode;

            if let Some(host_cfg) = &inspect.host_config {
                if let Some(bindings) = &host_cfg.port_bindings {
                    let mut ports = Vec::new();
                    let mut expose = Vec::new();
                    for (key, binds) in bindings {
                        let container_port = key.split('/').next().unwrap_or(key).trim();
                        if container_port.is_empty() {
                            continue;
                        }
                        if binds.is_empty() {
                            expose.push(container_port.to_string());
                            continue;
                        }
                        for binding in binds {
                            let host_port = binding
                                .host_port
                                .as_deref()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            if host_port.is_empty() {
                                expose.push(container_port.to_string());
                                continue;
                            }
                            let host_ip = binding
                                .host_ip
                                .as_deref()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let spec = if host_ip.is_empty() || host_ip == "0.0.0.0" {
                                format!("{host_port}:{container_port}")
                            } else {
                                format!("{host_ip}:{host_port}:{container_port}")
                            };
                            ports.push(spec);
                        }
                    }
                    ports.sort();
                    expose.sort();
                    entry.ports = ports;
                    entry.expose = expose;
                }
            }

            if entry.ports.is_empty() && entry.expose.is_empty() {
                if let Some(exposed) = inspect
                    .config
                    .as_ref()
                    .and_then(|cfg| cfg.exposed_ports.as_ref())
                {
                    let mut expose = Vec::new();
                    for key in exposed.keys() {
                        let port = key.split('/').next().unwrap_or(key).trim();
                        if !port.is_empty() {
                            expose.push(port.to_string());
                        }
                    }
                    expose.sort();
                    entry.expose = expose;
                }
            }

            if let Some(mounts) = &inspect.mounts {
                let mut volumes = Vec::new();
                let mut tmpfs = Vec::new();
                for mount in mounts {
                    let kind = mount.kind.as_deref().unwrap_or("").to_ascii_lowercase();
                    let dest = mount.destination.as_deref().unwrap_or("").trim().to_string();
                    if dest.is_empty() {
                        continue;
                    }
                    match kind.as_str() {
                        "bind" => {
                            let source = mount.source.as_deref().unwrap_or("").trim();
                            if source.is_empty() {
                                continue;
                            }
                            let mut spec = format!("{source}:{dest}");
                            if mount.read_only.unwrap_or(false) {
                                spec.push_str(":ro");
                            }
                            volumes.push(spec);
                        }
                        "volume" => {
                            let name = mount
                                .name
                                .as_deref()
                                .map(|v| v.trim().to_string())
                                .filter(|v| !v.is_empty())
                                .or_else(|| {
                                    mount
                                        .source
                                        .as_deref()
                                        .map(|v| v.trim().to_string())
                                        .filter(|v| !v.is_empty())
                                });
                            if let Some(name) = name {
                                let mut spec = format!("{name}:{dest}");
                                if mount.read_only.unwrap_or(false) {
                                    spec.push_str(":ro");
                                }
                                volumes.push(spec);
                                volume_defs.entry(name).or_insert_with(|| ComposeVolume {
                                    driver: mount
                                        .driver
                                        .as_deref()
                                        .map(|v| v.trim().to_string())
                                        .filter(|v| !v.is_empty()),
                                });
                            }
                        }
                        "tmpfs" => {
                            tmpfs.push(dest);
                        }
                        _ => {}
                    }
                }
                volumes.sort();
                tmpfs.sort();
                entry.volumes = volumes;
                entry.tmpfs = tmpfs;
            }
        }

        if entry.network_mode.is_none() {
            if let Some(settings) = &inspect.network_settings {
                if let Some(nets) = &settings.networks {
                    for (name, attachment) in nets {
                        if is_system_network_name(name) {
                            continue;
                        }
                        network_refs.insert(name.clone());
                        let svc_net = entry.networks.entry(name.clone()).or_default();
                        if let Some(aliases) = &attachment.aliases {
                            for alias in aliases {
                                let alias = alias.trim();
                                if alias.is_empty() || svc_net.aliases.contains(&alias.to_string())
                                {
                                    continue;
                                }
                                svc_net.aliases.push(alias.to_string());
                            }
                        }
                        if svc_net.ipv4_address.is_none() {
                            if let Some(ip) = &attachment.ip_address {
                                let ip = ip.trim();
                                if !ip.is_empty() {
                                    svc_net.ipv4_address = Some(ip.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut service_keys: HashMap<String, String> = HashMap::new();
    let mut used_service_keys: HashSet<String> = HashSet::new();
    for name in services.keys() {
        service_keys.insert(name.clone(), unique_compose_key(name, &mut used_service_keys));
    }

    let mut network_keys: HashMap<String, String> = HashMap::new();
    let mut used_network_keys: HashSet<String> = HashSet::new();
    for name in &network_refs {
        network_keys.insert(name.clone(), unique_compose_key(name, &mut used_network_keys));
    }

    let mut network_defs: BTreeMap<String, ComposeNetwork> = BTreeMap::new();
    for net in networks {
        if !network_refs.contains(&net.name) {
            continue;
        }
        let key = network_keys
            .get(&net.name)
            .cloned()
            .unwrap_or_else(|| net.name.clone());
        let mut entry = ComposeNetwork {
            name: net.name.clone(),
            driver: net
                .driver
                .as_deref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            internal: net.internal,
            attachable: net.attachable,
            enable_ipv6: net.enable_ipv6,
            ipam: None,
            options: BTreeMap::new(),
            labels: BTreeMap::new(),
        };
        if let Some(opts) = &net.options {
            for (k, v) in opts {
                entry.options.insert(k.clone(), v.clone());
            }
        }
        if let Some(labels) = &net.labels {
            entry.labels = filter_labels(labels);
        }
        if let Some(ipam) = &net.ipam {
            let mut configs = Vec::new();
            if let Some(cfgs) = &ipam.config {
                for cfg in cfgs {
                    if cfg.subnet.is_none() && cfg.gateway.is_none() && cfg.ip_range.is_none() {
                        continue;
                    }
                    configs.push(ComposeNetworkIpamConfig {
                        subnet: cfg.subnet.clone(),
                        gateway: cfg.gateway.clone(),
                        ip_range: cfg.ip_range.clone(),
                    });
                }
            }
            if !configs.is_empty() || ipam.driver.as_ref().is_some() {
                entry.ipam = Some(ComposeNetworkIpam {
                    driver: ipam.driver.clone(),
                    config: configs,
                });
            }
        }
        network_defs.insert(key, entry);
    }

    for (name, key) in &network_keys {
        if !network_defs.contains_key(key) {
            network_defs.insert(
                key.clone(),
                ComposeNetwork {
                    name: name.clone(),
                    ..ComposeNetwork::default()
                },
            );
        }
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("# Generated by containr".to_string());
    lines.push(format!("# Source: {source}"));
    lines.push(format!("# description: Exported from {source}"));
    lines.push("".to_string());
    lines.push(format!("name: {}", yaml_quote(template_name)));
    lines.push("".to_string());
    lines.push("services:".to_string());
    for (svc_name, svc) in &services {
        let key = service_keys
            .get(svc_name)
            .cloned()
            .unwrap_or_else(|| svc_name.clone());
        lines.push(format!("  {key}:"));
        if !svc.image.is_empty() {
            lines.push(format!("    image: {}", yaml_quote(&svc.image)));
        }
        if let Some(name) = &svc.container_name {
            lines.push(format!("    container_name: {}", yaml_quote(name)));
        }
        if !svc.command.is_empty() {
            lines.push("    command:".to_string());
            for part in &svc.command {
                lines.push(format!("      - {}", yaml_quote(part)));
            }
        }
        if !svc.entrypoint.is_empty() {
            lines.push("    entrypoint:".to_string());
            for part in &svc.entrypoint {
                lines.push(format!("      - {}", yaml_quote(part)));
            }
        }
        if let Some(workdir) = &svc.working_dir {
            lines.push(format!("    working_dir: {}", yaml_quote(workdir)));
        }
        if let Some(user) = &svc.user {
            lines.push(format!("    user: {}", yaml_quote(user)));
        }
        if let Some(restart) = &svc.restart {
            lines.push(format!("    restart: {}", yaml_quote(restart)));
        }
        if let Some(privileged) = svc.privileged {
            lines.push(format!("    privileged: {}", privileged));
        }
        if let Some(read_only) = svc.read_only {
            lines.push(format!("    read_only: {}", read_only));
        }
        if let Some(mode) = &svc.network_mode {
            lines.push(format!("    network_mode: {}", yaml_quote(mode)));
        }
        if !svc.extra_hosts.is_empty() {
            lines.push("    extra_hosts:".to_string());
            for host in &svc.extra_hosts {
                lines.push(format!("      - {}", yaml_quote(host)));
            }
        }
        if !svc.environment.is_empty() {
            lines.push("    environment:".to_string());
            for env in &svc.environment {
                lines.push(format!("      - {}", yaml_quote(env)));
            }
        }
        if !svc.labels.is_empty() {
            lines.push("    labels:".to_string());
            for (k, v) in &svc.labels {
                lines.push(format!("      {}: {}", yaml_quote(k), yaml_quote(v)));
            }
        }
        if !svc.ports.is_empty() {
            lines.push("    ports:".to_string());
            for port in &svc.ports {
                lines.push(format!("      - {}", yaml_quote(port)));
            }
        }
        if !svc.expose.is_empty() {
            lines.push("    expose:".to_string());
            for port in &svc.expose {
                lines.push(format!("      - {}", yaml_quote(port)));
            }
        }
        if !svc.volumes.is_empty() {
            lines.push("    volumes:".to_string());
            for volume in &svc.volumes {
                lines.push(format!("      - {}", yaml_quote(volume)));
            }
        }
        if !svc.tmpfs.is_empty() {
            lines.push("    tmpfs:".to_string());
            for dest in &svc.tmpfs {
                lines.push(format!("      - {}", yaml_quote(dest)));
            }
        }
        if let Some(hc) = &svc.healthcheck {
            lines.push("    healthcheck:".to_string());
            lines.push("      test:".to_string());
            for part in &hc.test {
                lines.push(format!("        - {}", yaml_quote(part)));
            }
            if let Some(interval) = &hc.interval {
                lines.push(format!("      interval: {}", yaml_quote(interval)));
            }
            if let Some(timeout) = &hc.timeout {
                lines.push(format!("      timeout: {}", yaml_quote(timeout)));
            }
            if let Some(retries) = hc.retries {
                lines.push(format!("      retries: {}", retries));
            }
            if let Some(start_period) = &hc.start_period {
                lines.push(format!("      start_period: {}", yaml_quote(start_period)));
            }
        }
        if svc.network_mode.is_none() && !svc.networks.is_empty() {
            let has_options = svc.networks.values().any(|n| {
                !n.aliases.is_empty() || n.ipv4_address.as_ref().is_some_and(|v| !v.is_empty())
            });
            lines.push("    networks:".to_string());
            for (name, opts) in &svc.networks {
                let key = network_keys
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                if has_options {
                    if opts.aliases.is_empty()
                        && opts
                            .ipv4_address
                            .as_ref()
                            .map(|v| v.is_empty())
                            .unwrap_or(true)
                    {
                        lines.push(format!("      {key}: {{}}"));
                        continue;
                    }
                    lines.push(format!("      {key}:"));
                    if !opts.aliases.is_empty() {
                        lines.push("        aliases:".to_string());
                        for alias in &opts.aliases {
                            lines.push(format!("          - {}", yaml_quote(alias)));
                        }
                    }
                    if let Some(ip) = &opts.ipv4_address {
                        if !ip.is_empty() {
                            lines.push(format!("        ipv4_address: {}", yaml_quote(ip)));
                        }
                    }
                } else {
                    lines.push(format!("      - {key}"));
                }
            }
        }
    }

    if !volume_defs.is_empty() {
        lines.push("".to_string());
        lines.push("volumes:".to_string());
        for (name, vol) in &volume_defs {
            if let Some(driver) = &vol.driver {
                lines.push(format!("  {}:", yaml_quote(name)));
                lines.push(format!("    driver: {}", yaml_quote(driver)));
            } else {
                lines.push(format!("  {}: {{}}", yaml_quote(name)));
            }
        }
    }

    if !network_defs.is_empty() {
        lines.push("".to_string());
        lines.push("networks:".to_string());
        for (key, net) in &network_defs {
            lines.push(format!("  {key}:"));
            lines.push(format!("    name: {}", yaml_quote(&net.name)));
            if let Some(driver) = &net.driver {
                lines.push(format!("    driver: {}", yaml_quote(driver)));
            }
            if let Some(internal) = net.internal {
                lines.push(format!("    internal: {}", internal));
            }
            if let Some(attachable) = net.attachable {
                lines.push(format!("    attachable: {}", attachable));
            }
            if let Some(enable_ipv6) = net.enable_ipv6 {
                lines.push(format!("    enable_ipv6: {}", enable_ipv6));
            }
            if let Some(ipam) = &net.ipam {
                lines.push("    ipam:".to_string());
                if let Some(driver) = &ipam.driver {
                    lines.push(format!("      driver: {}", yaml_quote(driver)));
                }
                if !ipam.config.is_empty() {
                    lines.push("      config:".to_string());
                    for cfg in &ipam.config {
                        lines.push("        -".to_string());
                        if let Some(subnet) = &cfg.subnet {
                            lines.push(format!("          subnet: {}", yaml_quote(subnet)));
                        }
                        if let Some(gateway) = &cfg.gateway {
                            lines.push(format!("          gateway: {}", yaml_quote(gateway)));
                        }
                        if let Some(ip_range) = &cfg.ip_range {
                            lines.push(format!("          ip_range: {}", yaml_quote(ip_range)));
                        }
                    }
                }
            }
            if !net.options.is_empty() {
                lines.push("    options:".to_string());
                for (k, v) in &net.options {
                    lines.push(format!("      {}: {}", yaml_quote(k), yaml_quote(v)));
                }
            }
            if !net.labels.is_empty() {
                lines.push("    labels:".to_string());
                for (k, v) in &net.labels {
                    lines.push(format!("      {}: {}", yaml_quote(k), yaml_quote(v)));
                }
            }
        }
    }

    if lines.last().is_some_and(|l| !l.is_empty()) {
        lines.push(String::new());
    }
    lines.join("\n")
}

fn hc_interval(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.interval)
}

fn hc_timeout(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.timeout)
}

fn hc_start_period(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.start_period)
}

fn hc_retries(inspect: &ContainerInspect) -> Option<i64> {
    inspect
        .config
        .as_ref()
        .and_then(|cfg| cfg.healthcheck.as_ref())
        .and_then(|hc| hc.retries)
}

fn hc_duration(value: Option<i64>) -> Option<String> {
    value.and_then(format_duration_ns)
}

pub(in crate::ui) fn create_template(app: &mut App, name: &str) -> anyhow::Result<()> {
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

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let dir = stacks_dir.join(name);
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;
    let compose = dir.join("compose.yaml");
    let skeleton = r#"# Stack template (docker compose)
# description: REPLACE_WITH_A_SHORT_DESCRIPTION
#
# Tips:
# - Keep values simple and edit after creation.
# - Add more services as needed.
# - Use named volumes for persistent data.
#
# Docs: https://docs.docker.com/compose/compose-file/

name: REPLACE_STACK_NAME

services:
  app:
    image: REPLACE_IMAGE:latest
    container_name: REPLACE_CONTAINER_NAME
    restart: unless-stopped

    # Optional: publish ports (host:container)
    ports:
      - "8080:80"

    # Optional: environment variables
    environment:
      TZ: "UTC"
      EXAMPLE: "value"

    # Optional: bind-mounts or named volumes
    volumes:
      - app_data:/var/lib/app

    # Optional: networks (useful when you run multiple services)
    networks:
      - app_net

    # Optional: healthcheck
    healthcheck:
      test: ["CMD", "sh", "-lc", "curl -fsS http://localhost/ || exit 1"]
      interval: 30s
      timeout: 5s
      retries: 3

    # Optional: labels (containr can add its own labels during deploy later)
    labels:
      com.example.stack: "REPLACE_STACK_NAME"

volumes:
  app_data:
    driver: local

networks:
  app_net:
    driver: bridge
"#;
    fs::write(&compose, skeleton)?;
    Ok(())
}

pub(in crate::ui) fn create_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
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

    let root = app.net_templates_dir();
    fs::create_dir_all(&root)?;
    let dir = root.join(name);
    anyhow::ensure!(!dir.exists(), "template already exists: {}", dir.display());
    fs::create_dir_all(&dir)?;

    let cfg = dir.join("network.json");
    let skeleton = format!(
        r#"{{
  "description": "Shared network template (edit me)",
  "name": "{name}",
  "driver": "ipvlan",
  "parent": "eth0.10",
  "ipvlan_mode": "l2",
  "ipv4": {{
    "subnet": "192.168.10.0/24",
    "gateway": "192.168.10.1",
    "ip_range": null
  }},
  "internal": null,
  "attachable": null,
  "options": {{}},
  "labels": {{}}
}}
"#
    );
    fs::write(&cfg, skeleton)?;
    Ok(())
}















pub(in crate::ui) fn delete_template(app: &mut App, name: &str) -> anyhow::Result<()> {
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

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let dir = stacks_dir.join(name);
    anyhow::ensure!(dir.exists(), "template does not exist: {}", dir.display());

    let root = fs::canonicalize(&stacks_dir)?;
    let target = fs::canonicalize(&dir)?;
    anyhow::ensure!(
        target.starts_with(&root),
        "refusing to delete outside templates dir"
    );

    fs::remove_dir_all(&target)?;
    if let Some(info) = extract_template_id(&dir.join("compose.yaml")) {
        if app.template_deploys.remove(&info).is_some() {
            app.save_local_state();
        }
    }
    Ok(())
}

pub(in crate::ui) fn delete_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
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

    let root = app.net_templates_dir();
    fs::create_dir_all(&root)?;
    let dir = root.join(name);
    anyhow::ensure!(dir.exists(), "template does not exist: {}", dir.display());

    let root_can = fs::canonicalize(&root)?;
    let dir_can = fs::canonicalize(&dir)?;
    anyhow::ensure!(
        dir_can.starts_with(&root_can),
        "refusing to delete outside templates dir"
    );

    fs::remove_dir_all(&dir_can)?;
    Ok(())
}


pub(in crate::ui) fn extract_net_template_description(path: &PathBuf) -> Option<String> {
    let data = fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;
    let d = v.get("description")?.as_str()?.trim();
    if d.is_empty() {
        None
    } else {
        Some(d.to_string())
    }
}
