use crate::ui::state::app::App;
use crate::ui::state::shell_types::TemplatesKind;
use crate::ui::theme;

pub(in crate::ui) struct CmdlineCompletionContext {
    pub(in crate::ui) tokens_before: Vec<String>,
    pub(in crate::ui) token_prefix: String,
    pub(in crate::ui) token_start: usize,
    pub(in crate::ui) cursor_byte: usize,
    pub(in crate::ui) quote_prefix: bool,
}

fn cmdline_char_to_byte_index(input: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    match input.char_indices().nth(char_idx) {
        Some((idx, _)) => idx,
        None => input.len(),
    }
}

pub(in crate::ui) fn cmdline_completion_context(
    input: &str,
    cursor: usize,
) -> CmdlineCompletionContext {
    let cursor_byte = cmdline_char_to_byte_index(input, cursor);
    let mut tokens_before: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut token_start: Option<usize> = None;
    let mut in_quotes = false;
    let mut escaped = false;

    for (idx, ch) in input[..cursor_byte].char_indices() {
        if escaped {
            token.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quotes = !in_quotes;
            if token_start.is_none() {
                token_start = Some(idx);
            }
            continue;
        }
        if !in_quotes && ch.is_whitespace() {
            if token_start.is_some() {
                tokens_before.push(std::mem::take(&mut token));
                token_start = None;
            }
            continue;
        }
        if token_start.is_none() {
            token_start = Some(idx);
        }
        token.push(ch);
    }

    let (token_prefix, token_start) = if let Some(start) = token_start {
        (token, start)
    } else {
        (String::new(), cursor_byte)
    };
    let quote_prefix =
        token_start < cursor_byte && input[token_start..cursor_byte].starts_with('"');

    CmdlineCompletionContext {
        tokens_before,
        token_prefix,
        token_start,
        cursor_byte,
        quote_prefix,
    }
}

fn cmdline_common_prefix_len_ci(a: &str, b: &str) -> usize {
    let mut len = 0usize;
    let mut it_a = a.chars();
    let mut it_b = b.chars();
    while let (Some(ca), Some(cb)) = (it_a.next(), it_b.next()) {
        if !ca.eq_ignore_ascii_case(&cb) {
            break;
        }
        len += 1;
    }
    len
}

pub(in crate::ui) fn cmdline_common_prefix_ci(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let mut len = strings[0].chars().count();
    for s in strings.iter().skip(1) {
        len = len.min(cmdline_common_prefix_len_ci(&strings[0], s));
    }
    strings[0].chars().take(len).collect()
}

fn cmdline_filter_candidates(prefix: &str, candidates: Vec<String>) -> Vec<String> {
    let prefix_lc = prefix.to_ascii_lowercase();
    let mut out: Vec<String> = candidates
        .into_iter()
        .filter(|c| c.to_ascii_lowercase().starts_with(&prefix_lc))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn cmdline_command_candidates() -> Vec<&'static str> {
    vec![
        "q",
        "help",
        "?",
        "messages",
        "msgs",
        "ack",
        "refresh",
        "theme",
        "git",
        "map",
        "unmap",
        "container",
        "ctr",
        "stack",
        "stacks",
        "stk",
        "image",
        "img",
        "volume",
        "vol",
        "network",
        "net",
        "sidebar",
        "ai",
        "inspect",
        "logs",
        "set",
        "layout",
        "templates",
        "template",
        "tpl",
        "registries",
        "registry",
        "reg",
        "nettemplate",
        "nettpl",
        "ntpl",
        "nt",
        "server",
    ]
}

fn cmdline_scope_candidates() -> Vec<String> {
    vec![
        "always",
        "global",
        "view:dashboard",
        "view:stacks",
        "view:containers",
        "view:images",
        "view:volumes",
        "view:networks",
        "view:templates",
        "view:registries",
        "view:logs",
        "view:inspect",
        "view:messages",
        "view:help",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}

fn cmdline_key_candidates() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in [
        "Enter",
        "Esc",
        "Tab",
        "Backspace",
        "Delete",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Up",
        "Down",
        "Left",
        "Right",
        "Space",
    ] {
        out.push(key.to_string());
    }
    for n in 1..=12 {
        out.push(format!("F{n}"));
        out.push(format!("C-F{n}"));
    }
    for ch in [
        'a', 'b', 'c', 'd', 'e', 'g', 'k', 'n', 'o', 'p', 'r', 's', 't', 'u', 'y',
    ] {
        out.push(format!("C-{ch}"));
    }
    out
}

fn cmdline_theme_names(app: &App) -> Vec<String> {
    match theme::list_theme_names(&app.config_path) {
        Ok(mut names) => {
            if !names.iter().any(|n| n == "default") {
                names.insert(0, "default".to_string());
            }
            names
        }
        Err(_) => vec![],
    }
}

fn cmdline_server_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = app.servers.iter().map(|s| s.name.clone()).collect();
    names.sort();
    names
}

fn cmdline_template_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = match app.templates_state.kind {
        TemplatesKind::Stacks => app
            .templates_state
            .templates
            .iter()
            .map(|t| t.name.clone())
            .collect(),
        TemplatesKind::Networks => app
            .templates_state
            .net_templates
            .iter()
            .map(|t| t.name.clone())
            .collect(),
    };
    names.sort();
    names
}

fn cmdline_net_template_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = app
        .templates_state
        .net_templates
        .iter()
        .map(|t| t.name.clone())
        .collect();
    names.sort();
    names
}

fn cmdline_registry_hosts(app: &App) -> Vec<String> {
    let mut hosts: Vec<String> = app
        .registries_cfg
        .registries
        .iter()
        .map(|r| r.host.clone())
        .collect();
    hosts.sort();
    hosts
}

fn cmdline_stack_names(app: &App) -> Vec<String> {
    let mut names: Vec<String> = app.stacks.iter().map(|s| s.name.clone()).collect();
    names.sort();
    names
}

fn cmdline_normalize_cmd(tokens_before: &[String]) -> (Option<String>, usize) {
    if tokens_before.is_empty() {
        return (None, 0);
    }
    let mut first = tokens_before[0].as_str();
    if first == "!" {
        if let Some(cmd) = tokens_before.get(1) {
            return (Some(cmd.clone()), 1);
        }
        return (None, 1);
    }
    if let Some(rest) = first.strip_prefix(':') {
        first = rest;
    }
    if let Some(rest) = first.strip_prefix('!')
        && !rest.is_empty()
    {
        return (Some(rest.to_string()), 0);
    }
    if let Some(rest) = first.strip_suffix('!')
        && !rest.is_empty()
    {
        return (Some(rest.to_string()), 0);
    }
    (Some(first.to_string()), 0)
}

pub(in crate::ui) fn cmdline_completion_candidates(
    app: &App,
    ctx: &CmdlineCompletionContext,
) -> (String, Vec<String>) {
    let mut leading = String::new();
    let token_index = ctx.tokens_before.len();
    let mut token_prefix = ctx.token_prefix.clone();

    let command_position = token_index == 0
        || (token_index == 1 && ctx.tokens_before.first().is_some_and(|t| t == "!"));

    if command_position {
        if token_prefix.starts_with(':') {
            leading.push(':');
            token_prefix = token_prefix[1..].to_string();
        }
        if token_prefix.starts_with('!') {
            leading.push('!');
            token_prefix = token_prefix[1..].to_string();
        }
        if token_index == 1 {
            leading.push('!');
        }
        let candidates: Vec<String> = cmdline_command_candidates()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        return (
            leading,
            cmdline_filter_candidates(&token_prefix, candidates),
        );
    }

    let (cmd_opt, cmd_idx) = cmdline_normalize_cmd(&ctx.tokens_before);
    let Some(cmd_raw) = cmd_opt else {
        return (String::new(), Vec::new());
    };
    let cmd = cmd_raw.to_ascii_lowercase();
    let arg_index = ctx
        .tokens_before
        .len()
        .saturating_sub(cmd_idx.saturating_add(1));
    let sub = ctx
        .tokens_before
        .get(cmd_idx + 1)
        .map(|s| s.as_str())
        .unwrap_or("");

    let candidates: Vec<String> = match cmd.as_str() {
        "theme" => {
            if arg_index == 0 {
                vec!["list", "use", "new", "edit", "rm"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "use" | "edit" | "rm") && arg_index == 1 {
                cmdline_theme_names(app)
            } else {
                Vec::new()
            }
        }
        "server" => {
            if arg_index == 0 {
                vec!["list", "use", "add", "rm", "shell"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "use" | "rm" | "shell") && arg_index == 1 {
                cmdline_server_names(app)
            } else if sub == "add" {
                if arg_index == 2 {
                    vec!["ssh", "local"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        "template" | "tpl" | "templates" => {
            if arg_index == 0 {
                vec!["add", "edit", "rm", "deploy", "from", "from-network"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "add" | "edit" | "rm" | "deploy") && arg_index == 1 {
                cmdline_template_names(app)
            } else if sub == "from" {
                if arg_index == 1 {
                    vec!["stack", "container", "network"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                } else if arg_index == 2 {
                    cmdline_stack_names(app)
                } else {
                    Vec::new()
                }
            } else if sub == "from-network" {
                if arg_index == 1 {
                    cmdline_net_template_names(app)
                } else if arg_index == 2 {
                    cmdline_stack_names(app)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        "git" => {
            if arg_index == 0 {
                vec![
                    "status",
                    "commit",
                    "push",
                    "pull",
                    "init",
                    "clone",
                    "autocommit",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if arg_index == 1 {
                vec!["templates", "config", "messages"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if sub == "templates" && arg_index == 2 {
                vec![
                    "status",
                    "commit",
                    "push",
                    "pull",
                    "init",
                    "clone",
                    "autocommit",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if sub == "config" && arg_index == 2 {
                vec!["user.name", "user.email"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "commit" | "autocommit") && arg_index == 2 {
                vec!["-m".to_string()]
            } else {
                Vec::new()
            }
        }
        "map" => {
            if arg_index == 0 {
                let mut out = vec!["list".to_string()];
                out.extend(cmdline_scope_candidates());
                out
            } else if arg_index == 1 {
                cmdline_key_candidates()
            } else {
                Vec::new()
            }
        }
        "unmap" => {
            if arg_index == 0 {
                cmdline_scope_candidates()
            } else if arg_index == 1 {
                cmdline_key_candidates()
            } else {
                Vec::new()
            }
        }
        "registry" | "registries" | "reg" => {
            if arg_index == 0 {
                vec!["add", "rm", "set", "list", "test", "default"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if matches!(sub, "rm" | "set" | "test" | "default") && arg_index == 1 {
                cmdline_registry_hosts(app)
            } else if sub == "set" && arg_index == 2 {
                vec!["auth", "username", "secret", "repo"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "container" | "ctr" => {
            if arg_index == 0 {
                vec!["start", "stop", "restart", "rm", "console", "tree", "check"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else if sub == "console" && arg_index == 1 {
                vec!["bash", "sh", "-u"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "stack" | "stacks" | "stk" => {
            if arg_index == 0 {
                vec![
                    "start", "stop", "restart", "rm", "check", "update", "running", "all",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else if matches!(
                sub,
                "start" | "stop" | "restart" | "rm" | "check" | "update"
            ) && arg_index == 1
            {
                cmdline_stack_names(app)
            } else {
                Vec::new()
            }
        }
        "image" | "img" => {
            if arg_index == 0 {
                vec!["untag", "rm", "push"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "volume" | "vol" => {
            if arg_index == 0 {
                vec!["rm"].into_iter().map(|s| s.to_string()).collect()
            } else {
                Vec::new()
            }
        }
        "network" | "net" => {
            if arg_index == 0 {
                vec!["rm"].into_iter().map(|s| s.to_string()).collect()
            } else {
                Vec::new()
            }
        }
        "logs" | "log" => {
            if arg_index == 0 {
                vec!["reload", "save", "save!"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        "inspect" => Vec::new(),
        "set" => {
            if arg_index == 0 {
                vec![
                    "refresh",
                    "logtail",
                    "autocommit",
                    "git_autocommit",
                    "git_autocommit_confirm",
                    "kitty_graphics",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect()
            } else {
                Vec::new()
            }
        }
        "layout" => {
            if arg_index == 0 {
                vec!["toggle", "h", "hor", "horizontal", "v", "ver", "vertical"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    (
        String::new(),
        cmdline_filter_candidates(&ctx.token_prefix, candidates),
    )
}

pub(in crate::ui) fn cmdline_apply_completion(app: &mut App) {
    let input = app.shell_cmdline.input.clone();
    let cursor = app.shell_cmdline.cursor;
    let ctx = cmdline_completion_context(&input, cursor);
    let (leading, mut matches) = cmdline_completion_candidates(app, &ctx);
    if matches.is_empty() {
        return;
    }

    let mut prefix = ctx.token_prefix.clone();
    if !leading.is_empty() && prefix.starts_with(':') {
        prefix = prefix.trim_start_matches(':').to_string();
    }
    if leading.contains('!') && prefix.starts_with('!') {
        prefix = prefix.trim_start_matches('!').to_string();
    }

    let single_match = matches.len() == 1;
    let replacement = if single_match {
        matches[0].clone()
    } else {
        let common = cmdline_common_prefix_ci(&matches);
        if common.len() > prefix.len() {
            common
        } else {
            String::new()
        }
    };

    if replacement.is_empty() {
        let max = 12usize;
        if matches.len() > max {
            let rest = matches.len() - max;
            matches.truncate(max);
            app.set_info(format!("matches: {} ... +{rest} more", matches.join(" ")));
        } else {
            app.set_info(format!("matches: {}", matches.join(" ")));
        }
        return;
    }

    let mut replace_text = format!("{leading}{replacement}");
    if ctx.quote_prefix {
        replace_text = format!("\"{}", replace_text);
    }

    let mut new_input = String::new();
    new_input.push_str(&input[..ctx.token_start]);
    new_input.push_str(&replace_text);
    new_input.push_str(&input[ctx.cursor_byte..]);
    app.shell_cmdline.input = new_input;
    app.shell_cmdline.cursor = app.shell_cmdline.input[..ctx.token_start + replace_text.len()]
        .chars()
        .count();

    if single_match {
        let after = &app.shell_cmdline.input[ctx.token_start + replace_text.len()..];
        if after.is_empty() {
            app.shell_cmdline.input.push(' ');
            app.shell_cmdline.cursor += 1;
        }
    } else {
        let max = 12usize;
        if matches.len() > max {
            let rest = matches.len() - max;
            matches.truncate(max);
            app.set_info(format!("matches: {} ... +{rest} more", matches.join(" ")));
        } else {
            app.set_info(format!("matches: {}", matches.join(" ")));
        }
    }
}
