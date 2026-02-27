use crate::ui::state::app::App;
use crate::ui::state::shell_types::{MsgLevel, TemplatesKind, shell_begin_confirm};

pub(in crate::ui) fn maybe_autocommit_templates(
    app: &mut App,
    kind: TemplatesKind,
    action: &str,
    name: &str,
) {
    if !app.git_autocommit {
        return;
    }
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    if !crate::ui::commands::git_cmd::git_available() {
        return;
    }

    let dir = app.templates_state.dir.clone();
    if !crate::ui::commands::git_cmd::is_git_repo(&dir) {
        app.log_msg(
            MsgLevel::Warn,
            "git autocommit is enabled but templates repo is not initialized".to_string(),
        );
        return;
    }

    let status = match crate::ui::commands::git_cmd::run_git(&dir, &["status", "--porcelain"]) {
        Ok(out) => out,
        Err(e) => {
            app.log_msg(MsgLevel::Warn, format!("git autocommit skipped: {e:#}"));
            return;
        }
    };
    if status.trim().is_empty() {
        return;
    }

    let kind_label = match kind {
        TemplatesKind::Stacks => "stack",
        TemplatesKind::Networks => "network",
    };
    let msg = format!("templates: {action} {kind_label} {name}");

    if app.git_autocommit_confirm {
        let cmdline = format!("git templates autocommit -m {}", shell_escape_sh_arg(&msg));
        shell_begin_confirm(app, "git autocommit", cmdline);
        return;
    }

    if let Err(e) = crate::ui::commands::git_cmd::run_git(&dir, &["add", "-A"]) {
        app.log_msg(MsgLevel::Warn, format!("git autocommit failed: {e:#}"));
        return;
    }

    match crate::ui::commands::git_cmd::run_git(&dir, &["commit", "-m", msg.as_str()]) {
        Ok(out) => {
            if out.trim().is_empty() {
                app.log_msg(MsgLevel::Info, format!("git autocommit: {msg}"));
            } else {
                app.log_msg(MsgLevel::Info, format!("git autocommit: {out}"));
            }
        }
        Err(e) => app.log_msg(MsgLevel::Warn, format!("git autocommit failed: {e:#}")),
    }
}

fn shell_escape_sh_arg(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/:@".contains(c))
    {
        return text.to_string();
    }
    let escaped = text.replace('\'', r"'\''");
    format!("'{}'", escaped)
}
