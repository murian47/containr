//! Command-line dispatcher (`:` commands).

mod builtins;
mod stack;

use super::{
    container_cmd, dashboard_cmd, git_cmd, image_cmd, keymap_cmd, layout_cmd, logs_cmd,
    network_cmd, registry_cmd, server_cmd, set_cmd, sidebar_cmd, templates_cmd, theme_cmd,
    volume_cmd,
};
use crate::ui::core::requests::ActionRequest;
use crate::ui::core::requests::Connection;
use crate::ui::core::types::InspectTarget;
use crate::ui::state::app::App;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::watch;

pub(in crate::ui) use stack::stack_update;

pub(crate) fn parse_cmdline_tokens(input: &str) -> Result<Vec<String>, String> {
    crate::shell_parse::parse_shell_tokens(input)
}

struct CmdlineCtx<'a> {
    refresh_tx: &'a mpsc::UnboundedSender<()>,
    dash_refresh_tx: &'a mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &'a mpsc::UnboundedSender<()>,
    refresh_pause_tx: &'a watch::Sender<bool>,
    action_req_tx: &'a mpsc::UnboundedSender<ActionRequest>,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn execute_cmdline(
    app: &mut App,
    cmdline: &str,
    conn_tx: &watch::Sender<Connection>,
    refresh_tx: &mpsc::UnboundedSender<()>,
    dash_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_refresh_tx: &mpsc::UnboundedSender<()>,
    dash_all_enabled_tx: &watch::Sender<bool>,
    refresh_interval_tx: &watch::Sender<Duration>,
    refresh_pause_tx: &watch::Sender<bool>,
    image_update_limit_tx: &watch::Sender<usize>,
    inspect_req_tx: &mpsc::UnboundedSender<InspectTarget>,
    logs_req_tx: &mpsc::UnboundedSender<(String, usize)>,
    action_req_tx: &mpsc::UnboundedSender<ActionRequest>,
) {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return;
    }
    let cmdline = cmdline.trim_start_matches(':').trim();
    let cmdline_full = cmdline.to_string();

    let tokens = match parse_cmdline_tokens(cmdline) {
        Ok(v) => v,
        Err(e) => {
            app.set_warn(format!("invalid command line: {e}"));
            return;
        }
    };
    let mut it = tokens.iter().map(|s| s.as_str());
    let Some(cmd_raw) = it.next() else {
        return;
    };
    let (cmd, force) = if cmd_raw == "!" {
        let Some(next) = it.next() else {
            app.set_warn("usage: :! <command>");
            return;
        };
        (next, true)
    } else if let Some(rest) = cmd_raw.strip_prefix('!') {
        if rest.is_empty() {
            app.set_warn("usage: :! <command>");
            return;
        }
        (rest, true)
    } else if let Some(stripped) = cmd_raw.strip_suffix('!') {
        (stripped, true)
    } else {
        (cmd_raw, false)
    };

    let ctx = CmdlineCtx {
        refresh_tx,
        dash_refresh_tx,
        dash_all_refresh_tx,
        refresh_pause_tx,
        action_req_tx,
    };

    if builtins::handle_builtin_cmd(app, cmd, force, &mut it, &cmdline_full, &ctx) {
        return;
    }
    if stack::handle_stack_cmd(app, cmd, &mut it, force, &cmdline_full, &ctx) {
        return;
    }

    if cmd == "container" || cmd == "ctr" || cmd == "containers" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ =
            container_cmd::handle_container(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "image" || cmd == "img" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = image_cmd::handle_image(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "volume" || cmd == "vol" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = volume_cmd::handle_volume(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "network" || cmd == "net" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        let _ = network_cmd::handle_network(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "sidebar" {
        let sub = it.next().unwrap_or("toggle");
        let mut args: Vec<&str> = Vec::new();
        args.push(sub);
        args.extend(it);
        let _ = sidebar_cmd::handle_sidebar(app, &args);
        return;
    }

    if cmd == "ai" {
        if it.next().is_some() {
            app.set_warn("usage: :ai");
            return;
        }
        let _ = templates_cmd::handle_template_ai(app);
        return;
    }

    if cmd == "inspect" {
        app.enter_inspect(inspect_req_tx);
        return;
    }

    if cmd == "logs" {
        let sub = it.next().unwrap_or("");
        let mut args: Vec<&str> = Vec::new();
        if !sub.is_empty() {
            args.push(sub);
        }
        args.extend(it);
        if args.is_empty() && app.shell_view != crate::ui::state::shell_types::ShellView::Logs {
            app.enter_logs(logs_req_tx);
            return;
        }
        let _ = logs_cmd::handle_logs(app, &args, logs_req_tx);
        return;
    }

    if cmd == "set" {
        let args: Vec<&str> = it.collect();
        let _ = set_cmd::handle_set(
            app,
            &args,
            refresh_interval_tx,
            image_update_limit_tx,
            logs_req_tx,
        );
        return;
    }

    if cmd == "layout" {
        let sub = it.next().unwrap_or("toggle");
        let mut args: Vec<&str> = Vec::new();
        args.push(sub);
        args.extend(it);
        let _ = layout_cmd::handle_layout(app, &args);
        return;
    }

    if cmd == "templates" {
        let args: Vec<&str> = it.collect();
        let _ = templates_cmd::handle_templates(app, &args);
        return;
    }

    if cmd == "registries" {
        let args: Vec<&str> = it.collect();
        let _ = registry_cmd::handle_registries(app, &args);
        return;
    }

    if cmd == "template" || cmd == "tpl" {
        let args: Vec<&str> = it.collect();
        let _ =
            templates_cmd::handle_template(app, force, cmdline_full.clone(), &args, action_req_tx);
        return;
    }

    if cmd == "registry" || cmd == "reg" {
        let args: Vec<&str> = it.collect();
        let _ = registry_cmd::handle_registry(app, force, &args, action_req_tx);
        return;
    }

    if matches!(cmd, "nettemplate" | "nettpl" | "ntpl" | "nt") {
        let args: Vec<&str> = it.collect();
        let _ = templates_cmd::handle_nettemplate(
            app,
            force,
            cmdline_full.clone(),
            &args,
            action_req_tx,
        );
        return;
    }

    if cmd == "server" {
        let args: Vec<&str> = it.collect();
        let _ = server_cmd::handle_server(
            app,
            force,
            cmdline_full.clone(),
            &args,
            conn_tx,
            refresh_tx,
            dash_refresh_tx,
            dash_all_enabled_tx,
        );
        return;
    }

    if cmd == "dashboard" {
        let args: Vec<&str> = it.collect();
        let _ = dashboard_cmd::handle_dashboard(
            app,
            &args,
            conn_tx,
            refresh_tx,
            dash_refresh_tx,
            dash_all_refresh_tx,
            dash_all_enabled_tx,
        );
        return;
    }

    app.set_error(format!("unknown command: {cmd}"));
}
