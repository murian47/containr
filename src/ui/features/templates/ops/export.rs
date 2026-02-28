use super::common::{
    ComposeHealthcheck, ComposeNetwork, ComposeNetworkIpam, ComposeNetworkIpamConfig,
    ComposeService, ComposeVolume, ContainerInspect, NetworkInspect, NetworkTemplateSpecWrite,
    filter_labels, hc_duration, hc_interval, hc_retries, hc_start_period, hc_timeout,
    is_system_network_name, parse_container_inspects, parse_network_inspect,
    service_name_from_labels, stack_name_from_label_map, unique_compose_key,
    write_net_template_cfg, write_stack_template_compose, yaml_quote,
};
use crate::docker::{self, DockerCfg};
use crate::domain::image_refs::normalize_image_ref;
use crate::runner::Runner;
use crate::ui::NetworkTemplateIpv4;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

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
    let mut inspects: Vec<ContainerInspect> = parse_container_inspects(&raw)?;
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
            Ok(raw) => match parse_network_inspect(&raw) {
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
    let net: NetworkInspect = parse_network_inspect(&raw)?;

    let driver = net.driver.clone().unwrap_or_else(|| "bridge".to_string());
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

    let labels = net
        .labels
        .as_ref()
        .map(filter_labels)
        .filter(|m| !m.is_empty());
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
        let service_name =
            service_name_from_labels(&labels, stack_hint.as_deref(), &container_name);
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
            entry.privileged = inspect.host_config.as_ref().and_then(|cfg| cfg.privileged);
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
                            let host_ip =
                                binding.host_ip.as_deref().unwrap_or("").trim().to_string();
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
                    let dest = mount
                        .destination
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .to_string();
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
        service_keys.insert(
            name.clone(),
            unique_compose_key(name, &mut used_service_keys),
        );
    }

    let mut network_keys: HashMap<String, String> = HashMap::new();
    let mut used_network_keys: HashSet<String> = HashSet::new();
    for name in &network_refs {
        network_keys.insert(
            name.clone(),
            unique_compose_key(name, &mut used_network_keys),
        );
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
