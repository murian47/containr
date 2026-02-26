use image::{Rgba, RgbaImage};
use ratatui_image::picker::{Picker, ProtocolType};

use super::render::utils::theme_color_rgba;
use super::{DashboardImageState, theme};

pub(in crate::ui) fn init_dashboard_image(mut picker: Picker, theme: &theme::ThemeSpec) -> DashboardImageState {
    let fallback = Rgba([16, 16, 16, 255]);
    let panel_raw = theme.panel.bg.trim();
    let panel_bg = theme_color_rgba(&theme.panel.bg, fallback);
    let bg = if panel_raw.eq_ignore_ascii_case("default") || panel_raw.eq_ignore_ascii_case("reset") {
        theme_color_rgba(&theme.background.bg, fallback)
    } else {
        panel_bg
    };
    picker.set_background_color(bg);
    let enabled = picker.protocol_type() == ProtocolType::Kitty;
    DashboardImageState {
        enabled,
        picker,
        protocol: None,
        last_key: None,
    }
}

pub(in crate::ui) fn apply_dashboard_theme(state: &mut DashboardImageState, theme: &theme::ThemeSpec) {
    let fallback = Rgba([16, 16, 16, 255]);
    let panel_raw = theme.panel.bg.trim();
    let panel_bg = theme_color_rgba(&theme.panel.bg, fallback);
    let bg = if panel_raw.eq_ignore_ascii_case("default") || panel_raw.eq_ignore_ascii_case("reset") {
        theme_color_rgba(&theme.background.bg, fallback)
    } else {
        panel_bg
    };
    state.picker.set_background_color(bg);
    state.enabled = state.picker.protocol_type() == ProtocolType::Kitty;
    state.protocol = None;
    state.last_key = None;
}

pub(in crate::ui) fn build_dashboard_image(
    theme: &theme::ThemeSpec,
    ratios: &[f32],
    width: u32,
    height: u32,
) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    let fallback_bg = Rgba([16, 16, 16, 255]);
    let panel_raw = theme.panel.bg.trim();
    let bg = if panel_raw.eq_ignore_ascii_case("default") || panel_raw.eq_ignore_ascii_case("reset") {
        theme_color_rgba(&theme.background.bg, fallback_bg)
    } else {
        theme_color_rgba(&theme.panel.bg, fallback_bg)
    };
    let faint = theme_color_rgba(&theme.header.bg, Rgba([40, 40, 40, 255]));
    let ok = theme_color_rgba(&theme.text_ok.fg, Rgba([90, 200, 120, 255]));
    let warn = theme_color_rgba(&theme.text_warn.fg, Rgba([255, 190, 64, 255]));
    let err = theme_color_rgba(&theme.text_error.fg, Rgba([220, 120, 120, 255]));

    for p in img.pixels_mut() {
        *p = bg;
    }

    let mut fill_rect = |x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>| {
        let max_x = width.saturating_sub(1);
        let max_y = height.saturating_sub(1);
        let end_x = (x + w).min(width);
        let end_y = (y + h).min(height);
        for yy in y..end_y {
            if yy > max_y {
                break;
            }
            for xx in x..end_x {
                if xx > max_x {
                    break;
                }
                img.put_pixel(xx, yy, color);
            }
        }
    };

    let lerp = |a: u8, b: u8, t: f32| -> u8 {
        let t = t.clamp(0.0, 1.0);
        (a as f32 + (b as f32 - a as f32) * t).round() as u8
    };
    let lerp_rgba = |a: Rgba<u8>, b: Rgba<u8>, t: f32| -> Rgba<u8> {
        Rgba([
            lerp(a[0], b[0], t),
            lerp(a[1], b[1], t),
            lerp(a[2], b[2], t),
            255,
        ])
    };

    let ratios: Vec<f32> = ratios.iter().map(|r| r.clamp(0.0, 1.0)).collect();
    if ratios.is_empty() {
        return img;
    }
    let margin_x = 2u32;
    let bar_w = width.saturating_sub(margin_x * 2);
    let rows = ratios.len().max(1) as u32;
    let row_h = (height / rows).max(1);
    let pad = (row_h / 6).min(2);
    let bar_h = row_h.saturating_sub(pad * 2).max(3);
    for (idx, ratio) in ratios.iter().enumerate() {
        let row_top = idx as u32 * row_h;
        let y = row_top + (row_h.saturating_sub(bar_h)) / 2;
        fill_rect(margin_x, y, bar_w, bar_h, faint);
        let fill_w = ((bar_w as f32) * ratio).round() as u32;
        for xx in 0..fill_w {
            let t = if bar_w <= 1 { 1.0 } else { (xx as f32) / (bar_w as f32 - 1.0) };
            let color = if t <= 0.7 {
                lerp_rgba(ok, warn, t / 0.7)
            } else {
                lerp_rgba(warn, err, (t - 0.7) / 0.3)
            };
            fill_rect(margin_x + xx, y, 1, bar_h, color);
        }
    }

    img
}
