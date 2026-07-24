//! Choosing which executable in a game folder is *the game*.
//!
//! Size is worse than useless here. In the user's library the largest `.exe` in
//! both VR titles is `vcredist_2015-2019_x64.exe` at 14.3 MB, while the actual
//! game is 0.6 MB — a "biggest file" heuristic is wrong 100% of the time on
//! this data.
//!
//! What does work, in order of strength:
//!   1. **Hard exclusions.** Redistributables, crash handlers and uninstallers
//!      are never the game, and they are named predictably.
//!   2. **Engine markers.** Unity ships `<Name>_Data` beside `<Name>.exe`; that
//!      pairing is close to proof.
//!   3. **Name similarity** to the game folder — `Job-Simulator` ->
//!      `JobSimulator.exe`, `Vacation-Simulator` -> `Vacation Simulator.exe`.
//!   4. **Proximity to `steam_api*.dll`**, which sits next to the executable
//!      that links it.
//!
//! Anything ambiguous is still recorded, and the user can override the choice.

use std::path::{Path, PathBuf};

/// Directory names whose contents are never the game.
const EXCLUDED_DIRS: &[&str] = &[
    "_commonredist",
    "commonredist",
    "redist",
    "_redist",
    "directx",
    "vcredist",
    "dotnet",
    "_cef",
    "installers",
];

/// Filename fragments that mark a helper binary rather than a game.
const EXCLUDED_NAMES: &[&str] = &[
    "vcredist",
    "vc_redist",
    "dxwebsetup",
    "dxsetup",
    "directx",
    "oalinst",
    "openal",
    "crashhandler",
    "crashreport",
    "crashpad",
    "unitycrashhandler",
    "uninstall",
    "unins000",
    "setup",
    "installer",
    "dotnetfx",
    "vcredist_x86",
    "vcredist_x64",
    "notification_helper",
    "quicksfv",
];

/// True when a path is definitely not a game executable.
pub fn is_excluded(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if EXCLUDED_NAMES.iter().any(|n| stem.contains(n)) {
        return true;
    }
    // Any ancestor directory being a redist folder disqualifies the file.
    path.parent()
        .map(|p| {
            p.components().any(|c| {
                let s = c.as_os_str().to_string_lossy().to_lowercase();
                EXCLUDED_DIRS.contains(&s.as_str())
            })
        })
        .unwrap_or(false)
}

/// Normalise an identifier for comparison: split camelCase, drop punctuation,
/// lowercase. `JobSimulator` and `Job-Simulator` both become `job simulator`.
pub fn normalize_ident(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            // Insert a break at a lower->upper transition (camelCase) and
            // between a letter and a digit.
            if i > 0 && !out.ends_with(' ') {
                let prev = chars[i - 1];
                let camel = prev.is_lowercase() && c.is_uppercase();
                let digit_edge = prev.is_alphabetic() && c.is_numeric();
                if camel || digit_edge {
                    out.push(' ');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// Similarity of an executable name to the game folder name, 0.0..=1.0.
///
/// Titles are routinely truncated in executable names ("Adventure Time Explore
/// the Dungeon..." -> `AdventureTime.exe`), so this rewards the *executable's*
/// words all appearing in the folder name, not the reverse.
pub fn name_similarity(folder: &str, exe_stem: &str) -> f64 {
    let f = normalize_ident(folder);
    let e = normalize_ident(exe_stem);
    if f.is_empty() || e.is_empty() {
        return 0.0;
    }
    if f == e {
        return 1.0;
    }
    let f_words: Vec<&str> = f.split_whitespace().collect();
    let e_words: Vec<&str> = e.split_whitespace().collect();
    let matched = e_words.iter().filter(|w| f_words.contains(w)).count();
    let coverage = matched as f64 / e_words.len() as f64;

    // A leading-word match is a strong hint the exe is named after the game.
    let head = if f_words.first() == e_words.first() { 0.15 } else { 0.0 };
    (coverage * 0.85 + head).min(1.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub path: PathBuf,
    pub score: f64,
    /// Why it won, for showing in the UI.
    pub reasons: Vec<String>,
}

/// Score one executable within `game_dir`.
fn score_exe(game_dir: &Path, exe: &Path) -> Candidate {
    let mut score = 0.0;
    let mut reasons = Vec::new();

    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let parent = exe.parent().unwrap_or(game_dir);
    let folder_name = game_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // Unity: `<Name>_Data` beside `<Name>.exe`. Near-proof.
    if parent.join(format!("{stem}_Data")).is_dir() {
        score += 5.0;
        reasons.push("Unity data folder".into());
    }

    // Unreal: `<Name>\Binaries\Win64\<Name>.exe` layout.
    if parent
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("Binaries"))
    {
        score += 2.0;
        reasons.push("Unreal binaries path".into());
    }

    let sim = name_similarity(folder_name, stem);
    if sim > 0.0 {
        score += sim * 4.0;
        reasons.push(format!("name match {:.0}%", sim * 100.0));
    }

    // The Steam API DLL sits beside the executable that links it.
    let has_steam_dll = std::fs::read_dir(parent)
        .map(|d| {
            d.filter_map(Result::ok).any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.starts_with("steam_api") && n.ends_with(".dll")
            })
        })
        .unwrap_or(false);
    if has_steam_dll {
        score += 1.5;
        reasons.push("beside steam_api dll".into());
    }

    // Shallower is mildly better; deep nesting usually means tooling.
    let depth = exe.strip_prefix(game_dir).map(|p| p.components().count()).unwrap_or(9);
    score += (4_i32.saturating_sub(depth as i32)).max(0) as f64 * 0.25;

    // Size is deliberately worth almost nothing -- see the module docs. It only
    // breaks ties between otherwise identical candidates.
    let mb = exe.metadata().map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
    score += (mb.min(50.0)) * 0.002;

    Candidate {
        path: exe.to_path_buf(),
        score,
        reasons,
    }
}

/// All plausible executables in a game folder, best first.
pub fn rank_executables(game_dir: &Path) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = walkdir::WalkDir::new(game_dir)
        .max_depth(5)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
        })
        .filter(|e| !is_excluded(e.path()))
        .map(|e| score_exe(game_dir, e.path()))
        .collect();

    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The best guess at a game's executable.
pub fn pick_executable(game_dir: &Path) -> Option<PathBuf> {
    rank_executables(game_dir).into_iter().next().map(|c| c.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path, bytes: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; bytes]).unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("exepick-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn excludes_redistributables_and_helpers() {
        for bad in [
            r"C:\g\_CommonRedist\vcredist_2015-2019_x64.exe",
            r"C:\g\_CommonRedist\dxwebsetup.exe",
            r"C:\g\_CommonRedist\oalinst.exe",
            r"C:\g\Game\UnityCrashHandler64.exe",
            r"C:\g\unins000.exe",
            r"C:\g\DirectX\DXSETUP.exe",
        ] {
            assert!(is_excluded(Path::new(bad)), "should exclude {bad}");
        }
        for good in [r"C:\g\Game\JobSimulator.exe", r"C:\g\executable\AdventureTime.exe"] {
            assert!(!is_excluded(Path::new(good)), "should keep {good}");
        }
    }

    #[test]
    fn normalises_camel_case_and_punctuation() {
        assert_eq!(normalize_ident("JobSimulator"), "job simulator");
        assert_eq!(normalize_ident("Job-Simulator"), "job simulator");
        assert_eq!(normalize_ident("Vacation Simulator"), "vacation simulator");
        assert_eq!(normalize_ident("AdventureTime"), "adventure time");
        assert_eq!(normalize_ident("Portal2"), "portal 2");
    }

    #[test]
    fn matches_names_across_separator_styles() {
        assert!(name_similarity("Job-Simulator", "JobSimulator") > 0.9);
        assert!(name_similarity("Vacation-Simulator", "Vacation Simulator") > 0.9);
        // The exe is a truncation of a long title -- still a strong match.
        assert!(
            name_similarity("Adventure Time Explore the Dungeon Because I DON'T KNOW", "AdventureTime")
                > 0.8
        );
        // Unrelated names must not match.
        assert!(name_similarity("Job-Simulator", "UnityCrashHandler64") < 0.3);
    }

    /// The exact layout of Job Simulator on this machine.
    #[test]
    fn picks_the_game_not_the_biggest_binary() {
        let dir = tmp("unity");
        let g = dir.join("Job-Simulator");
        // Redistributables are far larger than the game.
        touch(&g.join("_CommonRedist/vcredist_2015-2019_x64.exe"), 14_300_000);
        touch(&g.join("_CommonRedist/vcredist_x86.exe"), 4_800_000);
        touch(&g.join("_CommonRedist/dxwebsetup.exe"), 300_000);
        touch(&g.join("_CommonRedist/oalinst.exe"), 800_000);
        touch(&g.join("Job Simulator/UnityCrashHandler64.exe"), 1_400_000);
        touch(&g.join("Job Simulator/JobSimulator.exe"), 600_000);
        touch(&g.join("Job Simulator/steam_api64.dll"), 1_700_000);
        std::fs::create_dir_all(g.join("Job Simulator/JobSimulator_Data")).unwrap();

        let picked = pick_executable(&g).expect("should pick something");
        assert_eq!(
            picked.file_name().unwrap(),
            "JobSimulator.exe",
            "picked {picked:?} -- size-based ranking would choose vcredist"
        );

        // Every decoy must be filtered out entirely, not merely outranked.
        let ranked = rank_executables(&g);
        assert_eq!(ranked.len(), 1, "decoys survived: {ranked:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handles_a_space_in_the_executable_name() {
        let dir = tmp("space");
        let g = dir.join("Vacation-Simulator");
        touch(&g.join("_CommonRedist/vcredist_x64.exe"), 14_000_000);
        touch(&g.join("Vacation Simulator/UnityCrashHandler64.exe"), 1_400_000);
        touch(&g.join("Vacation Simulator/Vacation Simulator.exe"), 600_000);
        std::fs::create_dir_all(g.join("Vacation Simulator/Vacation Simulator_Data/Plugins")).unwrap();
        touch(
            &g.join("Vacation Simulator/Vacation Simulator_Data/Plugins/steam_api64.dll"),
            200_000,
        );

        let picked = pick_executable(&g).expect("should pick something");
        assert_eq!(picked.file_name().unwrap(), "Vacation Simulator.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nested wrapper folders: `Game\Game\executable\Game.exe`.
    #[test]
    fn finds_an_executable_through_wrapper_folders() {
        let dir = tmp("nested");
        let long = "Adventure Time Explore the Dungeon";
        let g = dir.join(long);
        touch(&g.join(long).join("executable/AdventureTime.exe"), 8_900_000);
        touch(&g.join(long).join("executable/steam_api.dll"), 200_000);

        let picked = pick_executable(&g).expect("should find the nested exe");
        assert_eq!(picked.file_name().unwrap(), "AdventureTime.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_with_no_executable_yields_nothing() {
        let dir = tmp("empty");
        let g = dir.join("Data Only");
        std::fs::create_dir_all(g.join("assets")).unwrap();
        assert!(pick_executable(&g).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_of_only_redists_yields_nothing() {
        let dir = tmp("redistonly");
        let g = dir.join("Broken");
        touch(&g.join("_CommonRedist/vcredist_x64.exe"), 14_000_000);
        touch(&g.join("unins000.exe"), 900_000);
        assert!(
            pick_executable(&g).is_none(),
            "must not fall back to a redistributable"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
