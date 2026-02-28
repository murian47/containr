use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;

use age::armor::{ArmoredWriter, Format};
use age::secrecy::ExposeSecret;
use age::x25519;
use age::{Decryptor, Encryptor};
use anyhow::Context as _;

pub(in crate::ui) fn load_age_identities(
    path: &Path,
) -> anyhow::Result<Vec<Box<dyn age::Identity>>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read age identity: {}", path.display()))?;
    let mut ids: Vec<Box<dyn age::Identity>> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with("AGE-SECRET-KEY-") {
            continue;
        }
        let id = x25519::Identity::from_str(line)
            .map_err(|_| anyhow::anyhow!("invalid age identity"))?;
        ids.push(Box::new(id));
    }
    anyhow::ensure!(!ids.is_empty(), "no age identities found");
    Ok(ids)
}

pub(in crate::ui) fn ensure_age_identity(path: &Path) -> anyhow::Result<x25519::Identity> {
    if path.exists() {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read age identity: {}", path.display()))?;
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !line.starts_with("AGE-SECRET-KEY-") {
                continue;
            }
            let id = x25519::Identity::from_str(line)
                .map_err(|_| anyhow::anyhow!("invalid age identity"))?;
            return Ok(id);
        }
        anyhow::bail!("no age identities found");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create identity dir: {}", parent.display()))?;
    }
    let id = x25519::Identity::generate();
    let id_line = id.to_string();
    let content = format!("# containr age identity\n{}\n", id_line.expose_secret());
    fs::write(path, content)
        .with_context(|| format!("failed to write age identity: {}", path.display()))?;
    Ok(id)
}

pub(in crate::ui) fn encrypt_age_secret(
    secret: &str,
    identity: &x25519::Identity,
) -> anyhow::Result<String> {
    let recipient = identity.to_public();
    let encryptor = Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
        .map_err(|_| anyhow::anyhow!("failed to configure age recipient"))?;
    let mut out = Vec::new();
    let armor = ArmoredWriter::wrap_output(&mut out, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armor)?;
    writer.write_all(secret.as_bytes())?;
    let armor = writer.finish()?;
    let _ = armor.finish()?;
    let encoded = String::from_utf8(out).context("encrypted secret is not valid utf-8")?;
    Ok(encoded)
}

pub(in crate::ui) fn decrypt_age_secret(
    secret: &str,
    identities: &[Box<dyn age::Identity>],
) -> anyhow::Result<String> {
    let data = secret.as_bytes();
    let reader: Box<dyn std::io::Read> = if secret.contains("BEGIN AGE ENCRYPTED FILE") {
        Box::new(age::armor::ArmoredReader::new(std::io::Cursor::new(data)))
    } else {
        Box::new(std::io::Cursor::new(data))
    };
    let decryptor = Decryptor::new(reader)?;
    let mut out = String::new();
    let mut r = decryptor.decrypt(
        identities
            .iter()
            .map(|id| id.as_ref() as &dyn age::Identity),
    )?;
    r.read_to_string(&mut out)?;
    Ok(out.trim().to_string())
}
