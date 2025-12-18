use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
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
            docker_cmd: "docker".to_string(),
        }],
        Vec::new(),
        Some("local".to_string()),
        config_path,
        HashMap::new(),
        "default".to_string(),
        theme::default_theme_spec(),
    )
}

fn render_screen(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            draw_shell(f, app, Duration::from_secs(5));
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
fn render_help_contains_core_sections() {
    let mut app = mk_test_app();
    app.loading = false;
    app.shell_view = ShellView::Help;
    let screen = render_screen(&mut app, 120, 40);
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
