use super::common::{extract_template_description_from_file, validate_template_name};
use crate::ui::render::highlight::split_yaml_comment;
use crate::ui::shell_utils::extract_template_id;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::TemplatesKind;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

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

pub(in crate::ui) fn create_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    validate_template_name(name)?;
    let name = name.trim();

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

    # Optional: labels (the app can add its own labels during deploy later)
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
    validate_template_name(name)?;
    let name = name.trim();

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
    validate_template_name(name)?;
    let name = name.trim();

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
    if let Some(info) = extract_template_id(&dir.join("compose.yaml"))
        && app.template_deploys.remove(&info).is_some()
    {
        app.save_local_state();
    }
    Ok(())
}

pub(in crate::ui) fn delete_net_template(app: &mut App, name: &str) -> anyhow::Result<()> {
    validate_template_name(name)?;
    let name = name.trim();

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

pub(in crate::ui) fn maybe_autocommit_templates(
    app: &mut App,
    kind: TemplatesKind,
    action: &str,
    name: &str,
) {
    crate::ui::render::git::maybe_autocommit_templates(app, kind, action, name)
}

pub(in crate::ui) fn extract_template_description(path: &Path) -> Option<String> {
    extract_template_description_from_file(path)
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
