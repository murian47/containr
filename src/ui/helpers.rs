//! Small UI helpers shared across commands/actions.

use std::fs;
use std::path::PathBuf;
use serde_json::Value;
use crate::config::ServerEntry;

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

pub(in crate::ui) fn shell_quote_with_home(s: &str) -> String {
    if s.starts_with("$HOME/") {
        format!("\"{s}\"")
    } else {
        shell_single_quote(s)
    }
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

pub(in crate::ui) fn extract_container_ip(v: &Value) -> Option<String> {
    // Prefer user-defined networks.
    v.pointer("/NetworkSettings/Networks")
        .and_then(|n| n.as_object())
        .and_then(|map| {
            for (_name, net) in map {
                if let Some(ip) = net.get("IPAddress").and_then(|x| x.as_str()) {
                    let ip = ip.trim();
                    if !ip.is_empty() {
                        return Some(ip.to_string());
                    }
                }
            }
            None
        })
        .or_else(|| {
            v.pointer("/NetworkSettings/IPAddress")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

pub(in crate::ui) fn normalize_image_id(id: &str) -> String {
    let s = id.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.starts_with("sha256:") {
        return s.to_string();
    }
    format!("sha256:{s}")
}

pub(in crate::ui) fn build_server_shortcuts(servers: &[ServerEntry]) -> Vec<char> {
    // First 1..9 use digits. Remaining use deterministic "random-looking" uppercase letters.
    let mut out: Vec<char> = Vec::with_capacity(servers.len());
    let mut used: std::collections::HashSet<char> = std::collections::HashSet::new();

    for (i, _) in servers.iter().enumerate() {
        if i < 9 {
            let ch = char::from_digit((i + 1) as u32, 10).unwrap_or('?');
            out.push(ch);
            used.insert(ch);
        } else {
            out.push('\0');
        }
    }

    // Avoid letters that could be confused with common module letters in uppercase.
    for ch in ['C', 'S', 'M', 'I', 'V', 'N', 'L'] {
        used.insert(ch);
    }

    let pool: Vec<char> = ('A'..='Z').filter(|c| !used.contains(c)).collect();
    if pool.is_empty() {
        for ch in out.iter_mut().skip(9) {
            *ch = 'A';
        }
        return out;
    }

    // Stable assignment based on server name.
    for i in 9..servers.len() {
        let name = &servers[i].name;
        let mut h: u64 = 0xcbf29ce484222325;
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let start = (h as usize) % pool.len();
        let mut chosen = None;
        for off in 0..pool.len() {
            let c = pool[(start + off) % pool.len()];
            if !used.contains(&c) {
                chosen = Some(c);
                break;
            }
        }
        let c = chosen.unwrap_or(pool[start]);
        out[i] = c;
        used.insert(c);
    }
    out
}

pub(in crate::ui) fn truncate_msg(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max.saturating_sub(3) {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}
