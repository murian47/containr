mod data;
mod image;
mod state;

pub(in crate::ui) use data::{dashboard_command, parse_dashboard_output};
pub(in crate::ui) use image::{apply_dashboard_theme, build_dashboard_image, init_dashboard_image};
