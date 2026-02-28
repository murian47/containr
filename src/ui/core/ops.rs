use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::config;
use crate::docker::DockerCfg;
use crate::runner::Runner;
use crate::ui::core::types::{NetworkTemplateSpec, RegistryAuthResolved, StackUpdateService};
use crate::ui::features::templates::render_compose_with_template_id;
use crate::ui::helpers::{
    deploy_remote_dir_for, deploy_remote_net_dir_for, ensure_template_id, shell_quote_with_home,
    shell_single_quote, truncate_msg,
};

pub(in crate::ui) async fn perform_template_deploy(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    local_compose: &Path,
    pull: bool,
    force_recreate: bool,
    template_commit: Option<&str>,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let remote_dir = match runner {
        Runner::Local => {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            format!("{home}/.config/containr/apps/{name}")
        }
        Runner::Ssh(_) => deploy_remote_dir_for(name),
    };
    let template_id = ensure_template_id(&local_compose.to_path_buf())?;
    let rendered_path =
        render_compose_with_template_id(local_compose, &template_id, template_commit)?;
    let remote_compose = format!("{remote_dir}/compose.rendered.yaml");
    let remote_dir_q = shell_single_quote(&remote_dir);
    let compose_cmd = docker.docker_cmd.to_compose_shell();
    let mkdir_cmd = format!("mkdir -p {remote_dir_q}");
    let pull_cmd = format!("cd {remote_dir_q} && {compose_cmd} -f compose.rendered.yaml pull");
    let recreate_flag = if force_recreate {
        " --force-recreate"
    } else {
        ""
    };
    let up_cmd =
        format!("cd {remote_dir_q} && {compose_cmd} -f compose.rendered.yaml up -d{recreate_flag}");
    runner.run(&mkdir_cmd).await?;
    runner
        .copy_file_to(rendered_path.as_ref(), &remote_compose)
        .await?;
    if pull {
        let _ = run_with_local_compose_fallback(runner, &pull_cmd).await?;
    }
    let out = run_with_local_compose_fallback(runner, &up_cmd).await?;
    Ok(out)
}

pub(in crate::ui) async fn run_with_local_compose_fallback(
    runner: &Runner,
    cmd: &str,
) -> anyhow::Result<String> {
    match runner.run(cmd).await {
        Ok(out) => Ok(out),
        Err(e) => {
            let msg = format!("{:#}", e);
            let is_missing_desktop_helper = msg.contains("docker-credential-desktop")
                && msg.contains("executable file not found");
            if !matches!(runner, Runner::Local) || !is_missing_desktop_helper {
                return Err(e);
            }

            let home = std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("HOME is not set for local compose fallback"))?;
            let cfg_dir = PathBuf::from(home).join(".config/containr/docker-no-creds");
            fs::create_dir_all(&cfg_dir).map_err(|err| {
                anyhow::anyhow!("failed to create local docker fallback dir: {err}")
            })?;
            let cfg_file = cfg_dir.join("config.json");
            fs::write(&cfg_file, "{\"auths\":{}}\n").map_err(|err| {
                anyhow::anyhow!("failed to write local docker fallback config: {err}")
            })?;
            let cfg_q = shell_single_quote(cfg_dir.to_string_lossy().as_ref());
            let wrapped = format!("export DOCKER_CONFIG={cfg_q}; {cmd}");
            runner.run(&wrapped).await
        }
    }
}

pub(in crate::ui) async fn perform_stack_update(
    runner: &Runner,
    docker: &DockerCfg,
    stack_name: &str,
    compose_dirs: &[String],
    pull: bool,
    dry: bool,
    force: bool,
    services: &[StackUpdateService],
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let mut selected_dir: Option<String> = None;
    for dir in compose_dirs {
        let dir = dir.trim();
        if dir.is_empty() {
            continue;
        }
        let ok = match runner {
            Runner::Local => Path::new(dir).is_dir(),
            Runner::Ssh(_) => {
                let dir_q = shell_quote_with_home(dir);
                runner.run(&format!("test -d {dir_q}")).await.is_ok()
            }
        };
        if ok {
            selected_dir = Some(dir.to_string());
            break;
        }
    }
    let dir = selected_dir.ok_or_else(|| {
        anyhow::anyhow!(
            "stack update: compose dir not found for {stack_name} (tried: {})",
            compose_dirs.join(", ")
        )
    })?;
    let dir_q = shell_quote_with_home(&dir);
    let file_q = shell_single_quote("compose.rendered.yaml");
    let docker_cmd = docker.docker_cmd.to_shell();
    let compose_cmd = docker.docker_cmd.to_compose_shell();
    let pull_cmd = format!("cd {dir_q} && {compose_cmd} -f {file_q} pull");
    let mut svc_args: Vec<String> = Vec::new();
    for svc in services {
        let name = svc.name.trim();
        if !name.is_empty() {
            svc_args.push(shell_single_quote(name));
        }
    }
    let svc_args_str = if svc_args.is_empty() {
        String::new()
    } else {
        format!(" {}", svc_args.join(" "))
    };
    let up_cmd =
        format!("cd {dir_q} && {compose_cmd} -f {file_q} up -d --force-recreate{svc_args_str}");
    if dry {
        let mut lines = Vec::new();
        lines.push(format!("stack update dry-run: {stack_name}"));
        lines.push(format!("compose dir: {}", dir));
        if pull {
            lines.push(pull_cmd);
        }
        lines.push(up_cmd);
        return Ok(lines.join("\n"));
    }
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("stack update: {stack_name}"));
    lines.push(format!("compose dir: {}", dir));
    if pull {
        let pull_out = run_with_local_compose_fallback(runner, &pull_cmd).await?;
        let pull_msg = pull_out.trim();
        if pull_msg.is_empty() {
            lines.push("pull: ok".to_string());
        } else {
            lines.push(format!("pull: {}", truncate_msg(pull_msg, 200)));
        }
    } else {
        lines.push("pull: skipped".to_string());
    }
    let mut to_recreate: Vec<String> = Vec::new();
    if force {
        for svc in services {
            if !svc.name.trim().is_empty() {
                to_recreate.push(svc.name.clone());
            }
        }
    } else {
        for svc in services {
            let container_id = svc.container_id.trim();
            if container_id.is_empty() {
                continue;
            }
            let image_ref = svc.image.trim();
            if image_ref.is_empty() {
                continue;
            }
            let container_id_q = shell_single_quote(container_id);
            let image_ref_q = shell_single_quote(image_ref);
            let container_cmd =
                format!("{docker_cmd} inspect --format '{{{{.Image}}}}' {container_id_q}");
            let image_cmd =
                format!("{docker_cmd} image inspect --format '{{{{.Id}}}}' {image_ref_q}");
            let current = runner.run(&container_cmd).await?.trim().to_string();
            let latest = runner.run(&image_cmd).await?.trim().to_string();
            let cur_short = if current.len() > 20 {
                truncate_msg(&current, 20)
            } else {
                current.clone()
            };
            let new_short = if latest.len() > 20 {
                truncate_msg(&latest, 20)
            } else {
                latest.clone()
            };
            if current.is_empty() || latest.is_empty() {
                lines.push(format!("svc {}: digest missing", svc.name));
                continue;
            }
            if current != latest {
                lines.push(format!(
                    "svc {}: {} -> {} (update)",
                    svc.name, cur_short, new_short
                ));
                to_recreate.push(svc.name.clone());
            } else {
                lines.push(format!("svc {}: {} (no change)", svc.name, cur_short));
            }
        }
    }
    if !force && to_recreate.is_empty() {
        lines.push("result: no changes".to_string());
        return Ok(lines.join("\n"));
    }
    if !force {
        let mut uniq: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for name in to_recreate {
            if seen.insert(name.clone()) {
                uniq.push(name);
            }
        }
        svc_args = uniq.iter().map(|name| shell_single_quote(name)).collect();
    }
    let svc_args_str = if svc_args.is_empty() {
        String::new()
    } else {
        format!(" {}", svc_args.join(" "))
    };
    let up_cmd =
        format!("cd {dir_q} && {compose_cmd} -f {file_q} up -d --force-recreate{svc_args_str}");
    if !svc_args_str.is_empty() {
        let raw = svc_args
            .iter()
            .map(|s| s.trim_matches('\''))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("recreate: {raw}"));
    } else {
        lines.push("recreate: all".to_string());
    }
    let out = run_with_local_compose_fallback(runner, &up_cmd).await?;
    let out_msg = out.trim();
    if !out_msg.is_empty() {
        lines.push(format!("compose up: {}", truncate_msg(out_msg, 200)));
    } else {
        lines.push("compose up: ok".to_string());
    }
    Ok(lines.join("\n"))
}

pub(in crate::ui) async fn perform_image_push(
    runner: &Runner,
    docker: &DockerCfg,
    source_ref: &str,
    target_ref: &str,
    registry_host: &str,
    auth: Option<&RegistryAuthResolved>,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker_cmd = docker.docker_cmd.to_shell();
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("image push: {source_ref} -> {target_ref}"));
    if let Some(auth) = auth {
        if !matches!(auth.auth, config::RegistryAuth::Anonymous) {
            let secret = auth
                .secret
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("registry secret missing for {registry_host}"))?;
            let username = auth
                .username
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| match auth.auth {
                    config::RegistryAuth::BearerToken => Some("token".to_string()),
                    config::RegistryAuth::GithubPat => Some("token".to_string()),
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("registry username missing for {registry_host}"))?;
            let pass_q = shell_single_quote(secret);
            let user_q = shell_single_quote(&username);
            let host_q = shell_single_quote(registry_host);
            let login_cmd = format!(
                "printf %s {pass_q} | {docker_cmd} login -u {user_q} --password-stdin {host_q}"
            );
            let out = runner.run(&login_cmd).await?;
            if !out.trim().is_empty() {
                lines.push(format!("login: {}", truncate_msg(out.trim(), 200)));
            } else {
                lines.push("login: ok".to_string());
            }
        } else {
            lines.push("login: skipped".to_string());
        }
    } else {
        lines.push("login: skipped".to_string());
    }
    let src_q = shell_single_quote(source_ref);
    let dst_q = shell_single_quote(target_ref);
    let tag_cmd = format!("{docker_cmd} image tag {src_q} {dst_q}");
    runner.run(&tag_cmd).await?;
    lines.push("tag: ok".to_string());
    let push_cmd = format!("{docker_cmd} image push {dst_q}");
    let out = runner.run(&push_cmd).await?;
    if !out.trim().is_empty() {
        lines.push(format!("push: {}", truncate_msg(out.trim(), 200)));
    } else {
        lines.push("push: ok".to_string());
    }
    Ok(lines.join("\n"))
}

pub(in crate::ui) async fn perform_net_template_deploy(
    runner: &Runner,
    docker: &DockerCfg,
    name: &str,
    local_cfg: &Path,
    force: bool,
) -> anyhow::Result<String> {
    if docker.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let raw = fs::read_to_string(local_cfg)
        .with_context(|| format!("failed to read {}", local_cfg.display()))?;
    let spec: NetworkTemplateSpec =
        serde_json::from_str(&raw).context("network.json was not valid JSON")?;
    let net_name = spec.name.trim();
    anyhow::ensure!(!net_name.is_empty(), "network template: name is empty");

    let remote_dir = match runner {
        Runner::Local => {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            format!("{home}/.config/containr/networks/{name}")
        }
        Runner::Ssh(_) => deploy_remote_net_dir_for(name),
    };
    let remote_cfg = format!("{remote_dir}/network.json");
    let remote_dir_q = shell_single_quote(&remote_dir);
    let mkdir_cmd = format!("mkdir -p {remote_dir_q}");
    runner.run(&mkdir_cmd).await?;
    runner.copy_file_to(local_cfg, &remote_cfg).await?;

    let docker_cmd = docker.docker_cmd.to_shell();
    let net_q = shell_single_quote(net_name);
    let exists_cmd = format!("{docker_cmd} network inspect {net_q} >/dev/null 2>&1");
    let exists = runner.run(&exists_cmd).await.is_ok();
    if exists && !force {
        return Ok("exists".to_string());
    }
    if exists && force {
        let rm_cmd = format!("{docker_cmd} network rm {net_q}");
        runner.run(&rm_cmd).await?;
    }

    let mut parts: Vec<String> = Vec::new();
    parts.push(docker_cmd.clone());
    parts.push("network".to_string());
    parts.push("create".to_string());

    let driver = spec
        .driver
        .as_deref()
        .unwrap_or("bridge")
        .trim()
        .to_string();
    parts.push("--driver".to_string());
    parts.push(shell_single_quote(&driver));

    if spec.internal.unwrap_or(false) {
        parts.push("--internal".to_string());
    }
    if spec.attachable.unwrap_or(false) {
        parts.push("--attachable".to_string());
    }

    if let Some(ipv4) = &spec.ipv4 {
        if let Some(subnet) = ipv4.subnet.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--subnet".to_string());
            parts.push(shell_single_quote(subnet.trim()));
        }
        if let Some(gw) = ipv4.gateway.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--gateway".to_string());
            parts.push(shell_single_quote(gw.trim()));
        }
        if let Some(r) = ipv4.ip_range.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--ip-range".to_string());
            parts.push(shell_single_quote(r.trim()));
        }
    }

    let mut effective_parent: Option<String> = None;
    if driver == "ipvlan" {
        let parent = spec.parent.as_deref().unwrap_or("").trim();
        anyhow::ensure!(!parent.is_empty(), "ipvlan requires 'parent'");
        effective_parent = Some(parent.to_string());
        parts.push("--opt".to_string());
        parts.push(shell_single_quote(&format!("parent={parent}")));
        if let Some(mode) = spec.ipvlan_mode.as_deref().filter(|s| !s.trim().is_empty()) {
            parts.push("--opt".to_string());
            parts.push(shell_single_quote(&format!("ipvlan_mode={}", mode.trim())));
        }
    }

    if let Some(opts) = &spec.options {
        for (k, v) in opts {
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            parts.push("--opt".to_string());
            parts.push(shell_single_quote(&format!("{k}={v}")));
        }
    }
    if let Some(labels) = &spec.labels {
        for (k, v) in labels {
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            parts.push("--label".to_string());
            parts.push(shell_single_quote(&format!("{k}={v}")));
        }
    }

    parts.push(net_q);
    let create_cmd = parts.join(" ");
    match runner.run(&create_cmd).await {
        Ok(out) => Ok(out),
        Err(primary_err) => {
            let parent = effective_parent.as_deref().unwrap_or("");
            let can_retry_macos_parent = cfg!(target_os = "macos")
                && matches!(runner, Runner::Local)
                && driver == "ipvlan"
                && parent.starts_with("en")
                && !parent.is_empty();
            if !can_retry_macos_parent {
                return Err(primary_err);
            }

            let vlan_suffix = parent
                .split_once('.')
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty());
            let detected_base = detect_local_ipvlan_parent_base(runner, docker).await;
            let mapped_parent = if let Some(base) = detected_base {
                if let Some(vlan) = &vlan_suffix {
                    format!("{base}.{vlan}")
                } else {
                    base
                }
            } else if let Some(vlan) = &vlan_suffix {
                format!("eth0.{vlan}")
            } else {
                "eth0".to_string()
            };

            let from_quoted = shell_single_quote(&format!("parent={parent}"));
            let to_quoted = shell_single_quote(&format!("parent={mapped_parent}"));
            let from_plain = format!("parent={parent}");
            let to_plain = format!("parent={mapped_parent}");
            let retry_cmd = create_cmd
                .replace(&from_quoted, &to_quoted)
                .replace(&from_plain, &to_plain);

            match runner.run(&retry_cmd).await {
                Ok(out) => Ok(format!(
                    "{}\n(macos local ipvlan parent remapped: {} -> {})",
                    out.trim(),
                    parent,
                    mapped_parent
                )
                .trim()
                .to_string()),
                Err(_) => Err(primary_err),
            }
        }
    }
}

pub(in crate::ui) async fn detect_local_ipvlan_parent_base(
    runner: &Runner,
    docker: &DockerCfg,
) -> Option<String> {
    if !matches!(runner, Runner::Local) {
        return None;
    }
    let docker_cmd = docker.docker_cmd.to_shell();
    if docker_cmd.trim().is_empty() {
        return None;
    }
    let cmd = format!(
        "{docker_cmd} network inspect $({docker_cmd} network ls -q) --format '{{{{.Driver}}}} {{{{index .Options \"parent\"}}}}' 2>/dev/null || true"
    );
    let out = runner.run(&cmd).await.ok()?;
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let drv = it.next().unwrap_or("");
        let parent = it.next().unwrap_or("");
        if drv != "ipvlan" || parent.is_empty() || parent == "<no" {
            continue;
        }
        let base = parent.split('.').next().unwrap_or(parent).trim();
        if !base.is_empty() {
            return Some(base.to_string());
        }
    }
    None
}
