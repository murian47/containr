use super::panel_bg;
use crate::ui::core::types::ActionErrorKind;
use crate::ui::render::status::{action_error_details, action_error_label};
use crate::ui::render::table::{DetailRow, render_detail_table};
use crate::ui::state::app::App;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Block;

pub(super) fn draw_shell_image_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let Some(img) = app.selected_image().cloned() else {
        return;
    };
    let mut scroll = app.image_details_scroll;
    if app.image_details_id.as_deref() != Some(&img.id) {
        app.image_details_id = Some(img.id.clone());
        scroll = 0;
    }
    let used_by = app
        .image_containers_by_id
        .get(&img.id)
        .cloned()
        .unwrap_or_default();
    let used_by = if used_by.is_empty() {
        "-".to_string()
    } else {
        used_by.join(", ")
    };
    let val = bg;
    let key = App::image_row_key(&img);
    let mut rows = vec![
        DetailRow {
            key: "Ref",
            value: img.name(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.image_action_inflight.contains_key(&key) {
                "removing".to_string()
            } else if let Some(err) = app.image_action_error.get(&key) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.image_action_inflight.contains_key(&key) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.image_action_error.get(&key) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "ID",
            value: img.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Size",
            value: img.size.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.image_action_error.get(&key) {
        let v = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.image_details_scroll = scroll;
}

pub(super) fn draw_shell_volume_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let Some(v) = app.selected_volume().cloned() else {
        return;
    };
    let mut scroll = app.volume_details_scroll;
    if app.volume_details_id.as_deref() != Some(&v.name) {
        app.volume_details_id = Some(v.name.clone());
        scroll = 0;
    }
    let used_by = app
        .volume_containers_by_name
        .get(&v.name)
        .map(|xs| xs.join(", "))
        .unwrap_or_else(|| "-".to_string());
    let val = bg;
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: v.name.clone(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.volume_action_inflight.contains_key(&v.name) {
                "removing".to_string()
            } else if let Some(err) = app.volume_action_error.get(&v.name) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.volume_action_inflight.contains_key(&v.name) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.volume_action_error.get(&v.name) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "Driver",
            value: v.driver.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.volume_action_error.get(&v.name) {
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v_style,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.volume_details_scroll = scroll;
}

pub(super) fn draw_shell_network_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let Some(n) = app.selected_network().cloned() else {
        return;
    };
    let mut scroll = app.network_details_scroll;
    if app.network_details_id.as_deref() != Some(&n.id) {
        app.network_details_id = Some(n.id.clone());
        scroll = 0;
    }
    let is_system = App::is_system_network(&n);
    let used_by = app
        .network_containers_by_id
        .get(&n.id)
        .cloned()
        .unwrap_or_default();
    let used_by = if used_by.is_empty() {
        "-".to_string()
    } else {
        used_by.join(", ")
    };
    let val = bg;
    let type_style = if is_system {
        bg.patch(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        bg.patch(Style::default().fg(Color::White))
    };
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: n.name.clone(),
            style: val,
        },
        DetailRow {
            key: "Status",
            value: if app.network_action_inflight.contains_key(&n.id) {
                "removing".to_string()
            } else if let Some(err) = app.network_action_error.get(&n.id) {
                action_error_label(err).to_string()
            } else {
                "-".to_string()
            },
            style: if app.network_action_inflight.contains_key(&n.id) {
                bg.patch(app.theme.text_warn.to_style())
            } else if let Some(err) = app.network_action_error.get(&n.id) {
                match err.kind {
                    ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                    ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
                }
            } else {
                val
            },
        },
        DetailRow {
            key: "Type",
            value: if is_system { "System" } else { "User" }.to_string(),
            style: type_style,
        },
        DetailRow {
            key: "ID",
            value: n.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Driver",
            value: n.driver.clone(),
            style: val,
        },
        DetailRow {
            key: "Scope",
            value: n.scope.clone(),
            style: val,
        },
        DetailRow {
            key: "Used by",
            value: used_by,
            style: val,
        },
    ];
    if let Some(err) = app.network_action_error.get(&n.id) {
        let v_style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        rows.push(DetailRow {
            key: "Last error",
            value: format!("[{}] {}", action_error_details(err), err.message),
            style: v_style,
        });
    }
    scroll = render_detail_table(f, app, area, rows, scroll);
    app.network_details_scroll = scroll;
}
