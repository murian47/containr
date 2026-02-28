use std::collections::HashMap;

use crate::config;
use crate::docker::ImageRow;
use crate::ui::core::secrets::{decrypt_age_secret, load_age_identities};
use crate::ui::core::types::RegistryAuthResolved;
use crate::ui::render::utils::expand_user_path;
use crate::ui::state::app::App;
use crate::ui::state::shell_types::MsgLevel;

impl App {
    pub(in crate::ui) fn registry_env_secret(&self, key: &str) -> anyhow::Result<String> {
        let env_key = key.trim();
        if env_key.is_empty() {
            anyhow::bail!("empty env key");
        }
        std::env::var(env_key).map_err(|e| anyhow::anyhow!("env {env_key} not set: {e}"))
    }

    pub(in crate::ui) fn registry_keyring_secret(&self, key: &str) -> anyhow::Result<String> {
        let entry = keyring::Entry::new("containr", key)
            .map_err(|e| anyhow::anyhow!("keyring init failed: {e}"))?;
        entry
            .get_password()
            .map_err(|e| anyhow::anyhow!("keyring read failed: {e}"))
    }

    pub(in crate::ui) fn resolve_registry_auths(&mut self) {
        let mut identities: Vec<Box<dyn age::Identity>> = Vec::new();
        let identity_path = self.registries_cfg.age_identity.trim();
        if !identity_path.is_empty() {
            let path = expand_user_path(identity_path);
            match load_age_identities(&path) {
                Ok(ids) => {
                    identities = ids;
                }
                Err(e) => {
                    self.log_msg(
                        MsgLevel::Warn,
                        format!("registry identities load failed: {:#}", e),
                    );
                }
            }
        }

        let mut out: HashMap<String, RegistryAuthResolved> = HashMap::new();
        let entries = self.registries_cfg.registries.clone();
        for entry in entries {
            let host = entry.host.trim().to_ascii_lowercase();
            if host.is_empty() {
                continue;
            }
            let mut secret_plain: Option<String> = None;
            if !matches!(entry.auth, config::RegistryAuth::Anonymous) {
                if let Some(key_name) = entry
                    .secret_keyring
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                {
                    match self.registry_keyring_secret(&key_name) {
                        Ok(s) => secret_plain = Some(s),
                        Err(e) => {
                            self.log_msg(
                                MsgLevel::Warn,
                                format!("registry keyring read failed for {host}: {:#}", e),
                            );
                            // Fallback: ENV with same key name to avoid breaking existing setups.
                            if let Ok(s) = self.registry_env_secret(&key_name) {
                                secret_plain = Some(s);
                            }
                        }
                    }
                } else if let Some(secret) = entry.secret.as_ref().map(|s| s.trim().to_string()) {
                    if identities.is_empty() {
                        self.log_msg(
                            MsgLevel::Warn,
                            format!("registry secret ignored (no identity): {host}"),
                        );
                    } else {
                        match decrypt_age_secret(&secret, &identities) {
                            Ok(text) => secret_plain = Some(text),
                            Err(e) => self.log_msg(
                                MsgLevel::Warn,
                                format!("registry secret decrypt failed for {host}: {:#}", e),
                            ),
                        }
                    }
                } else {
                    self.log_msg(
                        MsgLevel::Warn,
                        format!("registry secret missing for {host}"),
                    );
                }
            }

            out.insert(
                host.clone(),
                RegistryAuthResolved {
                    auth: entry.auth.clone(),
                    username: entry.username.clone(),
                    secret: secret_plain,
                },
            );
        }
        if !out.is_empty() {
            let mut anonymous = 0usize;
            let mut basic = 0usize;
            let mut bearer = 0usize;
            let mut ghcr = 0usize;
            let mut with_secret = 0usize;
            let mut with_username = 0usize;
            for v in out.values() {
                match v.auth {
                    config::RegistryAuth::Anonymous => anonymous += 1,
                    config::RegistryAuth::Basic => basic += 1,
                    config::RegistryAuth::BearerToken => bearer += 1,
                    config::RegistryAuth::GithubPat => ghcr += 1,
                }
                if v.secret.is_some() {
                    with_secret += 1;
                }
                if v.username.is_some() {
                    with_username += 1;
                }
            }
            self.log_msg(
                MsgLevel::Info,
                format!(
                    "registries loaded: {} (anon={anonymous} basic={basic} bearer={bearer} ghcr={ghcr} secrets={with_secret} users={with_username})",
                    out.len()
                ),
            );
        }
        self.registry_auths = out;
    }

    pub(in crate::ui) fn registry_auth_for_host(
        &self,
        host: &str,
    ) -> anyhow::Result<RegistryAuthResolved> {
        let host = host.trim().to_ascii_lowercase();
        let entry = self
            .registries_cfg
            .registries
            .iter()
            .find(|r| r.host.trim().eq_ignore_ascii_case(&host))
            .ok_or_else(|| anyhow::anyhow!("registry not found: {host}"))?;
        let mut auth = RegistryAuthResolved {
            auth: entry.auth.clone(),
            username: entry.username.clone(),
            secret: None,
        };
        if !matches!(auth.auth, config::RegistryAuth::Anonymous)
            && let Some(resolved) = self.registry_auths.get(&host)
        {
            auth.secret = resolved.secret.clone();
        }
        match auth.auth {
            config::RegistryAuth::Anonymous => Ok(auth),
            config::RegistryAuth::Basic | config::RegistryAuth::GithubPat => {
                if auth.username.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("registry username missing for {host}");
                }
                if auth.secret.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("registry secret missing for {host}");
                }
                Ok(auth)
            }
            config::RegistryAuth::BearerToken => {
                if auth.secret.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("registry token missing for {host}");
                }
                Ok(auth)
            }
        }
    }

    pub(in crate::ui) fn registry_default_host(&self) -> Option<String> {
        let default = self
            .registries_cfg
            .default_registry
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())?;
        if self
            .registries_cfg
            .registries
            .iter()
            .any(|r| r.host.eq_ignore_ascii_case(default))
        {
            Some(default.to_string())
        } else {
            None
        }
    }

    pub(in crate::ui) fn image_row_key(img: &ImageRow) -> String {
        if img.repository != "<none>" && img.tag != "<none>" && !img.tag.trim().is_empty() {
            format!("ref:{}:{}", img.repository, img.tag)
        } else {
            format!("id:{}", img.id)
        }
    }

    pub(in crate::ui) fn image_row_ref(img: &ImageRow) -> Option<String> {
        if img.repository != "<none>" && img.tag != "<none>" && !img.tag.trim().is_empty() {
            Some(format!("{}:{}", img.repository, img.tag))
        } else {
            None
        }
    }
}
