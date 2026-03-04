//! Central application metadata and naming-related helpers.
//!
//! Keep product- and namespace-related strings here so future renames do not require
//! repository-wide literal replacement in runtime code.

use std::path::{Path, PathBuf};

pub const PRODUCT_NAME: &str = env!("CARGO_PKG_NAME");

// These namespaces may stay stable even if the public product name changes later.
pub const CONFIG_NAMESPACE: &str = "containr";
pub const KEYRING_SERVICE: &str = "containr";
pub const TEMPLATE_ID_MARKER: &str = "containr_template_id";
pub const TEMPLATE_LABEL_ID: &str = "app.containr.template_id";
pub const TEMPLATE_LABEL_COMMIT: &str = "app.containr.commit";

pub const AGE_IDENTITY_FILE: &str = "age.key";
pub const COMPOSE_TEMPFILE_PREFIX: &str = "app-compose-";

pub fn config_root_from_xdg(dir: &Path) -> PathBuf {
    dir.join(CONFIG_NAMESPACE)
}

pub fn config_root_from_home(home: &Path) -> PathBuf {
    home.join(".config").join(CONFIG_NAMESPACE)
}

pub fn state_root_from_xdg(dir: &Path) -> PathBuf {
    dir.join(CONFIG_NAMESPACE)
}

pub fn state_root_from_home(home: &Path) -> PathBuf {
    home.join(".local").join("state").join(CONFIG_NAMESPACE)
}

pub fn default_age_identity_path() -> String {
    format!("~/.config/{CONFIG_NAMESPACE}/{AGE_IDENTITY_FILE}")
}

pub fn system_theme_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/local/share")
            .join(CONFIG_NAMESPACE)
            .join("themes"),
        PathBuf::from("/usr/share")
            .join(CONFIG_NAMESPACE)
            .join("themes"),
    ]
}

pub fn apps_rel_dir(name: &str) -> String {
    format!(".config/{CONFIG_NAMESPACE}/apps/{name}")
}

pub fn networks_rel_dir(name: &str) -> String {
    format!(".config/{CONFIG_NAMESPACE}/networks/{name}")
}

pub fn apps_dir_under_home(home: &str, name: &str) -> String {
    config_root_from_home(Path::new(home))
        .join("apps")
        .join(name)
        .to_string_lossy()
        .to_string()
}

pub fn networks_dir_under_home(home: &str, name: &str) -> String {
    config_root_from_home(Path::new(home))
        .join("networks")
        .join(name)
        .to_string_lossy()
        .to_string()
}

pub fn docker_no_creds_dir(home: &str) -> PathBuf {
    config_root_from_home(Path::new(home)).join("docker-no-creds")
}
