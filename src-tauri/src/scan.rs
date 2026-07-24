//! Library scanning: walk the configured roots and identify game files.

use crate::model::ScannedEntry;
use crate::runners::{self, Runner};
use std::path::Path;
use walkdir::WalkDir;

/// How deep to descend into a scan root. ROM folders are usually flat, but PC
/// games nest (`Game/Game/executable/x.exe`), so this is not 1.
const MAX_DEPTH: usize = 6;

/// Directory names never worth descending into. Skipping them avoids walking
/// tens of thousands of irrelevant files inside emulator installs.
const SKIP_DIRS: &[&str] = &[
    "sys", "languages", "qtplugins", "licenses", "cache", "shaders",
    "_commonredist", "$recycle.bin", "system volume information",
];

fn should_skip(name: &str) -> bool {
    let lower = name.to_lowercase();
    SKIP_DIRS.contains(&lower.as_str())
}

/// Stable identifier for a game, derived from its path.
///
/// FNV-1a rather than `DefaultHasher`: the latter's output is explicitly not
/// guaranteed stable across Rust releases, and these ids are persisted.
pub fn id_for_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in normalized.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Scan one root with one runner.
///
/// A game is not always a file: Cemu's extracted Wii U dumps are directories
/// (`code/ content/ meta/`). When a directory is itself a game, it is recorded
/// and *not* descended into — otherwise its hundreds of internal `.rpx` and
/// archive files would each be scanned as separate titles.
pub fn scan_root(runner: &dyn Runner, root: &Path) -> Vec<ScannedEntry> {
    if !root.is_dir() {
        log::warn!("scan root missing: {}", root.display());
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut it = WalkDir::new(root).max_depth(MAX_DEPTH).into_iter();

    loop {
        let entry = match it.next() {
            None => break,
            Some(Err(e)) => {
                log::warn!("scan error: {e}");
                continue;
            }
            Some(Ok(e)) => e,
        };

        if entry.file_type().is_dir() {
            if entry.depth() == 0 {
                continue;
            }
            if should_skip(&entry.file_name().to_string_lossy()) {
                it.skip_current_dir();
                continue;
            }
            if runner.accepts(entry.path()) {
                if let Some(found) = runner.scan_entry(entry.path()) {
                    out.push(found);
                }
                it.skip_current_dir();
            }
            continue;
        }

        if let Some(found) = runner.scan_entry(entry.path()) {
            out.push(found);
        }
    }
    out
}

/// Scan every configured root, de-duplicating by path.
///
/// Duplicates are real: a user can add both a parent folder and its child as
/// separate roots, and the same file would otherwise be inserted twice.
pub fn scan_all(roots: &[(String, String)]) -> Vec<ScannedEntry> {
    let runners = runners::all();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for (runner_id, path) in roots {
        let Some(runner) = runners.iter().find(|r| r.id().as_str() == runner_id) else {
            log::warn!("no runner registered for id '{runner_id}'");
            continue;
        };
        // PC games do not exist on Linux builds -- no Wine/Proton layer.
        if !runner.available_on_this_os() {
            log::info!("skipping '{runner_id}': not available on this platform");
            continue;
        }
        for entry in scan_root(runner.as_ref(), Path::new(path)) {
            if seen.insert(entry.path.to_lowercase()) {
                out.push(entry);
            }
        }
    }
    drop_superseded_conversions(&mut out);
    out
}

/// Drop a convertible entry (`.nsz`/`.xcz`) when a directly-runnable file with
/// the same title ID has already been produced from it.
///
/// After the user converts `Game.nsz` to `Game.nsp`, both sit in the folder.
/// The `.nsp` is the one that runs; keeping the `.nsz` too would show a second,
/// unplayable copy of the same title. Matched on title ID, since the filenames
/// differ (the `.nsz` carries a size suffix the `.nsp` does not).
fn drop_superseded_conversions(entries: &mut Vec<ScannedEntry>) {
    use std::collections::HashSet;
    let runnable_ids: HashSet<String> = entries
        .iter()
        .filter(|e| crate::convert::convertible(Path::new(&e.path)).is_none())
        .filter_map(|e| e.title_id.clone())
        .collect();

    entries.retain(|e| {
        let is_convertible = crate::convert::convertible(Path::new(&e.path)).is_some();
        match (is_convertible, &e.title_id) {
            (true, Some(id)) => !runnable_ids.contains(id),
            _ => true,
        }
    });
}

/// Fill in Wii U title IDs and real names from Cemu's title cache.
///
/// `.wua` archives carry no title ID in their filename, and the format is
/// Cemu-specific and compressed, so its own cache is the pragmatic source.
/// Entries Cemu has never seen are left untouched rather than guessed at.
pub fn enrich_wiiu(entries: &mut [ScannedEntry], cemu_data_dir: &Path) {
    let index = match crate::runners::wiiu::TitleIndex::load(cemu_data_dir) {
        Ok(i) => i,
        Err(e) => {
            log::warn!("could not load Cemu title cache: {e}");
            return;
        }
    };
    if index.is_empty() {
        return;
    }

    for entry in entries.iter_mut() {
        if entry.runner != crate::model::RunnerId::Cemu {
            continue;
        }
        let Some(found) = index.get(Path::new(&entry.path)) else {
            continue;
        };
        if entry.title_id.is_none() {
            entry.title_id = Some(found.title_id.clone());
        }
        // Cemu's name beats a filename with region and version markers in it,
        // and is what gets handed to IGDB.
        if !found.name.trim().is_empty() {
            entry.cleaned_name = found.name.trim().to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ids_are_stable_and_case_insensitive() {
        let a = id_for_path(Path::new(r"D:\Games\Wii Roms\Galaxy.iso"));
        let b = id_for_path(Path::new(r"d:\games\wii roms\galaxy.iso"));
        assert_eq!(a, b, "Windows paths are case-insensitive");
        assert_eq!(a.len(), 16);

        // Different files must not collide.
        assert_ne!(a, id_for_path(Path::new(r"D:\Games\Wii Roms\Galaxy2.iso")));
    }

    #[test]
    fn ids_survive_separator_style() {
        assert_eq!(
            id_for_path(Path::new("D:/Games/x.iso")),
            id_for_path(Path::new(r"D:\Games\x.iso"))
        );
    }

    #[test]
    fn skips_emulator_internal_directories() {
        assert!(should_skip("Sys"));
        assert!(should_skip("_CommonRedist"));
        assert!(should_skip("QtPlugins"));
        assert!(!should_skip("Wii Roms"));
        assert!(!should_skip("Gamecube Roms"));
    }

    fn nsz_entry(path: &str, title_id: &str) -> ScannedEntry {
        ScannedEntry {
            path: path.into(),
            cleaned_name: "Game".into(),
            title_id: Some(title_id.into()),
            platform: crate::model::Platform::Switch,
            runner: crate::model::RunnerId::Eden,
            size_bytes: 1,
        }
    }

    #[test]
    fn a_converted_nsp_supersedes_its_nsz() {
        let mut entries = vec![
            nsz_entry(r"D:\games\Terraria [0100E46006708000] (0.16 GB).nsz", "0100E46006708000"),
            nsz_entry(r"D:\games\Terraria [0100E46006708000].nsp", "0100E46006708000"),
        ];
        drop_superseded_conversions(&mut entries);
        assert_eq!(entries.len(), 1, "the .nsz should be dropped");
        assert!(entries[0].path.ends_with(".nsp"));
    }

    #[test]
    fn an_unconverted_nsz_is_kept() {
        // No sibling .nsp -> the .nsz stays, so the user can convert it.
        let mut entries = vec![nsz_entry(r"D:\games\Skyrim [01000A10041EA000].nsz", "01000A10041EA000")];
        drop_superseded_conversions(&mut entries);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with(".nsz"));
    }

    #[test]
    fn a_different_titles_nsp_does_not_supersede() {
        let mut entries = vec![
            nsz_entry(r"D:\games\A [0100AAA].nsz", "0100AAA"),
            nsz_entry(r"D:\games\B [0100BBB].nsp", "0100BBB"),
        ];
        drop_superseded_conversions(&mut entries);
        assert_eq!(entries.len(), 2, "unrelated titles must both survive");
    }

    #[test]
    fn missing_root_yields_nothing_rather_than_failing() {
        let runners = runners::all();
        let d = runners.first().expect("at least one runner");
        let entries = scan_root(d.as_ref(), &PathBuf::from(r"Z:\does\not\exist"));
        assert!(entries.is_empty());
    }
}
