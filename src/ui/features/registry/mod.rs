//! Registry feature support.
//!
//! Network-facing registry checks and local registry/auth state management live here, separated
//! from the command layer that exposes them to the user.

mod http;
mod state;

pub(in crate::ui) use http::registry_test;
