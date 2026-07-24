//! Metadata and artwork resolution.

pub mod igdb;
pub mod match_score;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Credentials live in the config directory, never in the repository or in
/// source. This keeps them out of version control by construction rather than
/// by remembering to add a .gitignore entry.
pub fn credentials_path(config_dir: &Path) -> PathBuf {
    config_dir.join("credentials.json")
}

pub fn load_credentials(config_dir: &Path) -> Result<Option<igdb::Credentials>> {
    let path = credentials_path(config_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    // Windows editors (Notepad, PowerShell's `Out-File -Encoding utf8`) prepend
    // a UTF-8 BOM, which serde_json rejects with a baffling error. Users will
    // hand-edit this file, so tolerate it.
    let raw = raw.trim_start_matches('\u{feff}');
    let creds: igdb::Credentials =
        serde_json::from_str(raw).context("parsing credentials.json")?;
    Ok(Some(creds))
}

pub fn save_credentials(config_dir: &Path, creds: &igdb::Credentials) -> Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = credentials_path(config_dir);
    let json = serde_json::to_string_pretty(creds)?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
