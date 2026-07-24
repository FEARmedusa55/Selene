//! Direct-launch runner for installed PC games. **Windows only.**
//!
//! Linux builds deliberately exclude this: there is no Wine/Proton layer, so a
//! Linux library shows emulated titles only. `available_on_this_os` is what
//! enforces that.
//!
//! Unlike every other runner, a game here is a *folder*, not a file. The folder
//! is the unit the user recognises and names; which executable inside it to run
//! is a separate question, answered by [`super::exe_pick`].

use super::{exe_pick, LaunchPlan, PerGameConfigStyle, Runner};
use crate::model::{Platform, RunnerId};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct NativePc;

const PLATFORMS: &[Platform] = &[Platform::Pc];

/// Only meaningful for the manual-override picker; scanning works on folders.
const EXTENSIONS: &[&str] = &["exe"];

impl Runner for NativePc {
    fn id(&self) -> RunnerId {
        RunnerId::NativePc
    }

    fn display_name(&self) -> &'static str {
        "PC (direct launch)"
    }

    fn platforms(&self) -> &'static [Platform] {
        PLATFORMS
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn available_on_this_os(&self) -> bool {
        cfg!(windows)
    }

    /// There is no emulator binary to configure -- the game *is* the program.
    fn detect_executable(&self) -> Option<PathBuf> {
        None
    }

    fn extract_title_id(&self, _path: &Path) -> Option<String> {
        None
    }

    /// A game is a directory containing at least one plausible executable.
    ///
    /// Requiring a real candidate (not merely any `.exe`) keeps folders that
    /// hold only redistributables or an uninstaller out of the library.
    fn accepts(&self, path: &Path) -> bool {
        if !path.is_dir() {
            return false;
        }
        exe_pick::pick_executable(path).is_some()
    }

    fn per_game_config_style(&self) -> PerGameConfigStyle {
        // Nothing to configure: the launcher runs the executable directly.
        PerGameConfigStyle::CommandLine
    }

    fn build_launch_plan(&self, _exe: &Path, game_path: &Path) -> Result<LaunchPlan> {
        // `game_path` may be the folder (normal) or a specific executable when
        // the user has overridden the choice.
        let target = if game_path.is_dir() {
            exe_pick::pick_executable(game_path).with_context(|| {
                format!("no runnable executable found in {}", game_path.display())
            })?
        } else {
            game_path.to_path_buf()
        };

        Ok(LaunchPlan {
            program: target.clone(),
            args: Vec::new(),
            // Many games resolve assets relative to their own directory and
            // fail to start if launched from elsewhere.
            working_dir: target.parent().map(Path::to_path_buf),
            restore_after_exit: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path, bytes: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; bytes]).unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nativepc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn accepts_a_folder_holding_a_real_game() {
        let dir = tmp("accept");
        let g = dir.join("Job-Simulator");
        touch(&g.join("Job Simulator/JobSimulator.exe"), 600_000);
        assert!(NativePc.accepts(&g));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_folder_of_only_redistributables() {
        let dir = tmp("reject");
        let g = dir.join("Not A Game");
        touch(&g.join("_CommonRedist/vcredist_x64.exe"), 14_000_000);
        assert!(!NativePc.accepts(&g), "redist-only folder must not be a game");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_plain_files() {
        assert!(!NativePc.accepts(Path::new("C:/x/game.exe")));
    }

    #[test]
    fn launch_plan_targets_the_picked_exe_and_its_own_directory() {
        let dir = tmp("launch");
        let g = dir.join("Vacation-Simulator");
        let exe = g.join("Vacation Simulator/Vacation Simulator.exe");
        touch(&exe, 600_000);
        std::fs::create_dir_all(g.join("Vacation Simulator/Vacation Simulator_Data")).unwrap();

        let plan = NativePc.build_launch_plan(Path::new(""), &g).unwrap();
        assert_eq!(plan.program.file_name().unwrap(), "Vacation Simulator.exe");
        // Games commonly resolve assets relative to cwd.
        assert_eq!(plan.working_dir.as_deref(), exe.parent());
        assert!(plan.args.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_overridden_executable_is_used_verbatim() {
        let dir = tmp("override");
        let g = dir.join("Some Game");
        let chosen = g.join("tools/Alternate.exe");
        touch(&chosen, 100_000);
        touch(&g.join("Some Game.exe"), 900_000);

        // Passing a file rather than the folder means the user picked it.
        let plan = NativePc.build_launch_plan(Path::new(""), &chosen).unwrap();
        assert_eq!(plan.program, chosen);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_windows_only() {
        assert_eq!(NativePc.available_on_this_os(), cfg!(windows));
    }
}
