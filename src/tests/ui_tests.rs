use crate::config::{DockerCmd, RegistriesConfig, ServerEntry};
use crate::docker::NetworkRow;
use crate::ui::commands::cmdline_cmd::parse_cmdline_tokens;
use crate::ui::core::key_types::{
    KeyCodeNorm, KeyScope, KeySpec, build_default_keymap, parse_key_spec,
};
use crate::ui::core::requests::ActionRequest;
use crate::ui::core::types::{ImageUpdateEntry, ImageUpdateKind, LogsMode};
use crate::ui::state::app::App;
use crate::ui::state::shell_types::{ActiveView, ShellFocus, ShellView};
use crate::ui::theme;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn mk_temp_path(prefix: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos();
    dir.push(format!(
        "containr-tests-{prefix}-{now}-{}",
        std::process::id()
    ));
    dir
}

fn mk_test_app() -> App {
    let tmp = mk_temp_path("config");
    std::fs::create_dir_all(&tmp).unwrap();
    let config_path = tmp.join("config.json");
    App::new(
        vec![ServerEntry {
            name: "local".to_string(),
            target: "local".to_string(),
            port: None,
            identity: None,
            docker_cmd: DockerCmd::default(),
        }],
        Vec::new(),
        Some("local".to_string()),
        config_path,
        HashMap::new(),
        "default".to_string(),
        theme::default_theme_spec(),
        None,
        false,
        false,
        String::new(),
        4,
        false,
        false,
        true,
        false,
        5,
        RegistriesConfig::default(),
    )
}

fn render_screen(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            crate::ui::render::root::draw_shell(f, app, Duration::from_secs(5));
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = buf.area;
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn render_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            crate::ui::render::root::draw_shell(f, app, Duration::from_secs(5));
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn resolve_output_path_puts_bare_filename_into_home() {
    let home = std::env::var_os("HOME").expect("HOME");
    let resolved = crate::ui::render::utils::resolve_output_path("containr.log").expect("path");
    assert_eq!(resolved, PathBuf::from(home).join("containr.log"));
}

#[test]
fn resolve_output_path_keeps_explicit_subpath() {
    let resolved =
        crate::ui::render::utils::resolve_output_path("logs/containr.log").expect("path");
    assert_eq!(resolved, PathBuf::from("logs").join("containr.log"));
}

#[test]
fn parse_key_spec_allows_ctrl_shift_char_chord() {
    let ks = parse_key_spec("C-S-C").expect("parse C-S-C");
    assert_eq!(ks.mods, 1 | 2);
    assert_eq!(ks.code, KeyCodeNorm::Char('C'));
}

#[test]
fn default_keymap_contains_ctrl_shift_c_console_sh() {
    let km = build_default_keymap();
    let key = KeySpec {
        mods: 1 | 2,
        code: KeyCodeNorm::Char('C'),
    };
    let cmd = km
        .get(&(KeyScope::View(ShellView::Containers), key))
        .cloned();
    assert_eq!(cmd.as_deref(), Some("container console sh"));
}

#[test]
fn parse_cmdline_tokens_keeps_quoted_args() {
    let tokens = parse_cmdline_tokens("server add srv ssh host --cmd \"sudo docker\"")
        .expect("parse cmdline");
    assert_eq!(tokens.last().map(|s| s.as_str()), Some("sudo docker"));
}

#[test]
fn parse_cmdline_tokens_allows_escaped_space() {
    let tokens = parse_cmdline_tokens("set key foo\\ bar").expect("parse cmdline");
    assert_eq!(tokens, vec!["set", "key", "foo bar"]);
}

#[test]
fn docker_cmd_deserialize_from_string() {
    let v = json!({ "docker_cmd": "sudo docker" });
    let cmd: DockerCmd = serde_json::from_value(v["docker_cmd"].clone()).unwrap();
    assert_eq!(cmd.to_string(), "sudo docker");
    assert_eq!(cmd.to_shell(), "sudo docker");
}

#[test]
fn docker_cmd_deserialize_from_array() {
    let v = json!({ "docker_cmd": ["sudo", "docker"] });
    let cmd: DockerCmd = serde_json::from_value(v["docker_cmd"].clone()).unwrap();
    assert_eq!(cmd.to_string(), "sudo docker");
    assert_eq!(cmd.to_shell(), "sudo docker");
}

#[test]
fn docker_cmd_to_shell_escapes_tokens() {
    let cmd: DockerCmd = serde_json::from_value(json!("sudo docker --config 'a b'")).unwrap();
    assert_eq!(cmd.to_shell(), "sudo docker --config 'a b'");
}

#[test]
fn parse_cmdline_tokens_supports_mixed_quotes() {
    let tokens = parse_cmdline_tokens("cmd \"a b\" 'c d' \"e \\\"f\\\"\"").expect("parse cmdline");
    assert_eq!(tokens, vec!["cmd", "a b", "c d", "e \"f\""]);
}

#[test]
fn parse_cmdline_tokens_rejects_unterminated_quote() {
    let err = parse_cmdline_tokens("cmd \"unterminated").unwrap_err();
    assert!(err.contains("unterminated"));
}

#[test]
fn parse_cmdline_tokens_allows_single_quote_escapes() {
    let tokens = parse_cmdline_tokens("cmd 'a\\'b' 'c\\\\d'").expect("parse cmdline");
    assert_eq!(tokens, vec!["cmd", "a'b", "c\\d"]);
}

#[test]
fn dashboard_shows_no_server_message() {
    let tmp = mk_temp_path("config");
    std::fs::create_dir_all(&tmp).unwrap();
    let config_path = tmp.join("config.json");
    let mut app = App::new(
        Vec::new(),
        Vec::new(),
        None,
        config_path,
        HashMap::new(),
        "default".to_string(),
        theme::default_theme_spec(),
        None,
        false,
        false,
        String::new(),
        4,
        false,
        false,
        true,
        false,
        5,
        RegistriesConfig::default(),
    );
    app.loading = false;
    app.current_target.clear();
    app.shell_view = ShellView::Dashboard;
    let screen = render_screen(&mut app, 120, 30);
    assert!(screen.contains("No server configured"));
}

#[test]
fn sidebar_separator_uses_panel_background() {
    let mut app = mk_test_app();
    app.loading = false;
    app.shell_view = ShellView::Containers;
    app.shell_focus = ShellFocus::Sidebar;
    let buf = render_buffer(&mut app, 120, 40);
    let mut found = false;
    let expected_bg = theme::parse_color(&app.theme.panel.bg);
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if cell.symbol() == "─" {
                found = true;
                assert_eq!(cell.style().bg, Some(expected_bg));
                break;
            }
        }
        if found {
            break;
        }
    }
    assert!(found, "no sidebar separator glyph found");
}

#[test]
fn render_help_contains_core_sections() {
    let mut app = mk_test_app();
    app.loading = false;
    app.shell_view = ShellView::Help;
    let screen = render_screen(&mut app, 120, 60);
    assert!(screen.contains("General"));
    assert!(screen.contains(":map"));
    assert!(screen.contains(":messages"));
    assert!(screen.contains(":set refresh"));
}

#[test]
fn render_logs_shows_query_and_matches() {
    let mut app = mk_test_app();
    app.loading = false;
    app.shell_view = ShellView::Logs;
    app.logs.loading = false;
    app.logs.error = None;
    app.logs.text =
        Some("first line\nerror: something failed\nsecond line\nERROR: another one\n".to_string());
    app.logs.show_line_numbers = true;
    app.logs.query = "error".to_string();
    app.logs.mode = LogsMode::Normal;
    app.logs_rebuild_matches();

    let screen = render_screen(&mut app, 120, 30);
    assert!(screen.contains("error: something failed"));
    assert!(screen.contains("Matches:"));
    assert!(screen.contains("Query:"));
    assert!(screen.contains("error"));
}

#[test]
fn render_logs_marks_selected_range() {
    let mut app = mk_test_app();
    app.loading = false;
    app.shell_view = ShellView::Logs;
    app.logs.loading = false;
    app.logs.error = None;
    app.logs.text = Some("first line\nsecond line\nthird line\n".to_string());
    app.logs.cursor = 1;
    app.logs.select_anchor = Some(0);

    let buf = render_buffer(&mut app, 120, 20);
    let marked_fg = theme::parse_color(&app.theme.marked.fg);
    let mut found_marked = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if cell.symbol().trim().is_empty() {
                continue;
            }
            if cell.style().fg == Some(marked_fg) {
                found_marked = true;
                break;
            }
        }
        if found_marked {
            break;
        }
    }
    assert!(found_marked, "no marked log selection glyph found");
}

#[test]
fn render_messages_marks_selected_range() {
    let mut app = mk_test_app();
    app.loading = false;
    app.shell_view = ShellView::Messages;
    app.session_msgs
        .push(crate::ui::state::shell_types::SessionMsg {
            at: crate::ui::core::clock::now_local(),
            level: crate::ui::state::shell_types::MsgLevel::Info,
            text: "first message".to_string(),
        });
    app.session_msgs
        .push(crate::ui::state::shell_types::SessionMsg {
            at: crate::ui::core::clock::now_local(),
            level: crate::ui::state::shell_types::MsgLevel::Warn,
            text: "second message".to_string(),
        });
    app.shell_msgs.scroll = 1;
    app.shell_msgs.select_anchor = Some(0);

    let buf = render_buffer(&mut app, 120, 20);
    let marked_fg = theme::parse_color(&app.theme.marked.fg);
    let mut found_marked = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if cell.symbol() == "f" && cell.style().fg == Some(marked_fg) {
                found_marked = true;
                break;
            }
        }
        if found_marked {
            break;
        }
    }
    assert!(found_marked, "no marked message selection glyph found");
}

#[test]
fn check_image_updates_skips_fresh_cached_entries() {
    let mut app = mk_test_app();
    app.loading = false;
    let image = "docker.io/library/redis:7-alpine".to_string();
    app.image_updates.insert(
        image.clone(),
        ImageUpdateEntry {
            checked_at: crate::ui::core::clock::now_unix(),
            status: ImageUpdateKind::UpToDate,
            local_digest: Some("sha256:local".to_string()),
            remote_digest: Some("sha256:remote".to_string()),
            note: None,
            error: None,
        },
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ActionRequest>();
    crate::ui::ui_actions::check_image_updates(&mut app, vec![image], &tx);

    assert!(
        rx.try_recv().is_err(),
        "cached image check should not queue request"
    );
    assert!(app.image_updates_inflight.is_empty());
}

#[test]
fn network_remove_uses_marked_ids() {
    let mut app = mk_test_app();
    app.shell_view = ShellView::Networks;
    app.active_view = ActiveView::Networks;
    app.networks = vec![
        NetworkRow {
            id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            name: "net_a".to_string(),
            driver: "bridge".to_string(),
            scope: "local".to_string(),
            labels: String::new(),
        },
        NetworkRow {
            id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            name: "net_b".to_string(),
            driver: "bridge".to_string(),
            scope: "local".to_string(),
            labels: String::new(),
        },
    ];
    app.marked_networks.insert(app.networks[0].id.clone());
    app.marked_networks.insert(app.networks[1].id.clone());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ActionRequest>();
    crate::ui::state::actions::exec_network_remove(&mut app, &tx);

    let mut ids = Vec::new();
    while let Ok(req) = rx.try_recv() {
        if let ActionRequest::NetworkRemove { id } = req {
            ids.push(id);
        }
    }
    ids.sort();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().any(|i: &String| i.starts_with('a')));
    assert!(ids.iter().any(|i: &String| i.starts_with('b')));
}
