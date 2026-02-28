use image::DynamicImage;

use super::{apply_dashboard_theme, build_dashboard_image, init_dashboard_image};
use crate::ui::App;

impl App {
    pub(in crate::ui) fn dashboard_image_enabled(&self) -> bool {
        if !self.kitty_graphics {
            return false;
        }
        self.dashboard_image
            .as_ref()
            .map(|state| state.enabled)
            .unwrap_or(false)
    }

    pub(in crate::ui) fn set_kitty_graphics(&mut self, enabled: bool) -> bool {
        if enabled {
            if self.ascii_only {
                return false;
            }
            if self.dashboard_image.is_none() {
                let picker = ratatui_image::picker::Picker::from_query_stdio().ok();
                if let Some(p) = picker {
                    self.dashboard_image = Some(init_dashboard_image(p, &self.theme));
                } else {
                    return false;
                }
            }
            self.kitty_graphics = true;
            self.reset_dashboard_image();
        } else {
            self.kitty_graphics = false;
            self.dashboard_image = None;
        }
        true
    }

    pub(in crate::ui) fn update_dashboard_image(&mut self, area: ratatui::layout::Rect) {
        let Some(state) = &mut self.dashboard_image else {
            return;
        };
        if !state.enabled {
            return;
        }
        let Some(snap) = self.dashboard.snap.as_ref() else {
            return;
        };
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mem_ratio = if snap.mem_total_bytes == 0 {
            0.0
        } else {
            (snap.mem_used_bytes as f32) / (snap.mem_total_bytes as f32)
        };
        let cpu_ratio = if snap.cpu_cores == 0 {
            0.0
        } else {
            (snap.load1 / (snap.cpu_cores as f32)).clamp(0.0, 1.0)
        };
        let mut ratios: Vec<f32> = Vec::new();
        ratios.push(cpu_ratio);
        ratios.push(mem_ratio);
        if snap.disks.is_empty() {
            let disk_ratio = if snap.disk_total_bytes == 0 {
                0.0
            } else {
                (snap.disk_used_bytes as f32) / (snap.disk_total_bytes as f32)
            };
            ratios.push(disk_ratio);
        } else {
            for disk in &snap.disks {
                let total = disk.total_bytes.max(1) as f32;
                ratios.push((disk.used_bytes as f32) / total);
            }
        }
        let key = format!(
            "{:.2}-{:.2}-{}-{}x{}",
            cpu_ratio,
            mem_ratio,
            ratios.len(),
            area.width,
            area.height
        );
        if state.last_key.as_deref() == Some(&key) {
            return;
        }
        let (fw, fh) = state.picker.font_size();
        let px_w = (area.width as u32).saturating_mul(fw.max(1) as u32);
        let px_h = (area.height as u32).saturating_mul(fh.max(1) as u32);
        if px_w == 0 || px_h == 0 {
            return;
        }
        let img = build_dashboard_image(&self.theme, &ratios, px_w, px_h);
        let dyn_img = DynamicImage::ImageRgba8(img);
        state.protocol = Some(state.picker.new_resize_protocol(dyn_img));
        state.last_key = Some(key);
    }

    pub(in crate::ui) fn reset_dashboard_image(&mut self) {
        if let Some(state) = &mut self.dashboard_image {
            apply_dashboard_theme(state, &self.theme);
            state.protocol = None;
            state.last_key = None;
        }
    }
}
