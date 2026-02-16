#![allow(dead_code)]

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};
use ratatui_image::{Resize, StatefulImage};

use crate::ui::format_session_ts;
use crate::ui::render::format::{bar_spans_gradient, bar_spans_threshold, format_bytes_short};
use crate::ui::render::utils::{theme_color, truncate_end};
use crate::ui::theme;
use crate::ui::{current_server_label, App};

/// Dashboard render implementation (moved from render.inc.rs).
pub(in crate::ui) fn render_dashboard_impl(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: Rect,
) {
    let bg = app.theme.panel.to_style();
    f.render_widget(Block::default().style(bg), area);
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let mut show_image = app.dashboard_image_enabled() && inner.width >= 60 && inner.height >= 12;
    if app.dashboard.suppress_image_frames > 0 {
        app.dashboard.suppress_image_frames =
            app.dashboard.suppress_image_frames.saturating_sub(1);
        show_image = false;
    }
    let content_area = inner;
    if app.servers.is_empty() && app.current_target.trim().is_empty() {
        let msg = "No server configured. Use :server add to get started.";
        f.render_widget(
            Paragraph::new(msg)
                .style(bg.patch(app.theme.text_dim.to_style()))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            content_area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // health strip
            Constraint::Length(1), // spacer
            Constraint::Length(7), // summary table
            Constraint::Length(1), // spacer
            Constraint::Length(7), // metrics table
            Constraint::Min(1),    // notes
        ])
        .split(content_area);

    let ok = bg.patch(app.theme.text_ok.to_style());
    let warn = bg.patch(app.theme.text_warn.to_style());
    let err = bg.patch(app.theme.text_error.to_style());
    let dim = bg.patch(app.theme.text_dim.to_style());
    let faint = bg.patch(app.theme.text_faint.to_style());

    let ssh_ok = app.conn_error.is_none();
    let dash_ok = app.dashboard.error.is_none() && app.dashboard.snap.is_some();
    let snap = app.dashboard.snap.as_ref();

    let engine_ok = dash_ok && snap.is_some_and(|s| !s.engine.trim().is_empty() && s.engine != "-");
    let disk_ratio = snap
        .and_then(|s| {
            if s.disk_total_bytes == 0 {
                None
            } else {
                Some((s.disk_used_bytes as f32) / (s.disk_total_bytes as f32))
            }
        })
        .unwrap_or(0.0);
    let mem_ratio = snap
        .and_then(|s| {
            if s.mem_total_bytes == 0 {
                None
            } else {
                Some((s.mem_used_bytes as f32) / (s.mem_total_bytes as f32))
            }
        })
        .unwrap_or(0.0);

    let disk_total = snap.map(|s| s.disk_total_bytes).unwrap_or(0);
    let mem_total = snap.map(|s| s.mem_total_bytes).unwrap_or(0);
    let disk_style = if !dash_ok || disk_total == 0 {
        warn
    } else if disk_ratio >= 0.9 {
        err
    } else if disk_ratio >= 0.8 {
        warn
    } else {
        ok
    };
    let mem_style = if !dash_ok || mem_total == 0 {
        warn
    } else if mem_ratio >= 0.9 {
        err
    } else if mem_ratio >= 0.8 {
        warn
    } else {
        ok
    };

    let badge = |label: &str, st: Style| -> Span<'static> {
        Span::styled(format!("[ {label} ]"), st)
    };

    let mut strip: Vec<Span<'static>> = Vec::new();
    strip.push(badge(
        if ssh_ok { "SSH OK" } else { "SSH ERR" },
        if ssh_ok { ok } else { err },
    ));
    strip.push(Span::styled(" ", dim));
    strip.push(badge(
        if engine_ok { "ENGINE OK" } else { "ENGINE ?" },
        if engine_ok { ok } else { warn },
    ));
    strip.push(Span::styled(" ", dim));
    strip.push(badge(
        if disk_style == ok {
            "DISK OK"
        } else if disk_style == err {
            "DISK ERR"
        } else {
            "DISK WARN"
        },
        disk_style,
    ));
    strip.push(Span::styled(" ", dim));
    strip.push(badge(
        if mem_style == ok {
            "MEM OK"
        } else if mem_style == err {
            "MEM ERR"
        } else {
            "MEM WARN"
        },
        mem_style,
    ));
    let unseen_err = app.unseen_error_count();
    if unseen_err > 0 {
        strip.push(Span::styled(" ", dim));
        strip.push(badge(&format!("ERR {unseen_err}"), err));
    }
    f.render_widget(
        Paragraph::new(Line::from(strip)).style(bg).wrap(Wrap { trim: false }),
        chunks[0],
    );

    // Spacer line.
    f.render_widget(Paragraph::new(" ").style(bg), chunks[1]);

    // Summary.
    let (os, kernel, arch, uptime, engine, ts, load1, load5, load15, cores) = if let Some(s) = snap
    {
        (
            s.os.as_str(),
            s.kernel.as_str(),
            s.arch.as_str(),
            s.uptime.as_str(),
            s.engine.as_str(),
            format_session_ts(s.collected_at),
            s.load1,
            s.load5,
            s.load15,
            s.cpu_cores,
        )
    } else if app.dashboard.loading {
        ("Loading...", "-", "-", "-", "-", "-".to_string(), 0.0, 0.0, 0.0, 1)
    } else {
        ("-", "-", "-", "-", "-", "-".to_string(), 0.0, 0.0, 0.0, 1)
    };

    let server = current_server_label(app);
    // Container counts derived from current list (ps -a).
    let mut running = 0usize;
    let mut exited = 0usize;
    let mut paused = 0usize;
    let mut dead = 0usize;
    for c in &app.containers {
        let s = c.status.trim();
        if s.starts_with("Up") || s.starts_with("Restarting") {
            running += 1;
        } else if s.starts_with("Exited") {
            exited += 1;
        } else if s.starts_with("Paused") {
            paused += 1;
        } else if s.starts_with("Dead") {
            dead += 1;
        } else {
            exited += 1;
        }
    }
    let total = app.containers.len();

    let table_w = inner.width.max(1) as usize;
    let key_w = 12usize.min(table_w.saturating_sub(1).max(1));
    let val_w = table_w.saturating_sub(key_w + 1).max(1);
    let k = dim;
    let v = bg.patch(app.theme.text.to_style());
    let summary_rows: Vec<Row> = vec![
        Row::new(vec![
            Cell::from(Span::styled("Server", k)),
            Cell::from(Span::styled(truncate_end(&server, val_w), v)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Host", k)),
            Cell::from(Span::styled(
                truncate_end(&format!("{os} ({kernel} {arch})"), val_w),
                v,
            )),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Uptime", k)),
            Cell::from(Span::styled(truncate_end(uptime, val_w), v)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Engine", k)),
            Cell::from(Span::styled(truncate_end(engine, val_w), v)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Containers", k)),
            Cell::from(Span::styled(
                truncate_end(
                    &format!(
                        "running {running}/{total}  exited {exited}  paused {paused}  dead {dead}"
                    ),
                    val_w,
                ),
                v,
            )),
        ]),
        Row::new(vec![
            Cell::from(Span::styled("Updated", k)),
            Cell::from(Span::styled(truncate_end(&ts, val_w), faint)),
        ]),
    ];
    let summary = Table::new(
        summary_rows,
        [Constraint::Length(key_w as u16), Constraint::Min(1)],
    )
    .style(bg)
    .column_spacing(1);
    f.render_widget(summary, chunks[2]);

    f.render_widget(Paragraph::new(" ").style(bg), chunks[3]);

    // Metrics table (label | value | bar).
    let load_ratio = if cores == 0 {
        0.0
    } else {
        (load1 / (cores as f32)).clamp(0.0, 1.0)
    };
    let (mem_used, mem_total2, disk_used, disk_total2) = snap
        .map(|s| (s.mem_used_bytes, s.mem_total_bytes, s.disk_used_bytes, s.disk_total_bytes))
        .unwrap_or((0, 0, 0, 0));
    let mem_ratio2 = if mem_total2 == 0 {
        0.0
    } else {
        (mem_used as f32) / (mem_total2 as f32)
    };
    let disk_ratio2 = if disk_total2 == 0 {
        0.0
    } else {
        (disk_used as f32) / (disk_total2 as f32)
    };

    let metrics_w = inner.width.max(1) as usize;
    let m_key_w = key_w;
    let m_val_w = 20usize.min(metrics_w.saturating_sub(m_key_w + 2).max(10));
    let m_bar_w = metrics_w.saturating_sub(m_key_w + m_val_w + 2).max(10);
    let mk = dim;
    let mv = v;
    let header_bg = theme::parse_color(&app.theme.header.bg);
    let bar_empty = bg.fg(header_bg);
    let bar_ok = if app.kitty_graphics {
        bg.fg(theme_color(&app.theme.text_ok.fg))
    } else {
        bg.patch(app.theme.text_ok.to_style())
    };
    let bar_warn = if app.kitty_graphics {
        bg.fg(theme_color(&app.theme.text_warn.fg))
    } else {
        bg.patch(app.theme.text_warn.to_style())
    };
    let bar_err = if app.kitty_graphics {
        bg.fg(theme_color(&app.theme.text_error.fg))
    } else {
        bg.patch(app.theme.text_error.to_style())
    };

    let metric_row =
        |name: &str, val: String, bar: Vec<Span<'static>>, extra: Option<String>| -> Row<'static> {
        let mut val = truncate_end(&val, m_val_w);
        if let Some(extra) = extra {
            if !extra.trim().is_empty() {
                let extra = format!(" {extra}");
                val = truncate_end(&(val + &extra), m_val_w);
            }
        }
        let name = truncate_end(name, m_key_w);
        Row::new(vec![
            Cell::from(Span::styled(name, mk)),
            Cell::from(Span::styled(val, mv)),
            Cell::from(Line::from(bar)),
        ])
    };
    let metric_row_text = |name: &str, val: String, extra: Option<String>| -> Row<'static> {
        let mut val = truncate_end(&val, m_val_w);
        if let Some(extra) = extra {
            if !extra.trim().is_empty() {
                let extra = format!(" {extra}");
                val = truncate_end(&(val + &extra), m_val_w);
            }
        }
        let name = truncate_end(name, m_key_w);
        Row::new(vec![
            Cell::from(Span::styled(name, mk)),
            Cell::from(Span::styled(val, mv)),
        ])
    };

    let cpu_val = format!("{load1:.2}/{load5:.2}/{load15:.2}");
    let mem_val = format!(
        "{}/{} {:>3.0}%",
        format_bytes_short(mem_used),
        format_bytes_short(mem_total2),
        mem_ratio2 * 100.0
    );
    let dsk_val = format!(
        "{}/{} {:>3.0}%",
        format_bytes_short(disk_used),
        format_bytes_short(disk_total2),
        disk_ratio2 * 100.0
    );

    let cpu_fill = if load_ratio >= 0.85 {
        bar_err
    } else if load_ratio >= 0.70 {
        bar_warn
    } else {
        bar_ok
    };
    let cpu_bar = bar_spans_threshold(m_bar_w, load_ratio, app.ascii_only, cpu_fill, bar_empty);
    let mem_fill = if mem_ratio2 >= 0.85 {
        bar_err
    } else if mem_ratio2 >= 0.70 {
        bar_warn
    } else {
        bar_ok
    };
    let mem_bar = bar_spans_threshold(m_bar_w, mem_ratio2, app.ascii_only, mem_fill, bar_empty);

    let mut metric_rows: Vec<Row> = vec![
        metric_row("CPU", cpu_val.clone(), cpu_bar, Some(format!("{cores}c"))),
        metric_row("MEM", mem_val.clone(), mem_bar, None),
    ];
    let mut metric_rows_text: Vec<Row> = vec![
        metric_row_text("CPU", cpu_val, Some(format!("{cores}c"))),
        metric_row_text("MEM", mem_val, None),
    ];
    if let Some(s) = snap {
        for (idx, disk) in s.disks.iter().enumerate() {
            let total = disk.total_bytes.max(1);
            let ratio = (disk.used_bytes as f32) / (total as f32);
            let val = format!(
                "{}/{} {:>3.0}%",
                format_bytes_short(disk.used_bytes),
                format_bytes_short(disk.total_bytes),
                ratio * 100.0
            );
            let label = if idx == 0 { "DSK" } else { "" };
            let dsk_bar = bar_spans_gradient(
                m_bar_w,
                ratio,
                app.ascii_only,
                bar_ok,
                bar_warn,
                bar_err,
                bar_empty,
            );
            metric_rows.push(metric_row(&label, val.clone(), dsk_bar, None));
            metric_rows_text.push(metric_row_text(&label, val, None));
        }
        for (idx, nic) in s.nics.iter().take(3).enumerate() {
            let label = if idx == 0 {
                format!("NIC ({})", nic.name)
            } else {
                format!("({})", nic.name)
            };
            let val = nic.addr.clone();
            metric_rows.push(metric_row(&label, val, Vec::new(), None));
            metric_rows_text.push(metric_row_text(&label, nic.addr.clone(), None));
        }
    } else {
        let dsk_bar = bar_spans_gradient(
            m_bar_w,
            disk_ratio2,
            app.ascii_only,
            bar_ok,
            bar_warn,
            bar_err,
            bar_empty,
        );
        metric_rows.push(metric_row("DSK", dsk_val.clone(), dsk_bar, None));
        metric_rows_text.push(metric_row_text("DSK", dsk_val, None));
    }
    let metrics = Table::new(
        metric_rows,
        [
            Constraint::Length(m_key_w as u16),
            Constraint::Length(m_val_w as u16),
            Constraint::Min(1),
        ],
    )
    .style(bg)
    .column_spacing(1);
    if show_image {
        let text_w = m_key_w as u16 + m_val_w as u16 + 1;
        let metric_parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(text_w), Constraint::Min(10)])
            .split(chunks[4]);
        let metrics = Table::new(
            metric_rows_text,
            [
                Constraint::Length(m_key_w as u16),
                Constraint::Length(m_val_w as u16),
            ],
        )
        .style(bg)
        .column_spacing(1);
        f.render_widget(metrics, metric_parts[0]);

        let last = app.dashboard.last_disk_count.max(1);
        let cur = snap.map(|s| s.disks.len().max(1)).unwrap_or(1);
        let disk_rows = last.max(cur);
        let bar_rows = 2 + disk_rows;
        if (bar_rows as u16) <= metric_parts[1].height {
            let image_area = Rect {
                x: metric_parts[1].x,
                y: metric_parts[1].y,
                width: metric_parts[1].width,
                height: bar_rows as u16,
            };
            app.update_dashboard_image(image_area);
            if let Some(state) = app
                .dashboard_image
                .as_mut()
                .and_then(|img| img.protocol.as_mut())
            {
                let image = StatefulImage::default().resize(Resize::Fit(None));
                f.render_stateful_widget(image, image_area, state);
            }
        }
    } else {
        f.render_widget(metrics, chunks[4]);
    }

    if let Some(err) = &app.dashboard.error {
        let msg = truncate_end(err, inner.width.max(1) as usize);
        f.render_widget(
            Paragraph::new(format!("Dashboard error: {msg}"))
                .style(bg.patch(app.theme.text_warn.to_style()))
                .wrap(Wrap { trim: true }),
            chunks[5],
        );
    }
}

/// Public wrapper for callers (keeps existing call sites stable).
pub fn render_dashboard(f: &mut Frame, app: &mut App, area: Rect) {
    render_dashboard_impl(f, app, area);
}
