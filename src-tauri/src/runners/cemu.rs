//! Cemu runner — Wii U.
//!
//! Two shapes of game, unlike the other runners:
//!   * single-file archives (`.wua`, `.wud`, `.wux`)
//!   * extracted folders containing `code/ content/ meta/`, launched via the
//!     `.rpx` executable inside `code/`
//!
//! The folder case is why `Runner::accepts` is overridden here and why the
//! scanner cannot assume one game means one file.

use super::{wiiu, LaunchPlan, PerGameConfigStyle, Runner};
use crate::model::{Platform, RunnerId};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Cemu;

const PLATFORMS: &[Platform] = &[Platform::WiiU];
const EXTENSIONS: &[&str] = &["wua", "wud", "wux", "rpx", "iso"];

/// Marks an extracted Wii U game directory. All three are always present in a
/// loadiine-style dump; requiring all three avoids treating a stray `content`
/// folder somewhere else in the tree as a game.
fn is_extracted_game_dir(path: &Path) -> bool {
    path.is_dir()
        && path.join("code").is_dir()
        && path.join("content").is_dir()
        && path.join("meta").is_dir()
}

/// The `.rpx` Cemu should be pointed at inside an extracted game.
fn rpx_in(game_dir: &Path) -> Option<PathBuf> {
    let code = game_dir.join("code");
    let entries = std::fs::read_dir(code).ok()?;
    let mut best: Option<PathBuf> = None;
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("rpx"))
            != Some(true)
        {
            continue;
        }
        // Prefer the largest .rpx: dumps sometimes ship small helper modules
        // alongside the real executable.
        let size = p.metadata().map(|m| m.len()).unwrap_or(0);
        let best_size = best
            .as_ref()
            .and_then(|b| b.metadata().ok())
            .map(|m| m.len())
            .unwrap_or(0);
        if best.is_none() || size > best_size {
            best = Some(p);
        }
    }
    best
}

impl Runner for Cemu {
    fn id(&self) -> RunnerId {
        RunnerId::Cemu
    }

    fn display_name(&self) -> &'static str {
        "Cemu"
    }

    fn platforms(&self) -> &'static [Platform] {
        PLATFORMS
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn detect_executable(&self) -> Option<PathBuf> {
        candidate_paths().into_iter().find(|p| p.is_file())
    }

    fn extract_title_id(&self, path: &Path) -> Option<String> {
        // Extracted folder: meta.xml is authoritative and needs no Cemu state.
        if is_extracted_game_dir(path) {
            return wiiu::title_id_from_meta(path);
        }
        // Archives are resolved from Cemu's cache by the scanner, which holds
        // the index; nothing useful can be read from the path alone.
        None
    }

    /// Accepts archive files *and* extracted game directories.
    fn accepts(&self, path: &Path) -> bool {
        if is_extracted_game_dir(path) {
            return true;
        }
        if path.is_dir() {
            return false;
        }
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn per_game_config_style(&self) -> PerGameConfigStyle {
        // gameProfiles/<TitleID>.ini. Resolution is separate — it runs through
        // graphic packs recorded in settings.xml. See config::cemu.
        PerGameConfigStyle::NativeFile
    }

    fn build_launch_plan(&self, exe: &Path, game_path: &Path) -> Result<LaunchPlan> {
        let exe = exe
            .canonicalize()
            .with_context(|| format!("Cemu executable not found at {}", exe.display()))?;

        // Cemu is given the .rpx for extracted games, the archive otherwise.
        let target = if is_extracted_game_dir(game_path) {
            rpx_in(game_path).ok_or_else(|| {
                anyhow::anyhow!("no .rpx found in {}/code", game_path.display())
            })?
        } else {
            game_path.to_path_buf()
        };

        Ok(LaunchPlan {
            program: exe,
            args: vec![
                "-g".into(),
                target.to_string_lossy().into_owned(),
            ],
            working_dir: game_path.parent().map(Path::to_path_buf),
            // NativeFile style: nothing swapped, nothing to restore.
            restore_after_exit: Vec::new(),
        })
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut v = vec![PathBuf::from(r"C:\Program Files\Cemu\Cemu.exe")];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join(r"Cemu\Cemu.exe"));
        }
        v
    }
    #[cfg(not(windows))]
    {
        vec![
            PathBuf::from("/usr/bin/cemu"),
            PathBuf::from("/var/lib/flatpak/exports/bin/info.cemu.Cemu"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_extracted(root: &Path, rpx: &[(&str, usize)]) {
        for d in ["code", "content", "meta"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for (name, size) in rpx {
            std::fs::write(root.join("code").join(name), vec![0u8; *size]).unwrap();
        }
    }

    #[test]
    fn accepts_wii_u_archive_formats() {
        let c = Cemu;
        assert!(c.accepts(Path::new("Skylanders Trap Team (USA) (v16).wua")));
        assert!(c.accepts(Path::new("game.WUD")), "case-insensitive");
        assert!(c.accepts(Path::new("game.wux")));
        // Other runners' formats.
        assert!(!c.accepts(Path::new("Galaxy.iso.nsp")));
        assert!(!c.accepts(Path::new("Mario.nsz")));
    }

    #[test]
    fn accepts_extracted_folders_and_rejects_plain_ones() {
        let base = std::env::temp_dir().join(format!("cemu-acc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        let game = base.join("Splatoon [AGMP01]");
        make_extracted(&game, &[("Gambit.rpx", 64)]);
        assert!(Cemu.accepts(&game), "extracted game dir should be accepted");

        // A folder that merely exists is not a game.
        let plain = base.join("Just A Folder");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(!Cemu.accepts(&plain));

        // Missing one of the three required subfolders.
        let partial = base.join("Partial");
        std::fs::create_dir_all(partial.join("code")).unwrap();
        std::fs::create_dir_all(partial.join("content")).unwrap();
        assert!(!Cemu.accepts(&partial), "meta/ missing, must not match");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn launches_the_largest_rpx_from_an_extracted_game() {
        let base = std::env::temp_dir().join(format!("cemu-rpx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let game = base.join("Splatoon [AGMP01]");
        // A small helper module alongside the real executable.
        make_extracted(&game, &[("helper.rpx", 16), ("Gambit.rpx", 4096)]);

        let picked = rpx_in(&game).expect("should find an rpx");
        assert_eq!(picked.file_name().unwrap(), "Gambit.rpx");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn archive_launch_passes_the_archive_itself() {
        let base = std::env::temp_dir().join(format!("cemu-arc-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let exe = base.join("Cemu.exe");
        std::fs::write(&exe, b"stub").unwrap();
        let game = base.join("Skylanders Trap Team (USA) (v16).wua");
        std::fs::write(&game, b"stub").unwrap();

        let plan = Cemu.build_launch_plan(&exe, &game).unwrap();
        assert_eq!(plan.args[0], "-g");
        assert!(plan.args[1].ends_with("Skylanders Trap Team (USA) (v16).wua"));
        assert!(plan.restore_after_exit.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn uses_native_per_game_config() {
        assert_eq!(Cemu.per_game_config_style(), PerGameConfigStyle::NativeFile);
    }
}
