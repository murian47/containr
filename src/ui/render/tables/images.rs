use crate::ui::core::types::ActionErrorKind;
use crate::ui::render::tables::common::shell_header_style;
use crate::ui::render::utils::{shell_row_highlight, truncate_end};
use crate::ui::state::app::App;
use ratatui::layout::{Constraint, Margin, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Cell, Row, Table, TableState};

pub(in crate::ui) fn draw_shell_images_table(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });

    const REF_TEXT_MAX: usize = 62;
    const ID_TEXT_MAX: usize = 50;
    const USED_W: usize = 3;
    const SIZE_W: usize = 10;
    const REF_MIN_W: usize = 24;
    const ID_MIN_W: usize = 10;

    let size_cell = |s: &str| -> String {
        if s.chars().count() >= SIZE_W {
            truncate_end(s, SIZE_W)
        } else {
            format!("{:>width$}", s, width = SIZE_W)
        }
    };

    let mut max_ref = 0usize;
    let mut max_id = 0usize;
    let mut rows: Vec<Row> = Vec::new();
    for img in app
        .images
        .iter()
        .filter(|img| !app.images_unused_only || !app.image_referenced(img))
    {
        let reference_full = img.name();
        let reference = truncate_end(&reference_full, REF_TEXT_MAX);
        let id = truncate_end(&img.id, ID_TEXT_MAX);
        let key = App::image_row_key(img);
        let marked = app.is_image_marked(&key);
        let row_style = if marked {
            app.theme.marked.to_style()
        } else {
            Style::default()
        };
        let is_removing = app.image_action_inflight.contains_key(&key);
        let err = app.image_action_error.get(&key);
        let used = app
            .image_referenced_count_by_id
            .get(&img.id)
            .copied()
            .unwrap_or(0)
            > 0;
        let used_cell = if used {
            if app.ascii_only { "Y" } else { "✓" }
        } else {
            ""
        };
        let size = if is_removing {
            Cell::from(size_cell("removing")).style(bg.patch(app.theme.text_warn.to_style()))
        } else if let Some(err) = err {
            let style = match err.kind {
                ActionErrorKind::InUse => bg.patch(app.theme.text_warn.to_style()),
                ActionErrorKind::Other => bg.patch(app.theme.text_error.to_style()),
            };
            Cell::from(size_cell(crate::ui::render::status::action_error_label(
                err,
            )))
            .style(style)
        } else {
            Cell::from(size_cell(&img.size))
        };
        max_ref = max_ref.max(reference.chars().count());
        max_id = max_id.max(id.chars().count());
        rows.push(
            Row::new(vec![
                Cell::from(reference),
                Cell::from(id),
                Cell::from(used_cell).style(bg.patch(app.theme.text_ok.to_style())),
                size,
            ])
            .style(row_style),
        );
    }
    let inner_w = inner.width.max(1) as usize;
    let spacing = 3;
    let fixed = USED_W + SIZE_W + spacing;
    let avail = inner_w.saturating_sub(fixed);

    let mut ref_w = max_ref.clamp(REF_MIN_W, REF_TEXT_MAX).min(avail);
    let mut id_w = max_id
        .clamp(ID_MIN_W, ID_TEXT_MAX)
        .min(avail.saturating_sub(ref_w));
    if ref_w + id_w < avail {
        let extra = avail - (ref_w + id_w);
        let add_ref = extra.min(REF_TEXT_MAX.saturating_sub(ref_w));
        ref_w += add_ref;
        let extra = extra - add_ref;
        id_w = (id_w + extra).min(ID_TEXT_MAX);
    }
    if avail > 0 {
        if ref_w == 0 {
            ref_w = 1.min(avail);
        }
        if id_w == 0 && avail > ref_w {
            id_w = 1;
        }
    }

    let mut state = TableState::default();
    state.select(Some(app.images_selected.min(rows.len().saturating_sub(1))));
    let table = Table::new(
        rows,
        [
            Constraint::Length(ref_w as u16),
            Constraint::Length(id_w as u16),
            Constraint::Length(USED_W as u16),
            Constraint::Length(SIZE_W as u16),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("REF"),
            Cell::from("ID"),
            Cell::from("USED"),
            Cell::from(size_cell("SIZE")),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}
