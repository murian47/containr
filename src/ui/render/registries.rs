use crate::config;
use crate::ui::App;
use ratatui::layout::Constraint;
use ratatui::text::Span;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap};

use super::tables::shell_header_style;
use super::utils::shell_row_highlight;

pub(in crate::ui) fn registry_auth_label(auth: &config::RegistryAuth) -> &'static str {
    match auth {
        config::RegistryAuth::Anonymous => "anonymous",
        config::RegistryAuth::Basic => "basic",
        config::RegistryAuth::BearerToken => "bearer",
        config::RegistryAuth::GithubPat => "github",
    }
}

pub(in crate::ui) fn draw_shell_registries_table(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 0,
        horizontal: 1,
    });

    if app.registries_cfg.registries.is_empty() {
        let msg = "No registries configured (edit via :registry add).".to_string();
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows: Vec<Row> = app
        .registries_cfg
        .registries
        .iter()
        .map(|r| {
            let host = r.host.clone();
            let is_default = app
                .registries_cfg
                .default_registry
                .as_ref()
                .map(|h| h.eq_ignore_ascii_case(&host))
                .unwrap_or(false);
            let def = if is_default {
                Cell::from(Span::styled("✓", bg.patch(app.theme.text_ok.to_style())))
            } else {
                Cell::from("")
            };
            let auth = registry_auth_label(&r.auth).to_string();
            let user = r.username.clone().unwrap_or_else(|| "-".to_string());
            let secret = if r.secret.as_ref().map(|s| s.trim()).unwrap_or("").is_empty() {
                "-"
            } else {
                "yes"
            };
            Row::new(vec![
                Cell::from(host),
                Cell::from(auth),
                Cell::from(user),
                Cell::from(secret),
                def,
            ])
        })
        .collect();

    let mut state = TableState::default();
    state.select(Some(
        app.registries_selected.min(rows.len().saturating_sub(1)),
    ));
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(7),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("HOST"),
            Cell::from("AUTH"),
            Cell::from("USER"),
            Cell::from("SECRET"),
            Cell::from("DEFAULT"),
        ])
        .style(shell_header_style(app)),
    )
    .style(bg)
    .column_spacing(1)
    .row_highlight_style(shell_row_highlight(app))
    .highlight_symbol("");
    f.render_stateful_widget(table, inner, &mut state);
}
