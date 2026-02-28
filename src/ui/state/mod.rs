//! UI state layer.
//!
//! This module groups persistent and transient state used by the shell UI:
//! - `app`: the central state container
//! - `shell_types`: view and overlay specific state types
//! - `actions` / `image_updates` / `persistence`: state mutations and persistence helpers

pub mod actions;
pub(in crate::ui) mod app;
pub mod image_updates;
pub mod persistence;
pub mod shell_types;
