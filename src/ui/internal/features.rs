pub(in crate::ui) use super::super::features::dashboard::{
    dashboard_command, parse_dashboard_output,
};
pub(in crate::ui) use super::super::features::registry::registry_test;
pub(in crate::ui) use super::super::features::templates::{
    create_net_template, create_template, delete_net_template, delete_template,
    export_net_template, export_stack_template, images_from_compose, maybe_autocommit_templates,
    render_compose_with_template_id, template_commit_from_labels, template_id_from_labels,
};
