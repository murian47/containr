//! Template feature surface.
//!
//! This module exposes stack/network template labeling, filesystem operations, export helpers, and
//! derived metadata used by deploy, git, and UI flows.

mod labels;
pub(in crate::ui) mod ops;
mod state;

pub(in crate::ui) use labels::{
    render_compose_with_template_id, template_commit_from_labels, template_id_from_labels,
};
pub(in crate::ui) use ops::{
    create_net_template, create_template, delete_net_template, delete_template,
    export_net_template, export_stack_template, extract_net_template_description,
    extract_template_description, images_from_compose, maybe_autocommit_templates,
};
