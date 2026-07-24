//! Wii U title-ID resolution.
//!
//! Cemu keys its per-game profiles and graphic packs by the 16-hex title ID
//! (`0005000010176A00`), which — unlike Switch dumps — is *not* present in the
//! filenames on this machine (`Skylanders Trap Team (USA) (v16).wua`). Two
//! sources, in order of preference:
//!
//! 1. `meta/meta.xml` inside an extracted game folder. Authoritative, always
//!    present for loadiine-style dumps, and readable without any Cemu state.
//! 2. Cemu's own `title_list_cache.xml`, which maps file paths to title IDs.
//!    This is how `.wua` archives are resolved: the format is Cemu-specific and
//!    compressed, so re-implementing a parser for it is not worth the risk of
//!    getting it subtly wrong.
//!
//! The cache only exists once Cemu has seen the folder, so (2) can legitimately
//! come up empty — games are then left without a title ID rather than guessed
//! at, and the UI surfaces that.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A base title ID: 16 hex characters starting `00050000`. The `0005000E`
/// variants in the cache are *updates*, and Cemu profiles never key on those.
static BASE_TITLE_ID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^00050000[0-9a-f]{8}$").expect("valid regex"));

pub fn is_base_title_id(s: &str) -> bool {
    BASE_TITLE_ID.is_match(s)
}

/// Read `meta/meta.xml` from an extracted game folder.
pub fn title_id_from_meta(game_dir: &Path) -> Option<String> {
    let meta = game_dir.join("meta").join("meta.xml");
    let text = std::fs::read_to_string(&meta).ok()?;
    // Deliberately not a full XML parse: the file is tiny and fixed-shape, and
    // some dumps carry invalid XML declarations that stricter parsers reject.
    let start = text.find("<title_id")?;
    let open = text[start..].find('>')? + start + 1;
    let close = text[open..].find("</title_id>")? + open;
    let id = text[open..close].trim().to_uppercase();
    is_base_title_id(&id).then_some(id)
}

/// A single entry from Cemu's title list cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTitle {
    pub title_id: String,
    pub name: String,
    pub path: String,
}

/// Parse Cemu's `title_list_cache.xml`, keeping only base titles.
///
/// Kept as string scanning rather than an XML parse for the same reason as
/// above, and because the file interleaves base and update entries that share
/// every element name.
pub fn parse_title_cache(xml: &str) -> Vec<CachedTitle> {
    let mut out = Vec::new();
    for chunk in xml.split("<title ").skip(1) {
        let Some(end) = chunk.find("</title>") else {
            continue;
        };
        let block = &chunk[..end];

        let attr = |name: &str| -> Option<String> {
            let pat = format!("{name}=\"");
            let s = block.find(&pat)? + pat.len();
            let e = block[s..].find('"')? + s;
            Some(block[s..e].to_string())
        };
        let elem = |name: &str| -> Option<String> {
            let open = format!("<{name}>");
            let close = format!("</{name}>");
            let s = block.find(&open)? + open.len();
            let e = block[s..].find(&close)? + s;
            Some(block[s..e].trim().to_string())
        };

        let Some(title_id) = attr("titleId") else { continue };
        let title_id = title_id.to_uppercase();
        // Skip update/DLC entries; only base titles carry a game profile.
        if !is_base_title_id(&title_id) {
            continue;
        }
        let Some(path) = elem("path") else { continue };
        out.push(CachedTitle {
            title_id,
            name: elem("name").unwrap_or_default(),
            path,
        });
    }
    out
}

/// Cemu's data directory. Portable installs keep `settings.xml` beside the
/// executable; otherwise it is `%APPDATA%\Cemu` (or the XDG equivalent).
pub fn cemu_data_dir(cemu_exe: &Path) -> Option<PathBuf> {
    if let Some(dir) = cemu_exe.parent() {
        if dir.join("portable.txt").exists() || dir.join("settings.xml").exists() {
            return Some(dir.to_path_buf());
        }
    }
    #[cfg(windows)]
    {
        std::env::var("APPDATA").ok().map(|a| PathBuf::from(a).join("Cemu"))
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share/Cemu"))
    }
}

/// Path -> title ID lookup built from Cemu's cache.
///
/// Paths are normalised to lowercase with forward slashes: Cemu writes
/// `D:/Games/...` while our scanner produces `d:\Games\...`.
pub struct TitleIndex {
    by_path: HashMap<String, CachedTitle>,
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

impl TitleIndex {
    pub fn empty() -> Self {
        Self { by_path: HashMap::new() }
    }

    pub fn load(cemu_data_dir: &Path) -> Result<Self> {
        let path = cemu_data_dir.join("title_list_cache.xml");
        let Ok(xml) = std::fs::read_to_string(&path) else {
            log::info!("no Cemu title cache at {}", path.display());
            return Ok(Self::empty());
        };
        Ok(Self::from_xml(&xml))
    }

    pub fn from_xml(xml: &str) -> Self {
        let by_path = parse_title_cache(xml)
            .into_iter()
            .map(|t| (normalize_path(&t.path), t))
            .collect();
        Self { by_path }
    }

    pub fn get(&self, path: &Path) -> Option<&CachedTitle> {
        self.by_path.get(&normalize_path(&path.to_string_lossy()))
    }

    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from this machine's real cache, including an update entry that
    /// must be ignored.
    const CACHE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<title_list>
 <title titleId="00050000101f4d00" version="0" app_type="80000000">
  <name>Skylanders Imaginators</name>
  <path>D:/Games/Wii U/Roms/Skylanders Imaginators (USA) (v16).wua</path>
 </title>
 <title titleId="0005000e101f4d00" version="16" app_type="0800001b">
  <name>Skylanders Imaginators</name>
  <path>D:/Games/Wii U/Roms/Skylanders Imaginators (USA) (v16).wua</path>
 </title>
 <title titleId="0005000010176a00" version="16" app_type="80000000">
  <name>Splatoon</name>
  <path>D:/Games/Wii U/Roms/Splatoon [AGMP01]</path>
 </title>
</title_list>"#;

    #[test]
    fn recognises_base_title_ids_only() {
        assert!(is_base_title_id("0005000010176A00"));
        assert!(is_base_title_id("00050000101f4d00"));
        // Update title: same game, but never carries a game profile.
        assert!(!is_base_title_id("0005000E101F4D00"));
        assert!(!is_base_title_id("AGMP01"));
        assert!(!is_base_title_id(""));
    }

    #[test]
    fn parses_only_base_titles_from_the_cache() {
        let titles = parse_title_cache(CACHE);
        assert_eq!(titles.len(), 2, "update entry should be skipped: {titles:?}");
        assert_eq!(titles[0].title_id, "00050000101F4D00");
        assert_eq!(titles[1].name, "Splatoon");
    }

    #[test]
    fn looks_up_by_path_across_separator_and_case_differences() {
        let idx = TitleIndex::from_xml(CACHE);
        // Scanner-style path: backslashes, lowercase drive letter.
        let found = idx
            .get(Path::new(r"d:\Games\Wii U\Roms\Skylanders Imaginators (USA) (v16).wua"))
            .expect("should match despite separator/case differences");
        assert_eq!(found.title_id, "00050000101F4D00");

        // A folder-based entry with brackets in the name.
        let splat = idx
            .get(Path::new(r"D:\Games\Wii U\Roms\Splatoon [AGMP01]"))
            .expect("bracketed folder should match");
        assert_eq!(splat.title_id, "0005000010176A00");
    }

    #[test]
    fn unknown_paths_return_none_rather_than_guessing() {
        let idx = TitleIndex::from_xml(CACHE);
        assert!(idx.get(Path::new(r"d:\Games\Wii U\Roms\Unknown Game.wua")).is_none());
    }

    #[test]
    fn missing_cache_yields_an_empty_index() {
        let idx = TitleIndex::load(Path::new(r"Z:\nope")).unwrap();
        assert!(idx.is_empty());
    }

    #[test]
    fn extracts_title_id_from_meta_xml() {
        let dir = std::env::temp_dir().join(format!("wiiu-meta-{}", std::process::id()));
        let meta = dir.join("meta");
        std::fs::create_dir_all(&meta).unwrap();
        std::fs::write(
            meta.join("meta.xml"),
            "<menu><title_id type=\"hexBinary\" length=\"8\">0005000010176A00</title_id></menu>",
        )
        .unwrap();

        assert_eq!(title_id_from_meta(&dir).as_deref(), Some("0005000010176A00"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
