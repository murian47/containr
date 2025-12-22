use super::*;
use anyhow::Context as _;
use crate::config::{ContainrConfig, DockerCmd, ServerEntry};
use crate::docker::{self, ContainerAction, DockerCfg};
use crate::runner::Runner;
use crate::ssh::Ssh;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IT_ENV: &str = "CONTAINR_IT";
const IT_TARGET_ENV: &str = "CONTAINR_IT_TARGET";

fn it_enabled() -> bool {
    matches!(std::env::var(IT_ENV).ok().as_deref(), Some("1"))
}

fn it_target() -> String {
    std::env::var(IT_TARGET_ENV).unwrap_or_default()
}

fn mk_temp_path(prefix: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos();
    dir.push(format!(
        "containr-it-{prefix}-{now}-{}",
        std::process::id()
    ));
    dir
}

fn mk_integration_app() -> App {
    let tmp = mk_temp_path("config");
    fs::create_dir_all(&tmp).unwrap();
    let config_path = tmp.join("config.json");
    let mut app = App::new(
        vec![ServerEntry {
            name: "it".to_string(),
            target: "ssh".to_string(),
            port: None,
            identity: None,
            docker_cmd: DockerCmd::default(),
        }],
        Vec::new(),
        None,
        config_path,
        HashMap::new(),
        "default".to_string(),
        theme::default_theme_spec(),
        false,
        false,
        String::new(),
        4,
        false,
        false,
    );
    let cfg = ContainrConfig::default();
    app.templates_state.dir = expand_user_path(&cfg.templates_dir);
    app
}

#[tokio::test(flavor = "multi_thread")]
async fn integration_templates_and_networks() -> anyhow::Result<()> {
    if !it_enabled() {
        eprintln!("skipping integration tests (set {IT_ENV}=1 to enable)");
        return Ok(());
    }
    let target = it_target();
    if target.trim().is_empty() {
        eprintln!("skipping integration tests (set {IT_TARGET_ENV} to target a host)");
        return Ok(());
    }

    let runner = Runner::Ssh(Ssh {
        target,
        identity: None,
        port: None,
    });
    let docker = DockerCfg {
        docker_cmd: DockerCmd::default(),
    };

    let mut app = mk_integration_app();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    let net_name = format!("containr-it-net-{now}");
    let stack_name = format!("containr-it-stack-{now}");
    let container_name = format!("containr-it-container-{now}");

    let net_dir = app.net_templates_dir().join(&net_name);
    fs::create_dir_all(&net_dir)?;
    let net_cfg = net_dir.join("network.json");
    let net_json = format!(
        r#"{{
  "description": "integration test network",
  "name": "{net_name}",
  "driver": "bridge"
}}
"#
    );
    fs::write(&net_cfg, net_json)?;

    let stacks_dir = app.stack_templates_dir();
    fs::create_dir_all(&stacks_dir)?;
    let compose = format!(
        r#"services:
  it:
    image: alpine:latest
    container_name: {container_name}
    command: ["sh","-c","sleep 3600"]
"#
    );
    let compose_path = write_stack_template_compose(&stacks_dir, &stack_name, &compose)?;

    let mut cleanup_errors: Vec<String> = Vec::new();
    let result = async {
        let deploy_net = perform_net_template_deploy(&runner, &docker, &net_name, &net_cfg, false)
            .await?;
        if deploy_net.trim() == "exists" {
            anyhow::bail!("network already exists: {net_name}");
        }
        let _ = docker::fetch_network_inspect(&runner, &docker, &net_name).await?;

        perform_template_deploy(&runner, &docker, &stack_name, &compose_path, false, false).await?;
        let containers = docker::fetch_containers(&runner, &docker).await?;
        let found = containers
            .iter()
            .find(|c| c.name == container_name)
            .map(|c| c.id.clone());
        let Some(container_id) = found else {
            anyhow::bail!("container not found: {container_name}");
        };

        let _ = docker::container_action(&runner, &docker, ContainerAction::Stop, &container_id)
            .await?;
        let _ = docker::container_action(&runner, &docker, ContainerAction::Start, &container_id)
            .await?;
        let _ = docker::container_action(
            &runner,
            &docker,
            ContainerAction::Restart,
            &container_id,
        )
        .await?;

        let inspect = docker::fetch_inspect(&runner, &docker, &container_id).await?;
        let _: serde_json::Value =
            serde_json::from_str(&inspect).context("inspect output was not JSON")?;

        let mut app_for_msgs = mk_integration_app();
        app_for_msgs.log_msg(MsgLevel::Info, "integration test message");
        let log_path = mk_temp_path("messages").join("messages.txt");
        app_for_msgs.messages_save(log_path.to_string_lossy().as_ref());
        let meta = fs::metadata(&log_path)?;
        anyhow::ensure!(meta.len() > 0, "messages file is empty");

        Ok::<_, anyhow::Error>(())
    }
    .await;

    if let Err(e) = docker::container_action(&runner, &docker, ContainerAction::Remove, &container_name).await {
        cleanup_errors.push(format!("container cleanup failed: {e:#}"));
    }
    if let Err(e) = docker::network_remove(&runner, &docker, &net_name).await {
        cleanup_errors.push(format!("network cleanup failed: {e:#}"));
    }
    let remote_stack_dir = deploy_remote_dir_for(&stack_name);
    let remote_net_dir = deploy_remote_net_dir_for(&net_name);
    if let Err(e) = runner.run(&format!("rm -rf {}", shell_single_quote(&remote_stack_dir))).await
    {
        cleanup_errors.push(format!("remote stack dir cleanup failed: {e:#}"));
    }
    if let Err(e) = runner.run(&format!("rm -rf {}", shell_single_quote(&remote_net_dir))).await {
        cleanup_errors.push(format!("remote net dir cleanup failed: {e:#}"));
    }
    if let Err(e) = delete_template(&mut app, &stack_name) {
        cleanup_errors.push(format!("local stack template cleanup failed: {e:#}"));
    }
    if let Err(e) = delete_net_template(&mut app, &net_name) {
        cleanup_errors.push(format!("local net template cleanup failed: {e:#}"));
    }

    if let Err(e) = result {
        if !cleanup_errors.is_empty() {
            eprintln!("cleanup warnings:\n{}", cleanup_errors.join("\n"));
        }
        return Err(e);
    }
    if !cleanup_errors.is_empty() {
        eprintln!("cleanup warnings:\n{}", cleanup_errors.join("\n"));
    }
    Ok(())
}
