//! Title-ID extraction and filename cleanup.
//!
//! Scene and dump filenames embed the title ID far more often than not, and an
//! exact title ID beats fuzzy-matching a mangled filename against IGDB by a
//! wide margin. So the metadata pipeline is: extract ID -> look up the official
//! name -> query IGDB with that clean name. Cleanup below is the fallback for
//! files with no ID at all.
//!
//! Every test case in this module is a real filename from the user's library.

use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;

/// Switch title IDs: 16 hex digits, e.g. `[0100000000010000]`.
static SWITCH_ID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\[([0-9a-f]{16})\]").expect("valid regex"));

/// Wii U / Wii / GameCube IDs: 6 alphanumerics, e.g. `[AGMP01]`, `[RMGE01]`.
static NINTENDO_6: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[([A-Z0-9]{6})\]").expect("valid regex"));

/// Any bracketed or parenthesised group -- region, language list, version,
/// size, dump group. Stripped wholesale during cleanup.
static BRACKET_GROUP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\[\(][^\]\)]*[\]\)]").expect("valid regex"));

static MULTI_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").expect("valid regex"));

/// Extract a Switch title ID (16 hex), normalised to uppercase.
pub fn switch_title_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    SWITCH_ID
        .captures(name)
        .map(|c| c[1].to_ascii_uppercase())
}

/// Extract a Wii U / Wii / GameCube 6-character game ID.
///
/// Requires at least one digit, which cheaply rejects same-length region words
/// like `[EUROPE]` while keeping every real ID (the two-character maker code is
/// always digit-bearing, e.g. `01`, `8P`, `52`).
pub fn nintendo_game_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    NINTENDO_6.captures_iter(name).find_map(|c| {
        let id = &c[1];
        id.chars().any(|ch| ch.is_ascii_digit()).then(|| id.to_string())
    })
}

/// Strip extension, bracketed groups and separator noise to get something
/// searchable. Used only when no title ID is present.
pub fn clean_filename(path: &Path) -> String {
    // Folder-based entries (extracted Wii U games) have no extension to strip.
    let raw = if path.is_dir() {
        path.file_name().map(|s| s.to_string_lossy().into_owned())
    } else {
        path.file_stem().map(|s| s.to_string_lossy().into_owned())
    }
    .unwrap_or_default();

    let stripped = BRACKET_GROUP.replace_all(&raw, " ");
    // Hyphens are separators here, not punctuation: PC game folders are
    // routinely named "Job-Simulator" or "Vacation-Simulator", and querying
    // IGDB with the hyphen intact finds nothing. Titles that genuinely contain
    // one ("Spider-Man") are unharmed, since matching normalises punctuation
    // away on both sides anyway.
    let spaced = stripped.replace(['_', '.', '-'], " ");
    let collapsed = MULTI_SPACE.replace_all(&spaced, " ");
    collapsed.trim().trim_end_matches(',').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn extracts_switch_ids() {
        assert_eq!(
            switch_title_id(&p("Super Mario Odyssey [0100000000010000][v0].nsp")).as_deref(),
            Some("0100000000010000")
        );
        // ID is the third bracketed group here, after region and languages.
        assert_eq!(
            switch_title_id(&p(
                "Animal Crossing New Horizons [World] [En,Ja,Fr,De,Es,It,Nl,Pt,Ru] [01006F8002326000].nsp"
            ))
            .as_deref(),
            Some("01006F8002326000")
        );
        // Lowercase in the source filename, normalised on the way out.
        assert_eq!(
            switch_title_id(&p("Undertale [010080b00ad66000][v0].nsp")).as_deref(),
            Some("010080B00AD66000")
        );
        assert_eq!(
            switch_title_id(&p("Deltarune [0100A0D022A68000][v0] (0.62 GB).nsz")).as_deref(),
            Some("0100A0D022A68000")
        );
    }

    #[test]
    fn extracts_nintendo_game_ids() {
        assert_eq!(
            nintendo_game_id(&p("Splatoon [AGMP01]")).as_deref(),
            Some("AGMP01")
        );
    }

    #[test]
    fn rejects_six_letter_region_words() {
        // Would collide with the 6-character ID shape without the digit rule.
        assert_eq!(nintendo_game_id(&p("Some Game [EUROPE].iso")), None);
        assert_eq!(nintendo_game_id(&p("Some Game [NTSC].wbfs")), None);
    }

    #[test]
    fn cleans_filenames_without_ids() {
        assert_eq!(
            clean_filename(&p("Skylanders SuperChargers (USA) (v96).wua")),
            "Skylanders SuperChargers"
        );
        assert_eq!(
            clean_filename(&p("Super Mario Galaxy.iso")),
            "Super Mario Galaxy"
        );
        // Leading bracket group -- the tricky case.
        assert_eq!(
            clean_filename(&p("[Nintendo Wii] Super Mario Galaxy 2 [NTSC].wbfs")),
            "Super Mario Galaxy 2"
        );
        assert_eq!(
            clean_filename(&p("Deltarune [0100A0D022A68000][v0] (0.62 GB).nsz")),
            "Deltarune"
        );
    }

    #[test]
    fn treats_hyphens_in_folder_names_as_separators() {
        // PC game folders on this machine. Querying IGDB with the hyphen
        // intact returned no match at all.
        assert_eq!(clean_filename(&p("Job-Simulator")), "Job Simulator");
        assert_eq!(clean_filename(&p("Vacation-Simulator")), "Vacation Simulator");
        // A title that legitimately contains a hyphen still reads correctly.
        assert_eq!(clean_filename(&p("Spider-Man")), "Spider Man");
        assert_eq!(
            clean_filename(&p("Paper Mario - The Thousand-Year Door (USA).iso")),
            "Paper Mario The Thousand Year Door"
        );
    }
}
