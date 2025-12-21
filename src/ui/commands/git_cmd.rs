//! Git commands for templates/themes (`:git ...`).

use super::super::{App, ShellFocus, ShellView, set_text_and_cursor};
use crate::ui::theme;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitContext {
    Templates,
    Themes,
}

fn parse_context(raw: &str) -> Option<GitContext> {
    match raw {
        "templates" => Some(GitContext::Templates),
        "themes" => Some(GitContext::Themes),
        _ => None,
    }
}

fn context_dir(app: &App, ctx: GitContext) -> PathBuf {
    match ctx {
        GitContext::Templates => app.templates_state.dir.clone(),
        GitContext::Themes => theme::themes_dir_from_config_path(&app.config_path),
    }
}

fn is_dir_empty(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut it) => it.next().is_none(),
        Err(_) => true,
    }
}

fn templates_dir_is_empty(root: &Path) -> bool {
    let mut entries = match std::fs::read_dir(root) {
        Ok(it) => it.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(_) => return true,
    };
    if entries.is_empty() {
        return true;
    }
    for ent in entries.drain(..) {
        let name = ent.file_name().to_string_lossy().to_string();
        if name != "stacks" && name != "networks" {
            return false;
        }
        let path = ent.path();
        if path.is_dir() && !is_dir_empty(&path) {
            return false;
        }
    }
    true
}

fn remove_empty_templates_scaffold(root: &Path) -> anyhow::Result<()> {
    for name in ["stacks", "networks"] {
        let path = root.join(name);
        if path.is_dir() && is_dir_empty(&path) {
            let _ = std::fs::remove_dir(&path);
        }
    }
    Ok(())
}

fn run_git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to start git: {}", e))?;
    let mut text = String::new();
    if !out.stdout.is_empty() {
        text.push_str(&String::from_utf8_lossy(&out.stdout));
    }
    if !out.stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    if out.status.success() {
        Ok(text.trim_end().to_string())
    } else {
        let msg = text.trim_end();
        if msg.is_empty() {
            Err(anyhow::anyhow!("git failed with status {}", out.status))
        } else {
            Err(anyhow::anyhow!("{msg}"))
        }
    }
}

fn show_git_output(app: &mut App, title: &str, output: &str) {
    app.set_info(title.to_string());
    if output.trim().is_empty() {
        app.set_info("(no output)".to_string());
    } else {
        for line in output.lines() {
            app.set_info(line.to_string());
        }
    }
    app.shell_msgs.return_view = app.shell_view;
    app.shell_view = ShellView::Messages;
    app.shell_focus = ShellFocus::List;
    app.shell_msgs.scroll = usize::MAX;
}

pub fn handle_git(app: &mut App, args: &[&str]) -> bool {
    let ctx_raw = args.first().copied().unwrap_or("");
    let Some(ctx) = parse_context(ctx_raw) else {
        app.set_warn("usage: :git <templates|themes> <status|diff|log|commit|pull|push|init|clone> ...");
        return true;
    };
    let sub = args.get(1).copied().unwrap_or("");
    let rest = &args.get(2..).unwrap_or(&[]);
    let dir = context_dir(app, ctx);

    match sub {
        "status" => {
            let _ = std::fs::create_dir_all(&dir);
            match run_git(&dir, &["status", "-sb"]) {
                Ok(out) => show_git_output(app, "git status", &out),
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "diff" => {
            let _ = std::fs::create_dir_all(&dir);
            match run_git(&dir, &["diff"]) {
                Ok(out) => show_git_output(app, "git diff", &out),
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "log" => {
            let _ = std::fs::create_dir_all(&dir);
            match run_git(&dir, &["log", "--oneline", "-n", "20"]) {
                Ok(out) => show_git_output(app, "git log", &out),
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "pull" => {
            let _ = std::fs::create_dir_all(&dir);
            match run_git(&dir, &["pull", "--rebase"]) {
                Ok(out) => show_git_output(app, "git pull --rebase", &out),
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "push" => {
            let _ = std::fs::create_dir_all(&dir);
            match run_git(&dir, &["push"]) {
                Ok(out) => show_git_output(app, "git push", &out),
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "commit" => {
            let mut msg: Option<String> = None;
            let mut i = 0usize;
            while i < rest.len() {
                if rest[i] == "-m" {
                    msg = rest.get(i + 1).map(|s| (*s).to_string());
                    break;
                }
                i += 1;
            }
            if msg.as_deref().unwrap_or("").trim().is_empty() {
                // Prompt for a commit message.
                app.shell_cmdline.mode = true;
                let prompt = format!("git commit {ctx_raw} -m ");
                set_text_and_cursor(&mut app.shell_cmdline.input, &mut app.shell_cmdline.cursor, prompt);
                app.shell_cmdline.confirm = None;
                return true;
            }
            let _ = std::fs::create_dir_all(&dir);
            let msg = msg.unwrap();
            match run_git(&dir, &["commit", "-m", msg.as_str()]) {
                Ok(out) => show_git_output(app, "git commit", &out),
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "init" => {
            if ctx == GitContext::Templates {
                if !templates_dir_is_empty(&dir) {
                    app.set_warn("templates dir is not empty; git init blocked");
                    return true;
                }
            } else if dir.exists() && !is_dir_empty(&dir) {
                app.set_warn("themes dir is not empty; git init blocked");
                return true;
            }
            let _ = std::fs::create_dir_all(&dir);
            match run_git(&dir, &["init"]) {
                Ok(out) => {
                    if ctx == GitContext::Templates {
                        app.refresh_templates();
                        app.refresh_net_templates();
                    }
                    show_git_output(app, "git init", &out);
                }
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        "clone" => {
            let Some(url) = rest.first().copied() else {
                app.set_warn("usage: :git <templates|themes> clone <url>");
                return true;
            };
            if url.starts_with('-') {
                app.set_warn("invalid clone url");
                return true;
            }
            if ctx == GitContext::Templates {
                if !templates_dir_is_empty(&dir) {
                    app.set_warn("templates dir is not empty; git clone blocked");
                    return true;
                }
                let _ = std::fs::create_dir_all(&dir);
                let _ = remove_empty_templates_scaffold(&dir);
            } else {
                if dir.exists() && !is_dir_empty(&dir) {
                    app.set_warn("themes dir is not empty; git clone blocked");
                    return true;
                }
                let _ = std::fs::create_dir_all(&dir);
            }
            match run_git(&dir, &["clone", url, "."]) {
                Ok(out) => {
                    if ctx == GitContext::Templates {
                        app.refresh_templates();
                        app.refresh_net_templates();
                    }
                    show_git_output(app, "git clone", &out);
                }
                Err(e) => app.set_error(format!("{e:#}")),
            }
        }
        _ => {
            app.set_warn("usage: :git <templates|themes> <status|diff|log|commit|pull|push|init|clone> ...");
        }
    }
    true
}
