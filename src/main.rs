mod config;
mod docker;
mod runner;
mod shell_parse;
mod ssh;
mod ui;

use clap::Parser;
use runner::Runner;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "containr",
    about = "Local TUI dashboard for remote Docker via SSH"
)]
struct Args {
    #[arg(
        long,
        help = "SSH target, e.g. user@host or host (uses your ssh config)"
    )]
    target: Option<String>,

    #[arg(
        long,
        help = "Server name from ~/.config/containr/config.json (or $XDG_CONFIG_HOME)"
    )]
    server: Option<String>,

    #[arg(long, help = "Refresh interval in seconds (overrides config)")]
    refresh_secs: Option<u64>,

    #[arg(
        long,
        default_value = "docker",
        help = "Remote docker command to run (shell fragment), e.g. 'docker' or 'sudo docker'"
    )]
    docker_cmd: String,

    #[arg(long, help = "Optional SSH identity file (passed as -i)")]
    identity: Option<String>,

    #[arg(long, help = "Optional SSH port (passed as -p)")]
    port: Option<u16>,

    #[arg(
        long,
        default_value_t = false,
        help = "Force ASCII-only UI rendering (disable Unicode line art)"
    )]
    ascii_only: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Load server list from disk. When started with --target we will also
    // create/update the config to make the target selectable later.
    let config_path = config::config_path()?;
    let mut config = config::load_or_default(&config_path)?;

    let (
        runner,
        cfg,
        servers,
        keymap,
        active_server,
        refresh_secs,
        logs_tail,
        cmd_history_max,
        cmd_history,
        templates_dir,
        view_layout,
    ) = if let Some(target) = args.target.clone() {
        let parsed_docker_cmd = config::DockerCmd::from_shell(&args.docker_cmd)?;
        // "target mode": connect directly and (optionally) persist it as a named server.
        let desired_name = args
            .server
            .clone()
            .unwrap_or_else(|| derive_server_name(&target));

        // If the target already exists, use/update that entry. Otherwise create a new one.
        let selected_name: Option<String> =
            if let Some(existing) = config.servers.iter_mut().find(|s| s.target == target) {
                if args.port.is_some() {
                    existing.port = args.port;
                }
                if args.identity.is_some() {
                    existing.identity = args.identity.clone();
                }
                if args.docker_cmd != "docker" {
                    existing.docker_cmd = parsed_docker_cmd.clone();
                }
                Some(existing.name.clone())
            } else {
                let unique_name = make_unique_name(&desired_name, &config.servers);
                config.servers.push(config::ServerEntry {
                    name: unique_name.clone(),
                    target: target.clone(),
                    port: args.port,
                    identity: args.identity.clone(),
                    docker_cmd: parsed_docker_cmd.clone(),
                });
                Some(unique_name)
            };

        // Persist the list if it didn't exist yet or was extended.
        config.last_server = selected_name.clone();
        config::save(&config_path, &config)?;

        let runner = if target == "local" {
            Runner::Local
        } else {
            Runner::Ssh(ssh::Ssh {
                target,
                identity: args.identity,
                port: args.port,
            })
        };

        (
            runner,
            docker::DockerCfg {
                docker_cmd: parsed_docker_cmd,
            },
            config.servers.clone(),
            config.keymap.clone(),
            selected_name,
            config.refresh_secs,
            config.logs_tail,
            config.cmd_history_max,
            config.cmd_history.clone(),
            config.templates_dir.clone(),
            config.view_layout.clone(),
        )
    } else {
        // "server mode": resolve from config (or last_server).
        let wanted = args
            .server
            .clone()
            .or_else(|| config.last_server.clone())
            .or_else(|| config.servers.first().map(|s| s.name.clone()));

        if let Some(wanted) = wanted {
            let entry = config
                .servers
                .iter()
                .find(|s| s.name == wanted)
                .ok_or_else(|| {
                    anyhow::anyhow!("server '{}' not found in {}", wanted, config_path.display())
                })?
                .clone();

            let runner = if entry.target == "local" {
                Runner::Local
            } else {
                Runner::Ssh(ssh::Ssh {
                    target: entry.target,
                    identity: entry.identity,
                    port: entry.port,
                })
            };

            (
                runner,
                docker::DockerCfg {
                    docker_cmd: entry.docker_cmd,
                },
                config.servers.clone(),
                config.keymap.clone(),
                Some(wanted),
                config.refresh_secs,
                config.logs_tail,
                config.cmd_history_max,
                config.cmd_history.clone(),
                config.templates_dir.clone(),
                config.view_layout.clone(),
            )
        } else {
            (
                Runner::Local,
                docker::DockerCfg {
                    docker_cmd: config::DockerCmd::empty(),
                },
                config.servers.clone(),
                config.keymap.clone(),
                None,
                config.refresh_secs,
                config.logs_tail,
                config.cmd_history_max,
                config.cmd_history.clone(),
                config.templates_dir.clone(),
                config.view_layout.clone(),
            )
        }
    };

    let refresh_secs = args.refresh_secs.unwrap_or(refresh_secs).max(1);
    ui::run_tui(
        runner,
        cfg,
        Duration::from_secs(refresh_secs),
        logs_tail,
        cmd_history_max,
        cmd_history,
        templates_dir,
        view_layout,
        config.active_theme.clone(),
        servers,
        keymap,
        active_server,
        config_path,
        args.ascii_only,
        config.git_autocommit,
        config.git_autocommit_confirm,
        config.editor_cmd.clone(),
    )
    .await
}

fn derive_server_name(target: &str) -> String {
    // Produce a stable, human-readable default name from the SSH target.
    // user@host -> host, otherwise keep as-is; sanitize to a readable label.
    let base = target.rsplit('@').next().unwrap_or(target).trim();
    let mut out = String::new();
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "server".to_string()
    } else {
        out
    }
}

fn make_unique_name(desired: &str, servers: &[config::ServerEntry]) -> String {
    // Avoid name collisions in serverlist.json by suffixing "-N" if needed.
    if !servers.iter().any(|s| s.name == desired) {
        return desired.to_string();
    }
    for i in 2..=9999 {
        let candidate = format!("{}-{}", desired, i);
        if !servers.iter().any(|s| s.name == candidate) {
            return candidate;
        }
    }
    format!("{}-{}", desired, servers.len() + 1)
}
