use super::{
    App, MsgLevel, cmdline_is_destructive, is_single_letter_without_modifiers, parse_key_spec,
    parse_scope,
};

impl App {
    pub(super) fn rebuild_keymap(&mut self) {
        self.keymap_parsed.clear();
        self.keymap_defaults = super::build_default_keymap();
        let mut invalid: Vec<(String, String)> = Vec::new();
        for kb in &self.keymap {
            let cmd = kb.cmd.trim();
            let scope = match parse_scope(&kb.scope) {
                Some(s) => s,
                None => {
                    invalid.push((kb.scope.clone(), "invalid scope".to_string()));
                    continue;
                }
            };
            match parse_key_spec(&kb.key) {
                Ok(spec) => {
                    if is_single_letter_without_modifiers(spec) && cmdline_is_destructive(cmd) {
                        invalid.push((
                            format!("{} {}", kb.scope.trim(), kb.key.trim()),
                            "refusing to bind destructive command to unmodified single letter"
                                .to_string(),
                        ));
                        continue;
                    }
                    // Empty cmd means "disabled" for this key in this scope.
                    self.keymap_parsed
                        .insert((scope, spec), cmd.trim_start_matches(':').to_string());
                }
                Err(e) => {
                    invalid.push((kb.key.clone(), e));
                }
            }
        }
        for (key, err) in invalid {
            self.log_msg(
                MsgLevel::Warn,
                format!("invalid key binding '{key}': {err}"),
            );
        }
    }
}
