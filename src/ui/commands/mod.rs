//! UI command helpers.
//!
//! This module hosts implementations for individual `:` commands. The main command
//! dispatcher lives in `ui/mod.rs` for now and calls into these helpers. Over time
//! we can move more subcommands here to keep `ui/mod.rs` smaller.

pub mod theme_cmd;

