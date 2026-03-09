//! Addon command handling.
//!
//! Addons are declared in the config and expose named commands. This module resolves
//! metadata, builds the JSON invocation payload, persists enablement state, and
//! dispatches executable commands via the background action queue.

use crate::config::{AddonCommandSpec, AddonEntry, ContainrConfig};
use crate::ui::core::requests::ActionRequest;
use crate::ui::render::utils::shell_escape_sh_arg;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ShellInteractive, ShellView, TemplatesKind};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct AddonInvocationPayload {
    protocol: String,
    addon_id: String,
    addon_name: String,
    command: String,
    args: Vec<String>,
    context: AddonInvocationContext,
}

#[derive(Debug, Serialize)]
struct AddonInvocationContext {
    active_server: Option<String>,
    active_view: String,
    shell_view: String,
    selected: Vec<AddonSelection>,
    templates_dir: String,
}

#[derive(Debug, Serialize)]
struct AddonSelection {
    kind: String,
    id: String,
    name: String,
}

#[derive(Debug, Clone)]
struct AddonLookup {
    addon: AddonEntry,
    command: AddonCommandSpec,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn handle_addon(
    app: &mut App,
    args: &[&str],
    action_req_tx: &tokio::sync::mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    if args.is_empty() {
        app.set_warn("usage: :addon [list|enable|disable|reload|check|run] ...");
        return true;
    }

    let sub = args[0];
    let rest = &args[1..];

    match sub {
        "list" => {
            handle_addon_list(app);
            true
        }
        "enable" => {
            handle_addon_set_enabled(app, rest, true);
            true
        }
        "disable" => {
            handle_addon_set_enabled(app, rest, false);
            true
        }
        "reload" => {
            handle_addon_reload(app);
            true
        }
        "check" => {
            handle_addon_check(app, rest);
            true
        }
        "run" => handle_addon_run(app, rest, action_req_tx),
        _ => {
            app.set_warn("usage: :addon [list|enable|disable|reload|check|run] ...");
            true
        }
    }
}

fn find_addon_command(app: &App, token: &str) -> Option<AddonLookup> {
    let target = token.to_ascii_lowercase();
    let mut candidates: Vec<AddonLookup> = Vec::new();

    for addon in app.addons.iter() {
        for cmd in &addon.commands {
            if normalize_addon_token(&cmd.name) == target {
                candidates.push(AddonLookup {
                    addon: addon.clone(),
                    command: cmd.clone(),
                });
            }
        }
    }

    if candidates.is_empty() {
        if let Some((addon, command)) = find_single_command_for_addon_id(app, token) {
            return Some(AddonLookup {
                addon: addon.clone(),
                command: command.clone(),
            });
        }
        return None;
    }
    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }
    None
}

pub(in crate::ui) fn has_addon_command(app: &App, token: &str) -> bool {
    find_addon_command(app, token).is_some()
}

fn handle_addon_list(app: &mut App) {
    let mut count_enabled = 0usize;
    let mut count_disabled = 0usize;
    let mut lines: Vec<String> = Vec::new();
    for addon in &app.addons {
        if addon.enabled {
            count_enabled = count_enabled.saturating_add(1);
        } else {
            count_disabled = count_disabled.saturating_add(1);
        }
        let status = if addon.enabled { "enabled" } else { "disabled" };
        let version = addon.version.trim();
        let name = if addon.name.trim().is_empty() {
            addon.id.trim()
        } else {
            addon.name.trim()
        };
        let mut lines_for_addon: Vec<String> = vec![format!(
            "{:<20} {:<8} commands={} version={version}",
            name,
            status,
            addon.commands.len()
        )];
        for cmd in &addon.commands {
            let cmd_name = if cmd.name.trim().is_empty() {
                "<unnamed>"
            } else {
                cmd.name.trim()
            };
            let action = cmd.action.as_deref().unwrap_or("<no-action>");
            lines_for_addon.push(format!("  - {cmd_name}: {action}"));
        }
        lines.extend(lines_for_addon);
    }

    if lines.is_empty() {
        app.set_warn("no addons configured");
        return;
    }
    for line in lines {
        app.log_msg(crate::ui::state::shell_types::MsgLevel::Info, line);
    }
    app.set_info(format!(
        "addons: {count_enabled} enabled, {count_disabled} disabled"
    ));
}

fn handle_addon_set_enabled(app: &mut App, args: &[&str], enabled: bool) {
    let token = match args.first() {
        Some(s) if !s.trim().is_empty() => normalize_addon_token(s),
        _ => {
            let action = if enabled { "enable" } else { "disable" };
            app.set_warn(format!("usage: :addon {action} <addon_id>"));
            return;
        }
    };

    let Some(addon_idx) = app
        .addons
        .iter()
        .position(|a| normalize_addon_token(&a.id) == token)
    else {
        app.set_warn(format!("addon not found: {token}"));
        return;
    };

    let addon_name = if app.addons[addon_idx].name.trim().is_empty() {
        app.addons[addon_idx].id.clone()
    } else {
        app.addons[addon_idx].name.clone()
    };

    if app.addons[addon_idx].enabled == enabled {
        app.set_info(if enabled {
            format!(
                "addon already enabled: {}",
                if addon_name.is_empty() {
                    token.as_str()
                } else {
                    addon_name.as_str()
                }
            )
        } else {
            format!(
                "addon already disabled: {}",
                if addon_name.is_empty() {
                    token.as_str()
                } else {
                    addon_name.as_str()
                }
            )
        });
        return;
    }

    app.addons[addon_idx].enabled = enabled;
    app.persist_config();
    app.set_info(format!(
        "addon {} {}",
        if addon_name.is_empty() {
            token
        } else {
            addon_name
        },
        if enabled { "enabled" } else { "disabled" }
    ));
}

fn handle_addon_reload(app: &mut App) {
    match ContainrConfig::load_or_default(&app.config_path) {
        Ok(cfg) => {
            app.addons = cfg.addons;
            app.set_info("addons reloaded from config");
        }
        Err(e) => {
            app.set_warn(format!("failed to reload addons: {e:#}"));
        }
    }
}

fn handle_addon_check(app: &mut App, args: &[&str]) {
    if let [id_or_token] = args {
        if *id_or_token == "--all" {
            app.set_info("addon check: all addons requested");
            let mut all_ok = true;
            for addon in &app.addons {
                if !check_addon_invokable(addon) {
                    all_ok = false;
                }
            }
            if all_ok {
                app.set_info("addon check: all ok");
            }
            return;
        }
        if let Some(addon) = app
            .addons
            .iter()
            .find(|a| normalize_addon_token(&a.id) == normalize_addon_token(id_or_token))
        {
            if check_addon_invokable(addon) {
                app.set_info(format!("addon '{}' check ok", addon.id));
            } else {
                app.set_warn(format!("addon '{}' check failed", addon.id));
            }
            return;
        }
        app.set_warn(format!("addon not found: {id_or_token}"));
        return;
    }

    let checks: Vec<(String, bool)> = app
        .addons
        .iter()
        .filter(|a| a.enabled)
        .map(|addon| (addon.id.clone(), check_addon_invokable(addon)))
        .collect();

    let mut has_error = false;
    for (id, ok) in checks {
        if !ok {
            has_error = true;
            app.set_warn(format!("addon '{}' check failed", id));
        }
    }
    if !has_error {
        app.set_info("addon check: enabled addons ok");
    }
}

pub(in crate::ui) fn handle_addon_run(
    app: &mut App,
    args: &[&str],
    action_req_tx: &tokio::sync::mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    let Some(command_name) = args.first() else {
        app.set_warn("usage: :addon run <command>");
        return true;
    };
    let Some(lookup) = find_addon_command(app, command_name) else {
        app.set_warn(format!("unknown addon command: {command_name}"));
        return true;
    };
    if !lookup.addon.enabled {
        app.set_warn(format!(
            "addon disabled: {}",
            if lookup.addon.name.trim().is_empty() {
                &lookup.addon.id
            } else {
                &lookup.addon.name
            }
        ));
        return true;
    }

    handle_addon_command_with_lookup(app, lookup, args.get(1..).unwrap_or(&[]), action_req_tx)
}

pub(in crate::ui) fn handle_addon_command(
    app: &mut App,
    args: &[&str],
    action_req_tx: &tokio::sync::mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    let Some(command_name) = args.first() else {
        app.set_warn("usage: <addon command> ...");
        return true;
    };
    let Some(lookup) = find_addon_command(app, command_name) else {
        app.set_warn(format!("unknown addon command: {command_name}"));
        return true;
    };
    handle_addon_command_with_lookup(app, lookup, args.get(1..).unwrap_or(&[]), action_req_tx)
}

fn handle_addon_command_with_lookup(
    app: &mut App,
    lookup: AddonLookup,
    args: &[&str],
    action_req_tx: &tokio::sync::mpsc::UnboundedSender<ActionRequest>,
) -> bool {
    if !lookup.addon.enabled {
        app.set_warn(format!(
            "addon disabled: {}",
            if lookup.addon.name.trim().is_empty() {
                &lookup.addon.id
            } else {
                &lookup.addon.name
            }
        ));
        return true;
    }
    let cmd_args: Vec<String> = args.iter().map(std::string::ToString::to_string).collect();
    if let Some((entry, payload)) =
        build_addon_invocation(app, &lookup.addon, &lookup.command, &cmd_args)
    {
        let template_edit_kind = capture_template_edit_snapshot_for_addon(app, &lookup.command);
        let payload_json = match serde_json::to_string(&payload) {
            Ok(s) => s,
            Err(e) => {
                app.set_warn(format!("failed to serialize addon payload: {e:#}"));
                return true;
            }
        };
        if payload_json.len() > lookup.addon.max_payload_bytes {
            app.set_warn("addon payload too large");
            return true;
        }
        if lookup.command.interactive {
            let cmd = build_interactive_addon_command(&entry, &payload_json);
            app.shell_pending_interactive = Some(ShellInteractive::RunLocalCommand { cmd });
            let addon_name = if lookup.addon.name.trim().is_empty() {
                &lookup.addon.id
            } else {
                lookup.addon.name.as_str()
            };
            app.set_info(format!(
                "starting interactive addon command '{}' for {addon_name}",
                lookup.command.name
            ));
            return true;
        }
        let request = ActionRequest::AddonRun {
            addon_id: payload.addon_id,
            command: payload.command,
            payload_json,
            entry,
            timeout_secs: lookup.addon.timeout_secs,
            env_allowlist: lookup.addon.env_allowlist.clone(),
            working_dir: resolve_working_dir(&lookup.addon.working_dir_policy, app),
            template_edit_kind,
        };
        let _ = action_req_tx.send(request);
        let addon_name = if lookup.addon.name.trim().is_empty() {
            &lookup.addon.id
        } else {
            lookup.addon.name.as_str()
        };
        app.set_info(format!(
            "running addon command '{}' for {addon_name}",
            lookup.command.name
        ));
        true
    } else {
        app.set_warn("addon command metadata incomplete");
        true
    }
}

fn build_interactive_addon_command(entry: &[String], payload_json: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push("printf %s".to_string());
    parts.push(shell_escape_sh_arg(payload_json));
    parts.push("|".to_string());
    for token in entry {
        parts.push(shell_escape_sh_arg(token));
    }
    parts.join(" ")
}

fn capture_template_edit_snapshot_for_addon(
    app: &mut App,
    cmd: &AddonCommandSpec,
) -> Option<TemplatesKind> {
    if cmd.capability.as_deref() != Some("template-edit") || app.shell_view != ShellView::Templates
    {
        return None;
    }
    match app.templates_state.kind {
        TemplatesKind::Stacks => {
            let tpl = app.selected_template()?;
            if !tpl.has_compose {
                return None;
            }
            let name = tpl.name.clone();
            app.capture_template_edit_snapshot(
                TemplatesKind::Stacks,
                name.clone(),
                tpl.compose_path.clone(),
            );
            app.templates_state.templates_refresh_after_edit = Some(name);
            Some(TemplatesKind::Stacks)
        }
        TemplatesKind::Networks => {
            let tpl = app.selected_net_template()?;
            if !tpl.has_cfg {
                return None;
            }
            let name = tpl.name.clone();
            app.capture_template_edit_snapshot(
                TemplatesKind::Networks,
                name.clone(),
                tpl.cfg_path.clone(),
            );
            app.templates_state.net_templates_refresh_after_edit = Some(name);
            Some(TemplatesKind::Networks)
        }
    }
}

fn build_addon_invocation(
    app: &App,
    addon: &AddonEntry,
    cmd: &AddonCommandSpec,
    args: &[String],
) -> Option<(Vec<String>, AddonInvocationPayload)> {
    let missing_action = cmd.action.as_ref().is_none_or(|a| a.trim().is_empty());
    if addon.entry.is_empty() || missing_action {
        return None;
    }
    let context = addon_context(app);
    Some((
        addon.entry.clone(),
        AddonInvocationPayload {
            protocol: crate::config::addon_protocol(),
            addon_id: addon.id.clone(),
            addon_name: if addon.name.trim().is_empty() {
                addon.id.clone()
            } else {
                addon.name.clone()
            },
            command: cmd.name.clone(),
            args: args.to_vec(),
            context,
        },
    ))
}

fn addon_context(app: &App) -> AddonInvocationContext {
    let mut selected = Vec::new();
    if let Some(container) = app.selected_container() {
        selected.push(AddonSelection {
            kind: "container".to_string(),
            id: container.id.clone(),
            name: container.name.clone(),
        });
    }
    if let Some(image) = app.selected_image() {
        selected.push(AddonSelection {
            kind: "image".to_string(),
            id: image.id.clone(),
            name: image.name(),
        });
    }
    if let Some(volume) = app.selected_volume() {
        selected.push(AddonSelection {
            kind: "volume".to_string(),
            id: volume.name.clone(),
            name: volume.name.clone(),
        });
    }
    if let Some(network) = app.selected_network() {
        selected.push(AddonSelection {
            kind: "network".to_string(),
            id: network.id.clone(),
            name: network.name.clone(),
        });
    }
    if let Some(stack) = app.selected_stack_entry() {
        selected.push(AddonSelection {
            kind: "stack".to_string(),
            id: stack.name.clone(),
            name: stack.name.clone(),
        });
    }
    if let Some(tpl) = app.selected_template() {
        selected.push(AddonSelection {
            kind: "template".to_string(),
            id: tpl.name.clone(),
            name: tpl.name.clone(),
        });
    }
    if let Some(net_tpl) = app.selected_net_template() {
        selected.push(AddonSelection {
            kind: "net_template".to_string(),
            id: net_tpl.name.clone(),
            name: net_tpl.name.clone(),
        });
    }

    AddonInvocationContext {
        active_server: app.active_server.clone(),
        active_view: active_view_label(app.shell_view),
        shell_view: app.shell_view.title().to_string(),
        selected,
        templates_dir: app.templates_state.dir.to_string_lossy().to_string(),
    }
}

fn check_addon_invokable(addon: &AddonEntry) -> bool {
    if addon.entry.is_empty() {
        return false;
    }
    let exe = &addon.entry[0];
    if exe.is_empty() {
        return false;
    }
    let exists = std::path::Path::new(exe).exists();
    exists && addon.protocol == crate::config::addon_protocol()
}

fn find_single_command_for_addon_id<'a>(
    app: &'a App,
    token: &str,
) -> Option<(&'a AddonEntry, &'a AddonCommandSpec)> {
    let token = normalize_addon_token(token);
    app.addons
        .iter()
        .find(|addon| normalize_addon_token(&addon.id) == token)
        .and_then(|addon| addon.commands.first().map(|cmd| (addon, cmd)))
}

fn resolve_working_dir(policy: &str, app: &App) -> Option<PathBuf> {
    let policy = policy.trim().to_ascii_lowercase();
    match policy.as_str() {
        "" | "auto" => None,
        "templates" => Some(app.templates_state.dir.clone()),
        "config" => app.config_path.parent().map(PathBuf::from),
        _ => Some(PathBuf::from(policy)),
    }
}

fn normalize_addon_token(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn active_view_label(view: ShellView) -> String {
    match view {
        ShellView::Dashboard => "dashboard".to_string(),
        ShellView::Stacks => "stacks".to_string(),
        ShellView::Containers => "containers".to_string(),
        ShellView::Images => "images".to_string(),
        ShellView::Volumes => "volumes".to_string(),
        ShellView::Networks => "networks".to_string(),
        ShellView::Templates => "templates".to_string(),
        ShellView::Registries => "registries".to_string(),
        ShellView::Inspect => "inspect".to_string(),
        ShellView::Logs => "logs".to_string(),
        ShellView::Help => "help".to_string(),
        ShellView::Messages => "messages".to_string(),
        ShellView::ThemeSelector => "themes".to_string(),
    }
}

impl ContainrConfig {
    fn load_or_default(path: &Path) -> anyhow::Result<ContainrConfig> {
        crate::config::load_or_default(path)
    }
}
