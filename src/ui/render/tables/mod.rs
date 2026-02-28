pub mod common;
pub mod containers;
pub mod images;
pub mod networks;
pub mod volumes;

pub(in crate::ui) use common::shell_header_style;
pub(in crate::ui) use containers::draw_shell_containers_table;
pub(in crate::ui) use images::draw_shell_images_table;
pub(in crate::ui) use networks::draw_shell_networks_table;
pub(in crate::ui) use volumes::draw_shell_volumes_table;
