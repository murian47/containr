use super::panel_bg;
use crate::ui::core::types::ActionErrorKind;
use crate::ui::render::status::image_update_indicator;
use crate::ui::render::status::{action_error_details, action_error_label, action_status_prefix};
use crate::ui::render::table::{DetailRow, render_detail_table};
use crate::ui::state::app::App;
use crate::ui::state::image_updates::resolve_image_update_state;
use ratatui::layout::Margin;
use ratatui::widgets::{Block, Paragraph, Wrap};

pub(super) fn draw_shell_container_details(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = panel_bg(app);
    f.render_widget(Block::default().style(bg), area);
    let Some(c) = app.selected_container().cloned() else {
        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("Select a container to see details.")
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let mut scroll = app.container_details_scroll;
    if app.container_details_id.as_deref() != Some(&c.id) {
        app.container_details_id = Some(c.id.clone());
        scroll = 0;
    }
    let val = bg;
    let cpu = c.cpu_perc.clone().unwrap_or_else(|| "-".to_string());
    let mem = c.mem_perc.clone().unwrap_or_else(|| "-".to_string());
    let ip = app
        .ip_cache
        .get(&c.id)
        .map(|(ip, _)| ip.clone())
        .unwrap_or_else(|| "-".to_string());
    let (status_value, status_style) = if let Some(marker) = app.action_inflight.get(&c.id) {
        (
            action_status_prefix(marker.action).to_string(),
            bg.patch(app.theme.text_warn.to_style()),
        )
    } else if let Some(err) = app.container_action_error.get(&c.id) {
        let style = match err.kind {
            ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
            ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
        };
        (action_error_label(err).to_string(), style)
    } else {
        (c.status.clone(), val)
    };
    let (update_text, update_style) =
        image_update_indicator(app, resolve_image_update_state(app, &c.image).1, bg);
    let mut rows = vec![
        DetailRow {
            key: "Name",
            value: c.name.clone(),
            style: val,
        },
        DetailRow {
            key: "ID",
            value: c.id.clone(),
            style: val,
        },
        DetailRow {
            key: "Image",
            value: c.image.clone(),
            style: val,
        },
        DetailRow {
            key: "Update",
            value: update_text,
            style: update_style,
        },
        DetailRow {
            key: "Status",
            value: status_value,
            style: status_style,
        },
        DetailRow {
            key: "CPU / MEM",
            value: format!("{cpu} / {mem}"),
            style: val,
        },
        DetailRow {
            key: "IP",
            value: ip,
            style: val,
        },
        DetailRow {
            key: "Ports",
            value: c.ports.clone(),
            style: val,
        },
    ];
    if let Some(err) = app.container_action_error.get(&c.id) {
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
    app.container_details_scroll = scroll;
}
