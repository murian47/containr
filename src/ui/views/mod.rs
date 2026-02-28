//! View modules (Phase 1 scaffold)
//!
//! Goal: keep view-specific entry points grouped by view without changing behavior.
//! Rendering still lives in `ui/render/*`; these modules are the view-oriented layer.

pub mod containers;
pub mod dashboard;
pub mod help;
pub mod images;
pub mod inspect;
pub mod logs;
pub mod messages;
pub mod networks;
pub mod registries;
pub mod stacks;
pub mod templates;
pub mod themes;
pub mod volumes;
