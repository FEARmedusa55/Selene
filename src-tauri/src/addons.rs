//! Game updates and DLC ("add-ons").
//!
//! Add-ons are a separate concern from base games and are handled differently:
//!
//! * They are **not** library entries — the Eden runner rejects non-base titles
//!   so "A Hat in Time [update]" does not appear as its own game.
//! * They only take effect once **installed to Eden's NAND**, which is a
//!   GUI-only operation (verified: eden-cli exposes only `-c -f -g -h`, no
//!   install). This module does not attempt it.
//!
//! What it does do is the automatable part: find the add-ons for a game, and
//! flag the compressed ones so the conversion pipeline can decompress them into
//! an installable `.nsp`/`.xci`.
//!
//! Add-ons are matched to their base game by **title family** — Nintendo assigns
//! a base, its update, and its DLC title IDs that share the first 12 hex digits:
//!
//! ```text
//! A Hat in Time base   010056E00853A000
//!             update   010056E00853A800   (low 3 hex = 800)
//!             DLC      010056E00853B002   (13th char A -> B, then an index)
//! ```

use crate::convert;
use crate::runners::titleid;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CONTAINER_EXTS: &[&str] = &["nsp", "nsz", "xci", "xcz"];

static VERSION: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[v(\d+)\]").expect("valid regex"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AddonKind {
    Update,
    Dlc,
}

/// The 12-hex "title family" shared by a base game and all its add-ons.
pub fn title_family(title_id: &str) -> Option<String> {
    let id = title_id.trim();
    if id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(id[..12].to_uppercase())
    } else {
        None
    }
}

/// Classify a title ID by its low 3 hex digits. `None` for a base game (`000`).
pub fn classify(title_id: &str) -> Option<AddonKind> {
    if title_id.len() != 16 {
        return None;
    }
    match &title_id[13..16] {
        "000" => None,          // base game
        "800" => Some(AddonKind::Update),
        _ => Some(AddonKind::Dlc),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Addon {
    pub title_id: String,
    pub family: String,
    pub kind: AddonKind,
    pub name: String,
    pub version: Option<u64>,
    /// Path to the runnable file if one exists, else the compressed source.
    pub path: String,
    /// True when the only copy is compressed (`.nsz`/`.xcz`) and must be
    /// decompressed before it can be installed to NAND.
    pub needs_conversion: bool,
}

fn version_of(name: &str) -> Option<u64> {
    VERSION.captures(name).and_then(|c| c[1].parse().ok())
}

/// Scan a folder for update/DLC files, one entry per title ID.
///
/// When both a compressed and a runnable copy of the same add-on exist (the
/// user has already converted it), the runnable one wins and `needs_conversion`
/// is false.
pub fn scan(dir: &Path) -> Vec<Addon> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    // Group candidate files by title ID.
    let mut by_id: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();
        if !CONTAINER_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let Some(id) = titleid::switch_title_id(&path) else {
            continue;
        };
        if classify(&id).is_none() {
            continue; // base game sitting in the add-ons folder — not an add-on
        }
        by_id.entry(id).or_default().push(path);
    }

    let mut out = Vec::new();
    for (id, paths) in by_id {
        let kind = classify(&id).expect("filtered to add-ons above");
        let family = title_family(&id).expect("valid id");

        // Prefer a runnable copy; fall back to the compressed one.
        let runnable = paths
            .iter()
            .find(|p| convert::convertible(p).is_none())
            .cloned();
        let chosen = runnable.clone().or_else(|| paths.first().cloned()).unwrap();
        let name = titleid::clean_filename(&chosen);
        let version = chosen.file_name().and_then(|n| version_of(&n.to_string_lossy()));

        out.push(Addon {
            title_id: id,
            family,
            kind,
            name,
            version,
            path: chosen.to_string_lossy().into_owned(),
            needs_conversion: runnable.is_none(),
        });
    }

    // Updates first, then DLC; then by name.
    out.sort_by(|a, b| {
        (a.kind == AddonKind::Dlc)
            .cmp(&(b.kind == AddonKind::Dlc))
            .then(a.name.cmp(&b.name))
    });
    out
}

/// Add-ons belonging to a base game.
pub fn for_game(dir: &Path, base_title_id: &str) -> Vec<Addon> {
    let Some(family) = title_family(base_title_id) else {
        return Vec::new();
    };
    scan(dir).into_iter().filter(|a| a.family == family).collect()
}

/// Auto-detect a sibling `updates` folder next to a games directory.
///
/// The user's Switch library is `.../Switch/games` with `.../Switch/updates`
/// beside it; this finds that without configuration.
pub fn detect_sibling(games_dir: &Path) -> Option<PathBuf> {
    let parent = games_dir.parent()?;
    for name in ["updates", "Updates", "dlc", "DLC"] {
        let c = parent.join(name);
        if c.is_dir() {
            return Some(c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_low_hex_digits() {
        assert_eq!(classify("010056E00853A000"), None, "base game");
        assert_eq!(classify("010056E00853A800"), Some(AddonKind::Update));
        assert_eq!(classify("010056E00853B002"), Some(AddonKind::Dlc));
        assert_eq!(classify("01002E7016C47001"), Some(AddonKind::Dlc));
        assert_eq!(classify("short"), None);
    }

    #[test]
    fn base_update_and_dlc_share_a_family() {
        let base = title_family("010056E00853A000").unwrap();
        assert_eq!(title_family("010056E00853A800").unwrap(), base, "update");
        assert_eq!(title_family("010056E00853B002").unwrap(), base, "dlc");
        // A different game must not share the family.
        assert_ne!(title_family("01002E7016C46800").unwrap(), base);
    }

    #[test]
    fn extracts_version_from_filename() {
        assert_eq!(version_of("A Hat in Time [010056E00853A800][v262144] (5.50 GB).nsz"), Some(262144));
        assert_eq!(version_of("DLC [010056E00853B002][v0].nsp"), Some(0));
        assert_eq!(version_of("no version here.nsp"), None);
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn scans_real_style_updates_folder() {
        let dir = std::env::temp_dir().join(format!("addons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Shapes taken from the user's real updates folder.
        touch(&dir, "A Hat in Time [010056E00853A800][v262144] (5.50 GB).nsz");
        touch(&dir, "A Hat in Time [DLC Nyakuza Metro] [010056E00853B002][v0].nsp");
        touch(&dir, "A Hat in Time [DLC Seal the Deal] [010056E00853B001][v0].nsp");
        // A base game mistakenly in the folder — must be ignored.
        touch(&dir, "Some Base [0100AAA000000000][v0].nsp");

        let all = scan(&dir);
        assert_eq!(all.len(), 3, "3 add-ons, base ignored: {all:?}");

        // Matched to A Hat in Time by family.
        let hat = for_game(&dir, "010056E00853A000");
        assert_eq!(hat.len(), 3);
        assert_eq!(hat.iter().filter(|a| a.kind == AddonKind::Update).count(), 1);
        assert_eq!(hat.iter().filter(|a| a.kind == AddonKind::Dlc).count(), 2);

        // The update is .nsz-only -> needs conversion; DLC are .nsp -> ready.
        let update = hat.iter().find(|a| a.kind == AddonKind::Update).unwrap();
        assert!(update.needs_conversion);
        assert!(hat.iter().filter(|a| a.kind == AddonKind::Dlc).all(|a| !a.needs_conversion));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_converted_update_no_longer_needs_conversion() {
        let dir = std::env::temp_dir().join(format!("addons-conv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        touch(&dir, "Cult of the Lamb [01002E7016C46800][v1769472].nsz");
        touch(&dir, "Cult of the Lamb [01002E7016C46800][v1769472].nsp"); // converted

        let addons = scan(&dir);
        assert_eq!(addons.len(), 1, "the two copies collapse to one add-on");
        assert!(!addons[0].needs_conversion, "runnable copy exists");
        assert!(addons[0].path.ends_with(".nsp"), "prefer the runnable file");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
