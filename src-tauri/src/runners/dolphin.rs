//! Dolphin runner -- GameCube and Wii. The Phase 1 target.
//!
//! Dolphin is the cleanest of the three emulators to integrate: it has both a
//! native per-game config file (`GameSettings/<GameID>.ini`) *and* per-launch
//! `-C Section.Key=Value` overrides, so per-game settings never require
//! touching the user's global configuration.

use super::{LaunchPlan, PerGameConfigStyle, Runner};
use crate::model::{Platform, RunnerId};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Dolphin;

const PLATFORMS: &[Platform] = &[Platform::Wii, Platform::GameCube];

/// `.nkit` files are really `.nkit.iso` / `.nkit.gcz`; the extension check sees
/// the final component, so the container extensions cover them.
const EXTENSIONS: &[&str] = &["iso", "rvz", "wbfs", "gcm", "ciso", "gcz"];

impl Runner for Dolphin {
    fn id(&self) -> RunnerId {
        RunnerId::Dolphin
    }

    fn display_name(&self) -> &'static str {
        "Dolphin"
    }

    fn platforms(&self) -> &'static [Platform] {
        PLATFORMS
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn detect_executable(&self) -> Option<PathBuf> {
        for candidate in candidate_paths() {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    fn extract_title_id(&self, path: &Path) -> Option<String> {
        // Header first: it is authoritative. Super Mario Galaxy's disc reports
        // RMG**P**01 (PAL) while its filename says nothing about region, so a
        // filename guess would have produced the wrong ID.
        super::disc::probe(path)
            .map(|d| d.game_id)
            .or_else(|| super::titleid::nintendo_game_id(path))
    }

    fn platform_for(&self, path: &Path) -> Platform {
        super::disc::identify(path).1
    }

    fn per_game_config_style(&self) -> PerGameConfigStyle {
        PerGameConfigStyle::NativeFile
    }

    fn build_launch_plan(&self, exe: &Path, game_path: &Path) -> Result<LaunchPlan> {
        let exe = exe
            .canonicalize()
            .with_context(|| format!("Dolphin executable not found at {}", exe.display()))?;

        Ok(LaunchPlan {
            program: exe,
            args: vec![
                // `-b` exits Dolphin when the game closes, so the process tree
                // terminates cleanly and playtime tracking gets a real end time.
                "-b".into(),
                "-e".into(),
                game_path.to_string_lossy().into_owned(),
            ],
            working_dir: game_path.parent().map(Path::to_path_buf),
            // NativeFile style: nothing to restore.
            restore_after_exit: Vec::new(),
        })
    }
}

/// Common Dolphin install locations.
///
/// Deliberately contains no machine-specific paths: an earlier version
/// hardcoded this developer's own folder, which broke the moment that folder
/// was renamed. Detection is a convenience only -- the authoritative path is
/// whatever the user set, stored in the `runners` table.
fn candidate_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut v = vec![
            PathBuf::from(r"C:\Program Files\Dolphin\Dolphin.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Dolphin\Dolphin.exe"),
        ];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join(r"Dolphin\Dolphin.exe"));
        }
        if let Ok(pf) = std::env::var("ProgramW6432") {
            v.push(PathBuf::from(pf).join(r"Dolphin\Dolphin.exe"));
        }
        v
    }
    #[cfg(not(windows))]
    {
        vec![
            PathBuf::from("/usr/bin/dolphin-emu"),
            PathBuf::from("/usr/local/bin/dolphin-emu"),
            PathBuf::from("/var/lib/flatpak/exports/bin/org.DolphinEmu.dolphin-emu"),
        ]
    }
}

/// Search a user-nominated folder for a Dolphin build, a few levels deep so
/// portable layouts like `<root>/dolphin-2606-x64/Dolphin-x64/Dolphin.exe` are
/// found without the user drilling all the way down.
pub fn find_in_folder(root: &Path, max_depth: usize) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "Dolphin.exe" } else { "dolphin-emu" };
    walkdir::WalkDir::new(root)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| e.file_type().is_file() && e.file_name().to_string_lossy() == exe)
        .map(|e| e.into_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_disc_images_and_rejects_others() {
        let d = Dolphin;
        assert!(d.accepts(Path::new("Super Mario Galaxy.iso")));
        assert!(d.accepts(Path::new("Super Mario Galaxy 2.wbfs")));
        assert!(d.accepts(Path::new("Metroid Prime.RVZ")), "case-insensitive");
        // Belongs to other runners.
        assert!(!d.accepts(Path::new("Splatoon.wua")));
        assert!(!d.accepts(Path::new("Mario Odyssey.nsp")));
    }

    #[test]
    fn uses_native_per_game_config() {
        // Guards the core promise: Dolphin never needs global-config swapping.
        assert_eq!(
            Dolphin.per_game_config_style(),
            PerGameConfigStyle::NativeFile
        );
    }
}
