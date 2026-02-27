//! Image commands (`:image ...` / `:img ...`).

use super::super::{ActionRequest, App};
use super::super::shell_begin_confirm;
use crate::domain::image_refs::{image_registry_for_ref, image_repo_name};
use tokio::sync::mpsc;

fn tag_from_ref(image_ref: &str) -> Option<String> {
    let name = image_ref.split_once('@').map(|(n, _)| n).unwrap_or(image_ref);
    match name.rsplit_once(':') {
        Some((_, tag)) if !tag.contains('/') => Some(tag.to_string()),
        _ => None,
    }
}

pub(in crate::ui) fn handle_image(
    app: &mut App,
    force: bool,
    cmdline_full: String,
    args: &[&str],
    action_req_tx: &mpsc::UnboundedSender<super::super::ActionRequest>,
) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "push" => {
            let mut registry: Option<String> = None;
            let mut repo: Option<String> = None;
            let mut tag: Option<String> = None;
            let mut image_ref: Option<String> = None;
            let mut i = 1usize;
            while i < args.len() {
                let arg = args[i];
                match arg {
                    "--registry" | "registry" => {
                        if let Some(v) = args.get(i + 1) {
                            registry = Some(v.to_string());
                            i += 1;
                        } else {
                            app.set_warn("usage: :image push [--registry <host>] [--repo <repo>] [--tag <tag>] [--image <ref>]");
                            return true;
                        }
                    }
                    "--repo" | "repo" => {
                        if let Some(v) = args.get(i + 1) {
                            repo = Some(v.to_string());
                            i += 1;
                        } else {
                            app.set_warn("usage: :image push [--registry <host>] [--repo <repo>] [--tag <tag>] [--image <ref>]");
                            return true;
                        }
                    }
                    "--tag" | "tag" => {
                        if let Some(v) = args.get(i + 1) {
                            tag = Some(v.to_string());
                            i += 1;
                        } else {
                            app.set_warn("usage: :image push [--registry <host>] [--repo <repo>] [--tag <tag>] [--image <ref>]");
                            return true;
                        }
                    }
                    "--image" | "image" => {
                        if let Some(v) = args.get(i + 1) {
                            image_ref = Some(v.to_string());
                            i += 1;
                        } else {
                            app.set_warn("usage: :image push [--registry <host>] [--repo <repo>] [--tag <tag>] [--image <ref>]");
                            return true;
                        }
                    }
                    _ => {
                        if !arg.starts_with('-') && image_ref.is_none() {
                            image_ref = Some(arg.to_string());
                        }
                    }
                }
                i += 1;
            }
            let registry = registry
                .or_else(|| app.registry_default_host())
                .or_else(|| {
                    if app.registries_cfg.registries.len() == 1 {
                        Some(app.registries_cfg.registries[0].host.clone())
                    } else {
                        app.registries_cfg
                            .registries
                            .get(app.registries_selected)
                            .map(|r| r.host.clone())
                    }
                });
            let Some(registry) = registry else {
                app.set_warn("no registry configured (use :registry add <host>)");
                return true;
            };
            let image_ref = image_ref.or_else(|| {
                app.selected_image()
                    .and_then(|img| App::image_row_ref(img))
            });
            let Some(image_ref) = image_ref else {
                app.set_warn("no image selected");
                return true;
            };
            let repo = repo.unwrap_or_else(|| {
                let base = image_repo_name(&image_ref);
                let reg = image_registry_for_ref(&base);
                let prefix = format!("{reg}/");
                if base.starts_with(&prefix) {
                    base.trim_start_matches(&prefix).to_string()
                } else {
                    base
                }
            });
            let tag = tag.unwrap_or_else(|| {
                if let Some(t) = tag_from_ref(&image_ref) {
                    if !t.trim().is_empty() && t != "<none>" {
                        return t;
                    }
                }
                if let Some(img) = app.selected_image() {
                    let t = img.tag.trim();
                    if t.is_empty() || t == "<none>" {
                        "latest".to_string()
                    } else {
                        t.to_string()
                    }
                } else {
                    "latest".to_string()
                }
            });
            let target_ref = format!("{}/{}:{}", registry, repo, tag);
            let auth = match app.registry_auth_for_host(&registry) {
                Ok(v) => Some(v),
                Err(e) => {
                    app.set_error(format!("{e:#}"));
                    return true;
                }
            };
            let marker_key = format!("push:{target_ref}");
            if app.image_action_inflight.contains_key(&marker_key) {
                app.set_warn("image push already in progress");
                return true;
            }
            let docker_cmd = super::super::current_docker_cmd_from_app(app);
            if docker_cmd.is_empty() {
                app.set_warn("no server configured");
                return true;
            }
            let now = std::time::Instant::now();
            app.image_action_inflight.insert(
                marker_key.clone(),
                super::super::SimpleMarker {
                    until: now + std::time::Duration::from_secs(300),
                },
            );
            let _ = action_req_tx.send(ActionRequest::ImagePush {
                marker_key,
                source_ref: image_ref,
                target_ref,
                registry_host: registry,
                auth,
            });
            app.set_info("pushing image");
        }
        "untag" => {
            if force {
                crate::ui::state::actions::exec_image_action(app, true, action_req_tx);
            } else {
                shell_begin_confirm(app, "image untag", cmdline_full);
            }
        }
        "rm" | "remove" | "delete" => {
            if force {
                crate::ui::state::actions::exec_image_action(app, false, action_req_tx);
            } else {
                shell_begin_confirm(app, "image rm", cmdline_full);
            }
        }
        _ => app.set_warn("usage: :image push [--registry <host>] [--repo <repo>] [--tag <tag>] [--image <ref>] | :image untag | :image rm"),
    }
    true
}
