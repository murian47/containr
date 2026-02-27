use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::ui::App;
use crate::ui::theme;

pub(in crate::ui) fn header_logo_spans(app: &App, base: Style, shown: &str) -> Vec<Span<'static>> {
    // Render the "CONTAINR" logo in per-run colors without changing background.
    let bg = theme::parse_color(&app.theme.header.bg);
    let bg_rgb = match bg {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    };
    let is_dark = bg_rgb.map(|(r, g, b)| rel_luma(r, g, b) < 0.55).unwrap_or(true);

    let bright_palette: [Color; 8] = [
        Color::Rgb(255, 95, 86),  // red
        Color::Rgb(255, 189, 46), // yellow
        Color::Rgb(39, 201, 63),  // green
        Color::Rgb(64, 156, 255), // blue
        Color::Rgb(175, 82, 222), // purple
        Color::Rgb(255, 105, 180), // pink
        Color::Rgb(0, 212, 212),  // cyan
        Color::Rgb(255, 255, 255), // white
    ];
    let dark_palette: [Color; 8] = [
        Color::Rgb(120, 20, 20),
        Color::Rgb(120, 80, 0),
        Color::Rgb(0, 90, 40),
        Color::Rgb(0, 60, 120),
        Color::Rgb(70, 30, 110),
        Color::Rgb(120, 30, 70),
        Color::Rgb(0, 90, 90),
        Color::Rgb(0, 0, 0),
    ];
    let palette: &[Color] = if is_dark { &bright_palette } else { &dark_palette };

    let seed = app.header_logo_seed;
    let offset = (seed as usize) % palette.len();
    // Ensure we don't fall into short cycles (e.g. len=8, step=2 repeats every 4).
    let mut step = (((seed >> 8) as usize) % (palette.len().saturating_sub(1)).max(1)).max(1);
    step = coprime_step(step, palette.len());

    let mut out: Vec<Span<'static>> = Vec::new();
    let mut letter_i = 0usize;
    for ch in shown.chars() {
        if ch.is_ascii_alphabetic() {
            let mut c = palette[(offset + letter_i.saturating_mul(step)) % palette.len()];
            if let Some((br, bg, bb)) = bg_rgb {
                let ratio = contrast_ratio((br, bg, bb), c);
                if ratio < 3.0 {
                    c = if is_dark { Color::White } else { Color::Black };
                }
            }
            out.push(Span::styled(
                ch.to_string(),
                base.fg(c).add_modifier(Modifier::BOLD),
            ));
            letter_i = letter_i.saturating_add(1);
        } else {
            out.push(Span::styled(ch.to_string(), base));
        }
    }
    out
}

fn coprime_step(mut step: usize, len: usize) -> usize {
    if len <= 1 {
        return 1;
    }
    step = step.clamp(1, len - 1);
    while gcd(step, len) != 1 {
        step += 1;
        if step >= len {
            step = 1;
        }
    }
    step
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn rel_luma(r: u8, g: u8, b: u8) -> f32 {
    fn to_lin(u: u8) -> f32 {
        let c = (u as f32) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * to_lin(r) + 0.7152 * to_lin(g) + 0.0722 * to_lin(b)
}

fn contrast_ratio(bg: (u8, u8, u8), fg: Color) -> f32 {
    let fg = match fg {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        _ => (255, 255, 255),
    };
    let l1 = rel_luma(bg.0, bg.1, bg.2);
    let l2 = rel_luma(fg.0, fg.1, fg.2);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}
