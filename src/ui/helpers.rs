//! Small UI helpers shared across commands/actions.

use std::fs;
use std::path::PathBuf;

pub(in crate::ui) fn shell_single_quote(s: &str) -> String {
    // Produce a POSIX-shell-safe single-quoted string literal.
    // Example: abc'd -> 'abc'"'"'d'
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

pub(in crate::ui) fn extract_template_id(path: &PathBuf) -> Option<String> {
    // Heuristic: find a "# containr_template_id: ..." line near the top of compose.yaml.
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
        if !low.starts_with("containr_template_id:") {
            continue;
        }
        let value = body["containr_template_id:".len()..].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub(in crate::ui) fn ensure_template_id(path: &PathBuf) -> anyhow::Result<String> {
    if let Some(existing) = extract_template_id(path) {
        return Ok(existing);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let data = fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!("# containr_template_id: {id}\n"));
    out.push_str(&data);
    fs::write(path, out)?;
    Ok(id)
}

pub(in crate::ui) fn deploy_remote_dir_for(name: &str) -> String {
    format!(".config/containr/apps/{name}")
}

pub(in crate::ui) fn deploy_remote_net_dir_for(name: &str) -> String {
    format!(".config/containr/networks/{name}")
}

pub(in crate::ui) fn parse_kv_args(
    mut it: impl Iterator<Item = String>,
) -> (
    Option<u16>,
    Option<String>,
    Option<crate::config::DockerCmd>,
    Vec<String>,
) {
    // Supports: -p <port>  -i <identity>  --cmd <docker_cmd>
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut docker_cmd: Option<crate::config::DockerCmd> = None;
    let mut rest: Vec<String> = Vec::new();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "-p" => {
                if let Some(v) = it.next() {
                    port = v.parse::<u16>().ok();
                }
            }
            "-i" => {
                if let Some(v) = it.next() {
                    identity = Some(v);
                }
            }
            "--cmd" => {
                if let Some(v) = it.next() {
                    let parsed = crate::shell_parse::parse_shell_tokens(&v)
                        .ok()
                        .unwrap_or_else(|| vec![v]);
                    if parsed.is_empty() {
                        docker_cmd = Some(crate::config::DockerCmd::default());
                    } else {
                        docker_cmd = Some(crate::config::DockerCmd::new(parsed));
                    }
                }
            }
            _ => rest.push(tok),
        }
    }
    (port, identity, docker_cmd, rest)
}
