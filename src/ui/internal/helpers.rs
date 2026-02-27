pub(in crate::ui) use super::super::helpers::{
    build_server_shortcuts, deploy_remote_dir_for, deploy_remote_net_dir_for, ensure_template_id,
    extract_container_ip, extract_template_id, normalize_image_id, parse_kv_args,
    shell_quote_with_home, shell_single_quote, truncate_msg,
};
pub(in crate::ui) use super::super::text_edit::{
    backspace_at_cursor, clamp_cursor_to_text, delete_at_cursor, insert_char_at_cursor,
    set_text_and_cursor,
};
