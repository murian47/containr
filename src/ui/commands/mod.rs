//! UI command helpers.
//!
//! This module hosts implementations for individual `:` commands. The main command
//! dispatcher lives in `ui/mod.rs` for now and calls into these helpers. Over time
//! we can move more subcommands here to keep `ui/mod.rs` smaller.

pub(in crate::ui) mod cmdline_cmd;
mod common;
pub(in crate::ui) mod container_cmd;
pub(in crate::ui) mod dashboard_cmd;
pub(in crate::ui) mod git_cmd;
pub(in crate::ui) mod image_cmd;
pub(in crate::ui) mod keymap_cmd;
pub(in crate::ui) mod layout_cmd;
pub(in crate::ui) mod logs_cmd;
pub(in crate::ui) mod network_cmd;
pub(in crate::ui) mod registry_cmd;
pub(in crate::ui) mod server_cmd;
pub(in crate::ui) mod set_cmd;
pub(in crate::ui) mod sidebar_cmd;
pub(in crate::ui) mod templates_cmd;
pub(in crate::ui) mod theme_cmd;
pub(in crate::ui) mod volume_cmd;
