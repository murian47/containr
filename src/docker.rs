use crate::config::DockerCmd;
use crate::runner::Runner;
use anyhow::Context as _;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct DockerCfg {
    // Shell fragment executed on the remote side, e.g. "docker" or "sudo docker".
    pub docker_cmd: DockerCmd,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PsRow {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "Labels")]
    pub labels: String,
    #[serde(rename = "Command")]
    pub command: String,
    #[serde(rename = "CreatedAt")]
    pub created_at: String,
    #[serde(rename = "RunningFor")]
    pub running_for: String,
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Ports")]
    pub ports: String,
    #[serde(rename = "Names")]
    pub names: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatsRow {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "CPUPerc")]
    pub cpu_perc: String,
    #[serde(rename = "MemUsage")]
    pub mem_usage: String,
    #[serde(rename = "MemPerc")]
    pub mem_perc: String,
    #[serde(rename = "NetIO")]
    pub net_io: String,
    #[serde(rename = "BlockIO")]
    pub block_io: String,
    #[serde(rename = "PIDs")]
    pub pids: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ContainerRow {
    pub id: String,
    pub name: String,
    pub image: String,
    pub labels: String,
    pub command: String,
    pub created_at: String,
    pub running_for: String,
    pub status: String,
    pub ports: String,
    pub cpu_perc: Option<String>,
    pub mem_usage: Option<String>,
    pub mem_perc: Option<String>,
    pub stats_name: Option<String>,
    pub net_io: Option<String>,
    pub block_io: Option<String>,
    pub pids: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageLsRow {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Repository")]
    pub repository: String,
    #[serde(rename = "Tag")]
    pub tag: String,
    #[serde(rename = "Digest")]
    pub digest: String,
    #[serde(rename = "CreatedSince")]
    pub created_since: String,
    #[serde(rename = "CreatedAt")]
    pub created_at: String,
    #[serde(rename = "Size")]
    pub size: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ImageRow {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub digest: String,
    pub created_since: String,
    pub created_at: String,
    pub size: String,
}

impl ImageRow {
    pub fn name(&self) -> String {
        if self.repository == "<none>" && self.tag == "<none>" {
            "<none>".to_string()
        } else if self.tag.is_empty() || self.tag == "<none>" {
            self.repository.clone()
        } else {
            format!("{}:{}", self.repository, self.tag)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VolumeLsRow {
    #[serde(rename = "Driver")]
    pub driver: String,
    #[serde(rename = "Name")]
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkLsRow {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Driver")]
    pub driver: String,
    #[serde(rename = "Scope")]
    pub scope: String,
    #[serde(rename = "Labels")]
    pub labels: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NetworkRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub labels: String,
}

fn parse_json_lines<T: for<'de> Deserialize<'de>>(text: &str) -> anyhow::Result<Vec<T>> {
    // Docker can output one JSON object per line using --format '{{json .}}'.
    let mut rows = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = serde_json::from_str::<T>(line)
            .with_context(|| format!("failed to parse json line {}: {}", idx + 1, line))?;
        rows.push(row);
    }
    Ok(rows)
}

pub fn containers_command(cfg: &DockerCfg) -> String {
    if cfg.docker_cmd.is_empty() {
        return String::new();
    }
    // Run "ps -a" and "stats --no-stream" in a single SSH session to reduce latency.
    // We separate the outputs using a unique marker line.
    const SPLIT: &str = "__MCDOC_SPLIT__";
    let cmd = cfg.docker_cmd.to_shell();
    format!(
        "{cmd} ps -a --no-trunc --format '{{{{json .}}}}'; echo {split}; {cmd} stats --no-stream --format '{{{{json .}}}}'",
        cmd = cmd,
        split = SPLIT
    )
}

pub fn overview_command(cfg: &DockerCfg) -> String {
    if cfg.docker_cmd.is_empty() {
        return String::new();
    }
    // Fetch containers + stats + images + volumes + networks in a single SSH session.
    // Outputs are separated using unique marker lines.
    const S1: &str = "__MCDOC_SPLIT_1__";
    const S2: &str = "__MCDOC_SPLIT_2__";
    const S3: &str = "__MCDOC_SPLIT_3__";
    const S4: &str = "__MCDOC_SPLIT_4__";
    let cmd = cfg.docker_cmd.to_shell();
    format!(
        "{cmd} ps -a --no-trunc --format '{{{{json .}}}}'; echo {s1}; \
         {cmd} stats --no-stream --format '{{{{json .}}}}'; echo {s2}; \
         {cmd} image ls --no-trunc --format '{{{{json .}}}}'; echo {s3}; \
         {cmd} volume ls --format '{{{{json .}}}}'; echo {s4}; \
         {cmd} network ls --no-trunc --format '{{{{json .}}}}'",
        cmd = cmd,
        s1 = S1,
        s2 = S2,
        s3 = S3,
        s4 = S4
    )
}

pub fn parse_containers_output(out: &str) -> anyhow::Result<Vec<ContainerRow>> {
    // Parse the combined output from containers_command() and join ps/stats rows.
    const SPLIT: &str = "__MCDOC_SPLIT__";
    let mut ps_part = String::new();
    let mut stats_part = String::new();
    let mut in_stats = false;
    for line in out.lines() {
        if line.trim() == SPLIT {
            in_stats = true;
            continue;
        }
        if in_stats {
            stats_part.push_str(line);
            stats_part.push('\n');
        } else {
            ps_part.push_str(line);
            ps_part.push('\n');
        }
    }

    if !in_stats {
        anyhow::bail!(
            "unexpected docker output (missing split marker); check remote shell/permissions"
        );
    }

    let ps_rows: Vec<PsRow> = parse_json_lines(&ps_part)?;
    let stats_rows: Vec<StatsRow> = parse_json_lines(&stats_part)?;

    let mut stats_by_id: HashMap<String, StatsRow> =
        stats_rows.into_iter().map(|r| (r.id.clone(), r)).collect();

    let mut out_rows = Vec::with_capacity(ps_rows.len());
    for ps in ps_rows {
        // docker ps uses long IDs with --no-trunc; docker stats may use short IDs.
        // Try: full ID, 12-char prefix, then name.
        let short_id = ps.id.chars().take(12).collect::<String>();
        let stats = stats_by_id
            .remove(&ps.id)
            .or_else(|| stats_by_id.remove(&short_id))
            .or_else(|| stats_by_id.remove(&ps.names));
        out_rows.push(ContainerRow {
            id: ps.id.clone(),
            name: ps.names.clone(),
            image: ps.image,
            labels: ps.labels,
            command: ps.command,
            created_at: ps.created_at,
            running_for: ps.running_for,
            status: ps.status,
            ports: ps.ports,
            cpu_perc: stats.as_ref().map(|s| s.cpu_perc.clone()),
            mem_usage: stats.as_ref().map(|s| s.mem_usage.clone()),
            mem_perc: stats.as_ref().map(|s| s.mem_perc.clone()),
            stats_name: stats.as_ref().map(|s| s.name.clone()),
            net_io: stats.as_ref().map(|s| s.net_io.clone()),
            block_io: stats.as_ref().map(|s| s.block_io.clone()),
            pids: stats.as_ref().map(|s| s.pids.clone()),
        });
    }

    out_rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out_rows)
}

pub fn parse_overview_output(
    out: &str,
) -> anyhow::Result<(
    Vec<ContainerRow>,
    Vec<ImageRow>,
    Vec<VolumeRow>,
    Vec<NetworkRow>,
)> {
    const S1: &str = "__MCDOC_SPLIT_1__";
    const S2: &str = "__MCDOC_SPLIT_2__";
    const S3: &str = "__MCDOC_SPLIT_3__";
    const S4: &str = "__MCDOC_SPLIT_4__";

    let mut part_ps = String::new();
    let mut part_stats = String::new();
    let mut part_images = String::new();
    let mut part_volumes = String::new();
    let mut part_networks = String::new();

    enum Section {
        Ps,
        Stats,
        Images,
        Volumes,
        Networks,
    }
    let mut section = Section::Ps;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed == S1 {
            section = Section::Stats;
            continue;
        }
        if trimmed == S2 {
            section = Section::Images;
            continue;
        }
        if trimmed == S3 {
            section = Section::Volumes;
            continue;
        }
        if trimmed == S4 {
            section = Section::Networks;
            continue;
        }

        match section {
            Section::Ps => {
                part_ps.push_str(line);
                part_ps.push('\n');
            }
            Section::Stats => {
                part_stats.push_str(line);
                part_stats.push('\n');
            }
            Section::Images => {
                part_images.push_str(line);
                part_images.push('\n');
            }
            Section::Volumes => {
                part_volumes.push_str(line);
                part_volumes.push('\n');
            }
            Section::Networks => {
                part_networks.push_str(line);
                part_networks.push('\n');
            }
        }
    }

    let combined = format!("{}__MCDOC_SPLIT__\n{}", part_ps, part_stats);
    let containers = parse_containers_output(&combined)?;

    let images_raw: Vec<ImageLsRow> = parse_json_lines(&part_images)?;
    let mut images: Vec<ImageRow> = images_raw
        .into_iter()
        .map(|r| ImageRow {
            id: r.id,
            repository: r.repository,
            tag: r.tag,
            digest: r.digest,
            created_since: r.created_since,
            created_at: r.created_at,
            size: r.size,
        })
        .collect();
    images.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));

    let volumes_raw: Vec<VolumeLsRow> = parse_json_lines(&part_volumes)?;
    let mut volumes: Vec<VolumeRow> = volumes_raw
        .into_iter()
        .map(|r| VolumeRow {
            name: r.name,
            driver: r.driver,
        })
        .collect();
    volumes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let networks_raw: Vec<NetworkLsRow> = parse_json_lines(&part_networks)?;
    let mut networks: Vec<NetworkRow> = networks_raw
        .into_iter()
        .map(|r| NetworkRow {
            id: r.id,
            name: r.name,
            driver: r.driver,
            scope: r.scope,
            labels: r.labels.unwrap_or_default(),
        })
        .collect();
    networks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok((containers, images, volumes, networks))
}

#[allow(dead_code)]
pub async fn fetch_containers(
    runner: &Runner,
    cfg: &DockerCfg,
) -> anyhow::Result<Vec<ContainerRow>> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let cmd = containers_command(cfg);
    let out = runner.run(&cmd).await?;
    parse_containers_output(&out)
}

#[allow(dead_code)]
pub async fn fetch_overview(
    runner: &Runner,
    cfg: &DockerCfg,
) -> anyhow::Result<(
    Vec<ContainerRow>,
    Vec<ImageRow>,
    Vec<VolumeRow>,
    Vec<NetworkRow>,
)> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let cmd = overview_command(cfg);
    let out = runner.run(&cmd).await?;
    parse_overview_output(&out)
}

pub async fn fetch_inspect(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    // Return a single JSON object (not an array) so the UI can render a tree view.
    let cmd = cfg.docker_cmd.to_shell();
    let inspect_cmd = format!(
        "{cmd} inspect {arg} --format '{{{{json .}}}}'",
        cmd = cmd,
        arg = shell_escape_arg(id_or_name)
    );
    let out = runner.run(&inspect_cmd).await?;
    let out = out.trim();
    if out.is_empty() {
        anyhow::bail!("empty inspect output");
    }
    Ok(out.to_string())
}

pub async fn fetch_image_inspect(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} image inspect {arg} --format '{{{{json .}}}}'",
        docker = docker,
        arg = shell_escape_arg(id_or_name)
    );
    let out = runner.run(&cmd).await?;
    let out = out.trim();
    if out.is_empty() {
        anyhow::bail!("empty image inspect output");
    }
    Ok(out.to_string())
}

pub async fn fetch_volume_inspect(
    runner: &Runner,
    cfg: &DockerCfg,
    name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} volume inspect {arg} --format '{{{{json .}}}}'",
        docker = docker,
        arg = shell_escape_arg(name)
    );
    let out = runner.run(&cmd).await?;
    let out = out.trim();
    if out.is_empty() {
        anyhow::bail!("empty volume inspect output");
    }
    Ok(out.to_string())
}

pub async fn fetch_network_inspect(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} network inspect {arg} --format '{{{{json .}}}}'",
        docker = docker,
        arg = shell_escape_arg(id_or_name)
    );
    let out = runner.run(&cmd).await?;
    let out = out.trim();
    if out.is_empty() {
        anyhow::bail!("empty network inspect output");
    }
    Ok(out.to_string())
}

pub async fn fetch_inspects(
    runner: &Runner,
    cfg: &DockerCfg,
    ids: &[String],
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    if ids.is_empty() {
        return Ok("[]".to_string());
    }
    let mut args = String::new();
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            args.push(' ');
        }
        args.push_str(&shell_escape_arg(id));
    }

    // Default output is a JSON array.
    let cmd = cfg.docker_cmd.to_shell();
    let inspect_cmd = format!("{cmd} inspect {args}", cmd = cmd, args = args);
    let out = runner.run(&inspect_cmd).await?;
    let out = out.trim();
    if out.is_empty() {
        anyhow::bail!("empty inspect output");
    }
    Ok(out.to_string())
}

pub async fn fetch_manifest_inspect(
    runner: &Runner,
    cfg: &DockerCfg,
    image: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} manifest inspect --verbose {arg}",
        docker = docker,
        arg = shell_escape_arg(image)
    );
    let out = runner.run(&cmd).await?;
    let out = out.trim();
    if out.is_empty() {
        anyhow::bail!("empty manifest inspect output");
    }
    Ok(out.to_string())
}

#[derive(Debug, Clone, Copy)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Remove,
}

pub async fn container_action(
    runner: &Runner,
    cfg: &DockerCfg,
    action: ContainerAction,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = match action {
        ContainerAction::Start => format!(
            "{docker} start {arg}",
            docker = docker,
            arg = shell_escape_arg(id_or_name)
        ),
        ContainerAction::Stop => format!(
            "{docker} stop {arg}",
            docker = docker,
            arg = shell_escape_arg(id_or_name)
        ),
        ContainerAction::Restart => format!(
            "{docker} restart {arg}",
            docker = docker,
            arg = shell_escape_arg(id_or_name)
        ),
        ContainerAction::Remove => format!(
            "{docker} rm -f {arg}",
            docker = docker,
            arg = shell_escape_arg(id_or_name)
        ),
    };
    let out = runner.run(&cmd).await?;
    Ok(out.trim().to_string())
}

pub async fn image_remove(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} image rm {arg}",
        docker = docker,
        arg = shell_escape_arg(id_or_name)
    );
    let out = runner.run(&cmd).await?;
    Ok(out.trim().to_string())
}

pub async fn image_remove_force(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} image rm -f {arg}",
        docker = docker,
        arg = shell_escape_arg(id_or_name)
    );
    let out = runner.run(&cmd).await?;
    Ok(out.trim().to_string())
}

pub async fn volume_remove(runner: &Runner, cfg: &DockerCfg, name: &str) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} volume rm {arg}",
        docker = docker,
        arg = shell_escape_arg(name)
    );
    let out = runner.run(&cmd).await?;
    Ok(out.trim().to_string())
}

pub async fn network_remove(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} network rm {arg}",
        docker = docker,
        arg = shell_escape_arg(id_or_name)
    );
    let out = runner.run(&cmd).await?;
    Ok(out.trim().to_string())
}

pub async fn fetch_logs(
    runner: &Runner,
    cfg: &DockerCfg,
    id_or_name: &str,
    tail: usize,
) -> anyhow::Result<String> {
    if cfg.docker_cmd.is_empty() {
        anyhow::bail!("no server configured");
    }
    // Tail only (default 500) to keep UI responsive over SSH.
    let docker = cfg.docker_cmd.to_shell();
    let cmd = format!(
        "{docker} logs --timestamps --tail {tail} {arg}",
        docker = docker,
        tail = tail,
        arg = shell_escape_arg(id_or_name)
    );
    Ok(runner.run(&cmd).await?)
}

fn shell_escape_arg(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
    {
        return text.to_string();
    }
    let escaped = text.replace('\'', r"'\''");
    format!("'{}'", escaped)
}
