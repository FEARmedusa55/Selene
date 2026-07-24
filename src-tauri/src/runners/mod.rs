//! The `Runner` abstraction.
//!
//! Every way this app can start a game -- Cemu, Dolphin, Eden, or a PC
//! executable -- implements this one trait. That is what makes the library
//! genuinely unified instead of four parallel code paths, and it is why adding
//! a new emulator later is a single new module rather than edits scattered
//! across the scanner, launcher and config layers.

pub mod cemu;
pub mod disc;
pub mod dolphin;
pub mod eden;
pub mod exe_pick;
pub mod native_pc;
pub mod titleid;
pub mod wiiu;

use crate::model::{Platform, RunnerId, ScannedEntry};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// How a runner applies per-game settings.
///
/// The distinction matters: emulators with a native mechanism let us write an
/// isolated per-game file and leave the user's global config untouched. Only
/// runners with no such mechanism need the risky swap-and-restore dance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerGameConfigStyle {
    /// Emulator reads a dedicated per-title config file. Preferred.
    /// Dolphin: `GameSettings/<GameID>.ini`
    /// Cemu:    `gameProfiles/<TitleID>.ini`
    /// Eden:    `config/custom/<TitleID>.ini`
    NativeFile,
    /// Settings can be passed per launch, e.g. Dolphin's `-C Section.Key=Value`.
    CommandLine,
    /// No native support: back up the global config, write a modified copy,
    /// restore after the process exits. Last resort -- a crash mid-session can
    /// strand the user's real config, so implementations must restore on
    /// startup too.
    SwapGlobal,
}

pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    /// Files written before launch that must be reverted afterwards. Empty for
    /// runners using `NativeFile` or `CommandLine`.
    pub restore_after_exit: Vec<PathBuf>,
}

pub trait Runner: Send + Sync {
    fn id(&self) -> RunnerId;
    fn display_name(&self) -> &'static str;

    /// Platforms this runner covers. Drives the sidebar grouping.
    fn platforms(&self) -> &'static [Platform];

    /// Lowercase extensions, without the dot, that a scan should pick up.
    fn extensions(&self) -> &'static [&'static str];

    /// Whether this runner can run on the current OS at all. `native-pc`
    /// returns false on Linux by design -- there is no Wine/Proton layer, so
    /// Linux builds show emulated titles only.
    fn available_on_this_os(&self) -> bool {
        true
    }

    /// Common install locations to probe when the user has not set a path.
    fn detect_executable(&self) -> Option<PathBuf>;

    /// Pull a title ID out of a scanned path, where the format allows it.
    fn extract_title_id(&self, path: &Path) -> Option<String>;

    /// Which platform a specific file is for.
    ///
    /// Defaults to the runner's primary platform, which is correct for
    /// single-platform runners. Dolphin overrides it: it covers both GameCube
    /// and Wii, and the folder a file sits in is not proof of which it is.
    fn platform_for(&self, _path: &Path) -> Platform {
        self.platforms()[0]
    }

    fn per_game_config_style(&self) -> PerGameConfigStyle;

    /// Build the command line for a launch, applying merged config.
    fn build_launch_plan(&self, exe: &Path, game_path: &Path) -> Result<LaunchPlan>;

    /// Decide whether a scanned path belongs to this runner. Default matches on
    /// extension; runners with folder-based formats override it.
    fn accepts(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| self.extensions().contains(&e.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    /// Turn an accepted path into a scan entry.
    fn scan_entry(&self, path: &Path) -> Option<ScannedEntry> {
        if !self.accepts(path) {
            return None;
        }
        let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Some(ScannedEntry {
            path: path.to_string_lossy().into_owned(),
            cleaned_name: titleid::clean_filename(path),
            title_id: self.extract_title_id(path),
            platform: self.platform_for(path),
            runner: self.id(),
            size_bytes,
        })
    }
}

/// All runners available on this build, in display order.
///
/// `NativePc` is present on every platform so the UI can explain *why* PC games
/// are unavailable on Linux; `available_on_this_os` is what gates actual use.
pub fn all() -> Vec<Box<dyn Runner>> {
    vec![
        Box::new(dolphin::Dolphin),
        Box::new(cemu::Cemu),
        Box::new(eden::Eden),
        Box::new(native_pc::NativePc),
    ]
}
