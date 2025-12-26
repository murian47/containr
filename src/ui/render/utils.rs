use anyhow::Context as _;
use std::fs;
use std::path::PathBuf;

pub fn write_text_file(path: &str, text: &str, force: bool) -> anyhow::Result<PathBuf> {
    let path = path.trim();
    anyhow::ensure!(!path.is_empty(), "missing file path");

    let path = expand_user_path(path);
    if path.exists() && !force {
        anyhow::bail!("file exists (use save! to overwrite)");
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).context("failed to create parent dir")?;
        }
    }
    fs::write(&path, text).context("failed to write file")?;
    Ok(path)
}

pub fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
