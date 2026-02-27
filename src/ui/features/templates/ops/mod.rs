//! Template import/export helpers.

mod common;
mod export;
mod template_fs;

pub(in crate::ui) use export::{export_net_template, export_stack_template};
pub(in crate::ui) use template_fs::{
    create_net_template, create_template, delete_net_template, delete_template,
    extract_net_template_description, extract_template_description, images_from_compose,
    maybe_autocommit_templates,
};
