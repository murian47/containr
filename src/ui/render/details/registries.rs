use super::panel_bg;
use crate::ui::render::format::format_action_ts;
use crate::ui::render::registries::registry_auth_label;
use crate::ui::render::table::{render_detail_table, DetailRow};
use crate::ui::render::text::truncate_end;
use crate::ui::App;
use ratatui::widgets::Block;
use time::OffsetDateTime;

pub(super) fn draw_shell_registry_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let Some(r) = app
        .registries_cfg
        .registries
        .get(app.registries_selected)
        .cloned()
    else {
        return;
    };
    let mut scroll = app.registries_details_scroll;
    let host = r.host.trim().to_ascii_lowercase();
    let resolved = app.registry_auths.get(&host);
    let secret_status = if r.secret.as_ref().map(|s| s.trim()).unwrap_or("").is_empty() {
        "missing"
    } else if resolved.and_then(|a| a.secret.as_ref()).is_some() {
        "loaded"
    } else {
        "unavailable"
    };
    let username = r.username.clone().unwrap_or_else(|| "-".to_string());
    let test_repo = r.test_repo.clone().unwrap_or_else(|| "-".to_string());
    let (test_time, test_result) = if let Some(entry) = app.registry_tests.get(&host) {
        let ts = OffsetDateTime::from_unix_timestamp(entry.checked_at)
            .map(format_action_ts)
            .unwrap_or_else(|_| entry.checked_at.to_string());
        let status = if entry.ok { "ok" } else { "error" };
        let result = if entry.message.trim().is_empty() {
            status.to_string()
        } else {
            format!("{status}: {}", entry.message)
        };
        (ts, truncate_end(&result, 120))
    } else {
        ("-".to_string(), "-".to_string())
    };
    let val = bg;
    let rows = vec![
        DetailRow {
            key: "Host",
            value: r.host,
            style: val,
        },
        DetailRow {
            key: "Auth",
            value: registry_auth_label(&r.auth).to_string(),
            style: val,
        },
        DetailRow {
            key: "Username",
            value: username,
            style: val,
        },
        DetailRow {
            key: "Secret",
            value: secret_status.to_string(),
            style: val,
        },
        DetailRow {
            key: "Test repo",
            value: test_repo,
            style: val,
        },
        DetailRow {
            key: "Last test",
            value: test_time,
            style: val,
        },
        DetailRow {
            key: "Test result",
            value: test_result,
            style: val,
        },
        DetailRow {
            key: "Identity",
            value: app.registries_cfg.age_identity.clone(),
            style: val,
        },
    ];
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.registries_details_scroll = scroll;
}
