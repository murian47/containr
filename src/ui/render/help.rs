use crate::ui::theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn shell_help_lines(theme: &theme::ThemeSpec) -> Vec<Line<'static>> {
    let h = |title: &str| -> Line<'static> {
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let item = |scope: &str, syntax: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("{scope:<10} "), theme.text_dim.to_style()),
            Span::styled(format!("{syntax:<22} "), Style::default().fg(Color::White)),
            Span::styled(desc.to_string(), theme.text.to_style()),
        ])
    };

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(h("General"));
    out.push(item("Always", "F1", "Open help"));
    out.push(item("Global", ":q", "Quit (prompts y/n)"));
    out.push(item("Global", ":q!", "Quit immediately (! auto-confirms)"));
    out.push(item("Global", ":! <cmd>", "Run command with auto-confirm (! modifier)"));
    out.push(item(
        "Note",
        "confirm",
        "Destructive commands prompt y/n; add ! to auto-confirm",
    ));
    out.push(item("Global", ":?", "Open help"));
    out.push(item("Global", ":help", "Open help"));
    out.push(item("Global", ":messages", "Toggle messages view (session log)"));
    out.push(item(
        "Global",
        ":messages save <file>",
        "Save session messages to a file",
    ));
    out.push(item(
        "Global",
        ":messages save! <file>",
        "Overwrite when the file exists",
    ));
    out.push(item("Global", ":ack [all]", "Clear per-item action error markers"));
    out.push(item("Global", ":refresh", "Trigger immediate refresh"));
    out.push(item(
        "Global",
        ":sidebar (toggle|compact)",
        "Show/hide sidebar or compact it",
    ));
    out.push(item(
        "Global",
        ":layout [horizontal|vertical|toggle]",
        "Set list/details split for current module",
    ));
    out.push(item(
        "Note",
        "aliases",
        ":ctr, :stk, :tpl, :img, :vol, :net (logs has no alias)",
    ));
    out.push(item(
        "Global",
        ":set refresh <sec>",
        "Set refresh interval (1..3600), saved to config",
    ));
    out.push(item(
        "Global",
        ":set logtail <n>",
        "Set docker logs --tail (1..200000), saved to config",
    ));
    out.push(item(
        "Global",
        ":set history <n>",
        "Set command history size (1..5000), saved to config",
    ));
    out.push(item(
        "Global",
        ":set editor <command>",
        "Set editor command (falls back to $EDITOR, then vi)",
    ));
    out.push(item(
        "Global",
        ":set git_autocommit <on|off>",
        "Auto-commit template changes when git integration is enabled",
    ));
    out.push(item(
        "Global",
        ":set git_autocommit_confirm <on|off>",
        "Ask before auto-committing (only used when git_autocommit=on)",
    ));
    out.push(item(
        "Global",
        ":set image_update_concurrency <n>",
        "Set concurrent image update checks (1..32), saved to config",
    ));
    out.push(item(
        "Global",
        ":set image_update_debug <on|off>",
        "Log extra image update debug details",
    ));
    out.push(item(
        "Global",
        ":set image_update_autocheck <on|off>",
        "Auto-check updates after template deploy (only with --pull)",
    ));
    out.push(Line::from(""));

    out.push(h("Keymap"));
    out.push(item("Note", "^x", "Means Ctrl-x (caret notation)"));
    out.push(item(
        "Keymap",
        "Scopes",
        "always, global, view:<name> (e.g. view:logs)",
    ));
    out.push(item(
        "Keymap",
        "Precedence",
        "always -> view:<current> -> global",
    ));
    out.push(item(
        "Keymap",
        "Disable",
        ":unmap inserts a disable marker that overrides defaults",
    ));
    out.push(item(
        "Global",
        ":map [scope] <KEY> <CMD...>",
        "Bind (e.g. :map always F1 :help, :map view:logs ^l :logs reload)",
    ));
    out.push(item(
        "Global",
        ":unmap [scope] <KEY>",
        "Disable binding or remove override (restore defaults)",
    ));
    out.push(item(
        "Global",
        ":map list",
        "List effective bindings (* = configured/overridden)",
    ));
    out.push(item(
        "Keymap",
        "Safety",
        "Destructive commands cannot be mapped to plain single letters",
    ));
    out.push(Line::from(""));

    out.push(h("Theme"));
    out.push(item(
        "Global",
        ":theme list",
        "Open theme selector (preview + apply)",
    ));
    out.push(item("Global", ":theme use <name>", "Switch active theme (persisted)"));
    out.push(item(
        "Global",
        ":theme new <name>",
        "Create a new theme from default and open configured editor/$EDITOR/vi",
    ));
    out.push(item(
        "Global",
        ":theme edit [name]",
        "Edit theme file via configured editor/$EDITOR/vi (creates if missing)",
    ));
    out.push(item("Global", ":theme rm[!] <name>", "Delete theme (! skips confirmation)"));
    out.push(Line::from(""));

    out.push(h("Messages"));
    out.push(item("Global", "^g", "Open full messages view"));
    out.push(item("Global", ":log dock", "Toggle docked messages panel"));
    out.push(item("Global", ":messages copy", "Copy selected message"));
    out.push(item("Global", ":messages save <file>", "Save messages to file"));
    out.push(Line::from(""));

    out.push(h("Git"));
    out.push(item("Git", ":git templates status", "Show repo status (short)"));
    out.push(item("Git", ":git templates diff", "Show repo diff"));
    out.push(item("Git", ":git templates log", "Show recent commits"));
    out.push(item("Git", ":git templates commit -m", "Commit with prompt for message"));
    out.push(item(
        "Git",
        ":git templates config user.name|user.email <value>",
        "Set local repo identity (used for autocommit)",
    ));
    out.push(item("Git", ":git templates pull", "git pull --rebase"));
    out.push(item("Git", ":git templates push", "git push"));
    out.push(item("Git", ":git templates init", "Initialize repo (only if empty)"));
    out.push(item("Git", ":git templates clone <url>", "Clone repo (only if empty)"));
    out.push(Line::from(""));

    out.push(h("Servers"));
    out.push(item("Global", ":server list", "List configured servers"));
    out.push(item("Global", ":server use <name>", "Switch active server"));
    out.push(item(
        "Global",
        ":server shell [name]",
        "Open SSH shell (local uses $SHELL)",
    ));
    out.push(item("Global", ":server rm <name>", "Remove server"));
    out.push(item(
        "Global",
        ":server add <name> ssh <target> [-p <port>] [-i <identity>] [--cmd <docker|podman>]",
        "Add SSH server entry (quote --cmd if it contains spaces, e.g. --cmd \"sudo docker\"; use \\' inside single quotes)",
    ));
    out.push(item(
        "Global",
        ":server add <name> local [--cmd <docker|podman>]",
        "Add local engine entry (quote --cmd if it contains spaces, e.g. --cmd \"sudo docker\"; use \\' inside single quotes)",
    ));
    out.push(Line::from(""));

    out.push(h("Templates"));
    out.push(item(
        "Templates",
        ":templates kind (stacks|networks|toggle)",
        "Switch between stack templates and network templates",
    ));
    out.push(item("Templates", "^t", "Toggle stacks/networks (default binding)"));
    out.push(item("Templates", "^a", "Run configured AI agent (default binding)"));
    out.push(item(
        "Templates",
        ":template/:tpl add <name>",
        "Create a new template",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl edit [name]",
        "Edit selected template (or by name)",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl from-stack <name>",
        "Generate compose.yaml from the selected stack",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl from-container <name>",
        "Generate compose.yaml from the selected container",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl rm [name]",
        "Delete selected template (or by name)",
    ));
    out.push(item(
        "Templates",
        ":template/:tpl deploy [--pull] [--recreate] [name]",
        "Deploy selected template (or by name) to active server",
    ));
    out.push(item(
        "Templates",
        ":ai",
        "Run the configured AI agent for the selected template",
    ));
    out.push(Line::from(""));
    out.push(item(
        "Templates",
        ":nettemplate/:nt deploy[!] [name]",
        "Create network on active server (! = recreate if already exists)",
    ));
    out.push(Line::from(""));

    out.push(h("Registries"));
    out.push(item("Registries", ":registries [view|list]", "Open view or list entries"));
    out.push(item(
        "Registries",
        ":registries identity <path>",
        "Set age identity path for encrypted secrets",
    ));
    out.push(item("Registries", ":registry add <host>", "Add registry entry"));
    out.push(item(
        "Registries",
        ":registry set <host> auth <anonymous|basic|bearer|github>",
        "Set auth mode",
    ));
    out.push(item(
        "Registries",
        ":registry set <host> username <value>",
        "Set username (use clear to remove)",
    ));
    out.push(item(
        "Registries",
        ":registry set <host> secret <value>",
        "Store secret (age encrypted)",
    ));
    out.push(item(
        "Registries",
        ":registry set <host> secret-file <path>",
        "Store secret from file (age encrypted)",
    ));
    out.push(item(
        "Registries",
        ":registry set <host> test-repo <owner/name>",
        "Set repository used for auth tests",
    ));
    out.push(item(
        "Registries",
        ":registry test [host]",
        "Test registry credentials (uses selected if omitted)",
    ));
    out.push(item("Registries", "^y", "Test selected registry (default binding)"));
    out.push(item("Registries", ":registry rm[!] <host>", "Remove registry entry"));
    out.push(Line::from(""));

    out.push(h("Stacks"));
    out.push(item(
        "Stacks",
        ":stack/:stk (start|stop|restart|rm|check) [name]",
        "Run action for selected stack (or by name)",
    ));
    out.push(item(
        "Stacks",
        ":stack/:stk check [name]",
        "Check image updates for stack containers (manual)",
    ));
    out.push(item(
        "Stacks",
        ":stacks running|all",
        "Filter stacks list (running only or show all)",
    ));
    out.push(Line::from(""));

    out.push(h("Containers"));
    out.push(item(
        "Containers",
        ":container/:ctr (start|stop|restart|rm|check)",
        "Run action for selection/marks/stack",
    ));
    out.push(item(
        "Containers",
        ":container/:ctr check",
        "Check image updates for selected containers (manual)",
    ));
    out.push(item(
        "Containers",
        ":container/:ctr console [-u USER] [bash|sh|SHELL]",
        "Open console for selected running container (default user: root)",
    ));
    out.push(item("Containers", ":container/:ctr tree", "Toggle stack (tree) view"));
    out.push(Line::from(""));

    out.push(h("Images"));
    out.push(item(
        "Images",
        ":image/:img untag",
        "Remove tag from selected/marked image",
    ));
    out.push(item("Images", ":image/:img rm", "Remove selected/marked image"));
    out.push(Line::from(""));

    out.push(h("Volumes"));
    out.push(item("Volumes", ":volume/:vol rm", "Remove selected/marked volume"));
    out.push(Line::from(""));

    out.push(h("Networks"));
    out.push(item(
        "Networks",
        ":network/:net rm",
        "Remove selected/marked network",
    ));
    out.push(item("Networks", "^d", "Remove (default binding)"));
    out.push(Line::from(""));

    out.push(h("Logs"));
    out.push(item("Logs", "^l", "Reload logs (default binding)"));
    out.push(item("Logs", "^c", "Copy selected lines to clipboard"));
    out.push(item("Logs", "m", "Toggle regex search"));
    out.push(item("Logs", "/", "Enter search mode"));
    out.push(item("Logs", ":", "Enter command mode"));
    out.push(item("Logs", "n/N", "Next/previous match"));
    out.push(item("Logs", "j/k", "Down/up"));
    out.push(item("Logs", "j <n>", "Jump to line n (1-based)"));
    out.push(item("Logs", "save <file>", "Save full logs to a file"));
    out.push(item("Logs", "save! <file>", "Overwrite when the file exists"));
    out.push(item("Logs", "set number", "Enable line numbers"));
    out.push(item("Logs", "set nonumber", "Disable line numbers"));
    out.push(item("Logs", "set regex", "Enable regex search"));
    out.push(item("Logs", "set noregex", "Disable regex search"));
    out.push(Line::from(""));

    out.push(h("Inspect"));
    out.push(item("Inspect", "/", "Enter search mode"));
    out.push(item("Inspect", ":", "Enter command mode"));
    out.push(item("Inspect", "Enter", "Expand/collapse selected node"));
    out.push(item("Inspect", "n/N", "Next/previous match"));
    out.push(item("Inspect", "expand", "Expand all"));
    out.push(item("Inspect", "collapse", "Collapse all"));
    out.push(item("Inspect", "save <file>", "Save full inspect JSON to a file"));
    out.push(item("Inspect", "save! <file>", "Overwrite when the file exists"));
    out.push(item("Inspect", "y", "Copy selected value (pretty)"));
    out.push(item("Inspect", "p", "Copy selected JSON pointer path"));
    out
}
