//! UI command helpers.
//!
//! This module hosts implementations for individual `:` commands. The main command
//! dispatcher lives in `ui/mod.rs` for now and calls into these helpers. Over time
//! we can move more subcommands here to keep `ui/mod.rs` smaller.

pub mod container_cmd;
pub mod image_cmd;
pub mod keymap_cmd;
pub mod layout_cmd;
pub mod logs_cmd;
pub mod network_cmd;
pub mod server_cmd;
pub mod set_cmd;
pub mod sidebar_cmd;
pub mod templates_cmd;
pub mod theme_cmd;
pub mod volume_cmd;
