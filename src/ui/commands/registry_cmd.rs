//! Registry commands (`:registry ...`, `:registries ...`).

use super::super::{
    ActionRequest, App, ShellSidebarItem, ShellView, expand_user_path, ensure_age_identity,
    encrypt_age_secret, shell_begin_confirm, shell_set_main_view, shell_sidebar_select_item,
};
use crate::config::{RegistryAuth, RegistryEntry};
use std::fs;
use tokio::sync::mpsc;

fn focus_registries(app: &mut App) {
    shell_set_main_view(app, ShellView::Registries);
    shell_sidebar_select_item(app, ShellSidebarItem::Module(ShellView::Registries));
}

fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn registry_index(app: &App, host: &str) -> Option<usize> {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    app.registries_cfg
        .registries
        .iter()
        .position(|r| r.host.trim().eq_ignore_ascii_case(&host))
}

fn sort_registries(app: &mut App) {
    app.registries_cfg
        .registries
        .sort_by(|a, b| a.host.to_ascii_lowercase().cmp(&b.host.to_ascii_lowercase()));
}

fn registry_auth_from_str(v: &str) -> Option<RegistryAuth> {
    match v.trim().to_ascii_lowercase().as_str() {
        "anon" | "anonymous" => Some(RegistryAuth::Anonymous),
        "basic" => Some(RegistryAuth::Basic),
        "bearer" | "bearer-token" | "token" => Some(RegistryAuth::BearerToken),
        "github" | "github-pat" | "gh" | "ghcr" => Some(RegistryAuth::GithubPat),
        _ => None,
    }
}

fn normalize_test_repo(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches('/');
    let raw = raw.split('@').next().unwrap_or(raw);
    let raw = raw.split(':').next().unwrap_or(raw);
    raw.to_string()
}

fn ensure_default_identity_path(app: &mut App) -> String {
    if app.registries_cfg.age_identity.trim().is_empty() {
        app.registries_cfg.age_identity = "~/.config/containr/age.key".to_string();
    }
    app.registries_cfg.age_identity.clone()
}

pub fn handle_registries(app: &mut App, args: &[&str]) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "" | "view" => {
            focus_registries(app);
            true
        }
        "list" => {
            let names: Vec<String> = app
                .registries_cfg
                .registries
                .iter()
                .map(|r| r.host.clone())
                .collect();
            if names.is_empty() {
                app.set_info("registries: none");
            } else {
                app.set_info(format!("registries: {}", names.join(", ")));
            }
            true
        }
        "identity" => {
            let rest = args.get(1..).unwrap_or(&[]).join(" ").trim().to_string();
            if rest.is_empty() {
                let current = app.registries_cfg.age_identity.trim();
                if current.is_empty() {
                    app.set_info("registry identity: (unset)");
                } else {
                    app.set_info(format!("registry identity: {current}"));
                }
                return true;
            }
            app.registries_cfg.age_identity = rest;
            app.persist_registries();
            app.set_info("registry identity updated");
            true
        }
        _ => {
            app.set_warn("usage: :registries [view|list|identity <path>]");
            true
        }
    }
}

pub fn handle_registry(
    app: &mut App,
    force: bool,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "add" => {
            let Some(host_raw) = args.get(1).copied() else {
                app.set_warn("usage: :registry add <host>");
                return true;
            };
            let Some(host) = normalize_host(host_raw) else {
                app.set_warn("registry host must not be empty");
                return true;
            };
            if registry_index(app, &host).is_some() {
                app.set_warn("registry already exists");
                return true;
            }
            app.registries_cfg.registries.push(RegistryEntry {
                host: host.clone(),
                auth: RegistryAuth::Anonymous,
                username: None,
                secret: None,
                test_repo: None,
            });
            sort_registries(app);
            app.registries_selected = registry_index(app, &host).unwrap_or(0);
            app.registries_details_scroll = 0;
            app.persist_registries();
            focus_registries(app);
            app.set_info(format!("registry added: {host}"));
            true
        }
        "rm" | "remove" | "del" => {
            let Some(host_raw) = args.get(1).copied() else {
                app.set_warn("usage: :registry rm[!] <host>");
                return true;
            };
            let Some(host) = normalize_host(host_raw) else {
                app.set_warn("registry host must not be empty");
                return true;
            };
            let Some(idx) = registry_index(app, &host) else {
                app.set_warn("registry not found");
                return true;
            };
            if !force {
                let cmdline = format!("registry rm {host}");
                shell_begin_confirm(app, cmdline.clone(), cmdline);
                return true;
            }
            app.registries_cfg.registries.remove(idx);
            if app
                .registries_cfg
                .default_registry
                .as_ref()
                .map(|h| h.eq_ignore_ascii_case(&host))
                .unwrap_or(false)
            {
                app.registries_cfg.default_registry = None;
            }
            if app.registries_selected >= app.registries_cfg.registries.len() {
                app.registries_selected = app.registries_cfg.registries.len().saturating_sub(1);
            }
            app.registries_details_scroll = 0;
            app.persist_registries();
            app.set_info(format!("registry removed: {host}"));
            true
        }
        "default" => {
            let host = if let Some(raw) = args.get(1).copied() {
                if matches!(raw, "none" | "clear" | "unset" | "-") {
                    String::new()
                } else {
                    normalize_host(raw).unwrap_or_default()
                }
            } else {
                app.registries_cfg
                    .registries
                    .get(app.registries_selected)
                    .map(|r| r.host.clone())
                    .unwrap_or_default()
            };
            if host.is_empty() {
                app.registries_cfg.default_registry = None;
                app.persist_registries();
                app.set_info("registry default cleared");
                return true;
            }
            let Some(idx) = registry_index(app, &host) else {
                app.set_warn("registry not found");
                return true;
            };
            let host = app.registries_cfg.registries[idx].host.clone();
            app.registries_cfg.default_registry = Some(host.clone());
            app.persist_registries();
            app.set_info(format!("registry default set: {host}"));
            true
        }
        "set" => {
            let Some(host_raw) = args.get(1).copied() else {
                app.set_warn("usage: :registry set <host> <field> <value>");
                return true;
            };
            let Some(host) = normalize_host(host_raw) else {
                app.set_warn("registry host must not be empty");
                return true;
            };
            let Some(idx) = registry_index(app, &host) else {
                app.set_warn("registry not found");
                return true;
            };
            let field = args.get(2).copied().unwrap_or("");
            let value = args.get(3..).unwrap_or(&[]).join(" ").trim().to_string();
            if field.is_empty() {
                app.set_warn("usage: :registry set <host> <auth|username|secret|secret-file|test-repo> <value>");
                return true;
            }
            match field {
                "auth" => {
                    let Some(auth) = registry_auth_from_str(&value) else {
                        app.set_warn("auth must be anonymous|basic|bearer|github");
                        return true;
                    };
                    let entry = &mut app.registries_cfg.registries[idx];
                    entry.auth = auth;
                    if matches!(entry.auth, RegistryAuth::Anonymous) {
                        entry.username = None;
                        entry.secret = None;
                    }
                    app.persist_registries();
                    app.set_info(format!("registry auth updated: {host}"));
                }
                "username" | "user" => {
                    let entry = &mut app.registries_cfg.registries[idx];
                    if value.is_empty()
                        || matches!(value.as_str(), "-" | "none" | "unset" | "clear")
                    {
                        entry.username = None;
                    } else {
                        entry.username = Some(value);
                    }
                    app.persist_registries();
                    app.set_info(format!("registry username updated: {host}"));
                }
                "secret" | "token" | "password" => {
                    if value.is_empty()
                        || matches!(value.as_str(), "-" | "none" | "unset" | "clear")
                    {
                        let entry = &mut app.registries_cfg.registries[idx];
                        entry.secret = None;
                        app.persist_registries();
                        app.set_info(format!("registry secret cleared: {host}"));
                        return true;
                    }
                    let identity_path = ensure_default_identity_path(app);
                    let path = expand_user_path(&identity_path);
                    let identity = match ensure_age_identity(&path) {
                        Ok(id) => id,
                        Err(e) => {
                            app.set_error(format!("{e:#}"));
                            return true;
                        }
                    };
                    let encrypted = match encrypt_age_secret(&value, &identity) {
                        Ok(v) => v,
                        Err(e) => {
                            app.set_error(format!("{e:#}"));
                            return true;
                        }
                    };
                    let entry = &mut app.registries_cfg.registries[idx];
                    entry.secret = Some(encrypted);
                    app.persist_registries();
                    app.set_info(format!("registry secret updated: {host}"));
                }
                "secret-file" | "token-file" => {
                    if value.is_empty() {
                        app.set_warn("usage: :registry set <host> secret-file <path>");
                        return true;
                    }
                    let path = expand_user_path(&value);
                    let raw = match fs::read_to_string(&path) {
                        Ok(v) => v,
                        Err(e) => {
                            app.set_error(format!(
                                "failed to read secret file {}: {e}",
                                path.display()
                            ));
                            return true;
                        }
                    };
                    let secret = raw.trim().to_string();
                    if secret.is_empty() {
                        app.set_warn("secret file is empty");
                        return true;
                    }
                    let identity_path = ensure_default_identity_path(app);
                    let id_path = expand_user_path(&identity_path);
                    let identity = match ensure_age_identity(&id_path) {
                        Ok(id) => id,
                        Err(e) => {
                            app.set_error(format!("{e:#}"));
                            return true;
                        }
                    };
                    let encrypted = match encrypt_age_secret(&secret, &identity) {
                        Ok(v) => v,
                        Err(e) => {
                            app.set_error(format!("{e:#}"));
                            return true;
                        }
                    };
                    let entry = &mut app.registries_cfg.registries[idx];
                    entry.secret = Some(encrypted);
                    app.persist_registries();
                    app.set_info(format!("registry secret updated: {host}"));
                }
                "test-repo" | "test_repo" | "testrepo" => {
                    let entry = &mut app.registries_cfg.registries[idx];
                    if value.is_empty()
                        || matches!(value.as_str(), "-" | "none" | "unset" | "clear")
                    {
                        entry.test_repo = None;
                    } else {
                        let repo = normalize_test_repo(&value);
                        if repo.is_empty() {
                            entry.test_repo = None;
                        } else {
                            entry.test_repo = Some(repo);
                        }
                    }
                    app.persist_registries();
                    app.set_info(format!("registry test repo updated: {host}"));
                }
                _ => {
                    app.set_warn("usage: :registry set <host> <auth|username|secret|secret-file|test-repo> <value>");
                }
            }
            true
        }
        "test" => {
            let (host, test_repo) = if let Some(raw) = args.get(1).copied() {
                let host = normalize_host(raw).unwrap_or_default();
                if host.is_empty() {
                    app.set_warn("usage: :registry test [host]");
                    return true;
                }
                let Some(idx) = registry_index(app, &host) else {
                    app.set_warn("registry not found");
                    return true;
                };
                let entry = &app.registries_cfg.registries[idx];
                (entry.host.clone(), entry.test_repo.clone())
            } else {
                let Some(entry) = app.registries_cfg.registries.get(app.registries_selected) else {
                    app.set_warn("no registry selected");
                    return true;
                };
                (entry.host.clone(), entry.test_repo.clone())
            };
            let auth = match app.registry_auth_for_host(&host) {
                Ok(v) => v,
                Err(e) => {
                    app.set_warn(format!("{e:#}"));
                    return true;
                }
            };
            app.set_info(format!("testing registry {host}"));
            let _ = action_req_tx.send(ActionRequest::RegistryTest {
                host,
                auth,
                test_repo,
            });
            true
        }
        "list" => {
            let names: Vec<String> = app
                .registries_cfg
                .registries
                .iter()
                .map(|r| r.host.clone())
                .collect();
            if names.is_empty() {
                app.set_info("registries: none");
            } else {
                app.set_info(format!("registries: {}", names.join(", ")));
            }
            true
        }
        "" => {
            focus_registries(app);
            true
        }
        _ => {
            app.set_warn("usage: :registry <add|rm[!]|set|test|default|list>");
            true
        }
    }
}
