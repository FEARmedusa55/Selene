//! Eden runner — Nintendo Switch.
//!
//! Requires user-supplied `prod.keys` and console firmware; this app ships
//! neither. Both are reported through [`Requirements`] so the UI can say
//! plainly what is missing instead of letting games fail to boot with an
//! opaque emulator error.

use super::{titleid, LaunchPlan, PerGameConfigStyle, Runner};
use crate::model::{Platform, RunnerId};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Eden;

const PLATFORMS: &[Platform] = &[Platform::Switch];

/// `.nsz` is a compressed `.nsp`. It is absent from the original spec but makes
/// up the majority of this library, and Eden reads it natively.
const EXTENSIONS: &[&str] = &["nsp", "xci", "nsz"];

/// What Eden needs from the user before anything will run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    pub prod_keys: bool,
    pub title_keys: bool,
    /// Firmware titles installed into Eden's NAND.
    pub firmware: bool,
    pub firmware_title_count: usize,
}

impl Requirements {
    /// Whether games can run at all.
    ///
    /// Keys are the hard requirement; **firmware is not**. Most titles boot and
    /// play on `prod.keys` alone. Firmware only matters when a game invokes a
    /// system applet — the software keyboard, profile select, error dialogs — or
    /// uses amiibo, and a few titles that check the version explicitly.
    ///
    /// An earlier version treated missing firmware as fatal and told the user
    /// "most titles will not boot", which was simply wrong: this library ran
    /// fine without it.
    pub fn satisfied(&self) -> bool {
        self.prod_keys
    }

    /// Firmware absent: some games will fail *partway through* rather than at
    /// launch, so this is worth surfacing — as a caveat, not a blocker.
    pub fn applets_unavailable(&self) -> bool {
        !self.firmware
    }
}

/// True for a base game's title ID.
///
/// Switch title IDs encode their kind in the low 12 bits: base games end in
/// `000`, updates in `800`, and DLC counts upward from `001`. The user's
/// library has update and DLC dumps sitting beside the games they belong to.
pub fn is_base_title_id(title_id: &str) -> bool {
    title_id.len() == 16 && title_id.ends_with("000")
}

/// Eden's data directory: `%APPDATA%\eden` on Windows, XDG data dir on Linux.
/// Portable installs keep a `user` folder beside the executable.
pub fn data_dir(eden_exe: &Path) -> Option<PathBuf> {
    if let Some(dir) = eden_exe.parent() {
        let portable = dir.join("user");
        if portable.is_dir() {
            return Some(portable);
        }
    }
    #[cfg(windows)]
    {
        std::env::var("APPDATA").ok().map(|a| PathBuf::from(a).join("eden"))
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share/eden"))
    }
}

pub fn check_requirements(data_dir: &Path) -> Requirements {
    let keys = data_dir.join("keys");
    let registered = data_dir
        .join("nand")
        .join("system")
        .join("Contents")
        .join("registered");

    let firmware_title_count = std::fs::read_dir(&registered)
        .map(|d| d.filter_map(Result::ok).count())
        .unwrap_or(0);

    Requirements {
        prod_keys: keys.join("prod.keys").is_file(),
        title_keys: keys.join("title.keys").is_file(),
        // An existing but empty `registered` folder is the common failure: the
        // directory is created on first run, long before firmware is installed.
        firmware: firmware_title_count > 0,
        firmware_title_count,
    }
}

impl Runner for Eden {
    fn id(&self) -> RunnerId {
        RunnerId::Eden
    }

    fn display_name(&self) -> &'static str {
        "Eden"
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
        // Switch dumps carry the 16-hex title ID in the filename, which is what
        // Eden's per-game config files are keyed on.
        titleid::switch_title_id(path)
    }

    /// Accepts base games only.
    ///
    /// Update and DLC dumps share the game's extensions and often sit in the
    /// same folder, so an extension check alone turns "A Hat in Time" plus its
    /// two DLC packs into three library entries. Files with no title ID at all
    /// are still accepted — better a scanned game with no ID than a silently
    /// skipped one.
    fn accepts(&self, path: &Path) -> bool {
        if path.is_dir() {
            return false;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false);
        if !ext_ok {
            return false;
        }
        match titleid::switch_title_id(path) {
            Some(id) => is_base_title_id(&id),
            None => true,
        }
    }

    fn per_game_config_style(&self) -> PerGameConfigStyle {
        // config/custom/<TitleID>.ini, inherited from yuzu.
        PerGameConfigStyle::NativeFile
    }

    fn build_launch_plan(&self, exe: &Path, game_path: &Path) -> Result<LaunchPlan> {
        let exe = exe
            .canonicalize()
            .with_context(|| format!("Eden executable not found at {}", exe.display()))?;

        Ok(LaunchPlan {
            program: exe,
            // `-g` matches yuzu's lineage. The GUI binary is used rather than
            // eden-cli so the user keeps the emulator's own window controls.
            args: vec!["-g".into(), game_path.to_string_lossy().into_owned()],
            working_dir: game_path.parent().map(Path::to_path_buf),
            restore_after_exit: Vec::new(),
        })
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut v = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join(r"eden\eden.exe"));
        }
        v.push(PathBuf::from(r"C:\Program Files\Eden\eden.exe"));
        v
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/usr/bin/eden"), PathBuf::from("/usr/local/bin/eden")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_switch_formats_including_nsz() {
        let e = Eden;
        assert!(e.accepts(Path::new("Super Mario Odyssey [0100000000010000][v0].nsp")));
        assert!(e.accepts(Path::new("Super Mario Bros. Wonder [010015100B514000][v0][US].xci")));
        // Most of this library is .nsz; omitting it would hide 23 of 37 games.
        assert!(e.accepts(Path::new("Deltarune [0100A0D022A68000][v0] (0.62 GB).nsz")));
        assert!(!e.accepts(Path::new("Skylanders.wua")));
        assert!(!e.accepts(Path::new("Galaxy.iso")));
    }

    #[test]
    fn rejects_updates_and_dlc_dumps() {
        let e = Eden;
        // Base game.
        assert!(e.accepts(Path::new("A Hat in Time [010056E00853A000][v0].nsp")));
        // Update: same extension, same folder, must not become a second entry.
        assert!(!e.accepts(Path::new("A Hat in Time [010056E00853A800][v262144].nsz")));
        // DLC.
        assert!(!e.accepts(Path::new("A Hat in Time [DLC Nyakuza Metro] [010056E00853B002][v0].nsp")));
        assert!(!e.accepts(Path::new("A Hat in Time [DLC Seal the Deal] [010056E00853B001][v0].nsp")));
        // No ID at all: still accepted rather than silently dropped.
        assert!(e.accepts(Path::new("Homebrew Thing.nsp")));
    }

    #[test]
    fn classifies_title_id_kinds() {
        assert!(is_base_title_id("0100000000010000"));
        assert!(!is_base_title_id("010056E00853A800"), "update");
        assert!(!is_base_title_id("010056E00853B002"), "dlc");
        assert!(!is_base_title_id("short"));
    }

    #[test]
    fn extracts_switch_title_ids_for_config_keying() {
        assert_eq!(
            Eden.extract_title_id(Path::new("Undertale [010080b00ad66000][v0].nsp"))
                .as_deref(),
            Some("010080B00AD66000")
        );
    }

    #[test]
    fn reports_missing_firmware_when_registered_is_empty() {
        let dir = std::env::temp_dir().join(format!("eden-req-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("keys")).unwrap();
        std::fs::write(dir.join("keys").join("prod.keys"), b"x").unwrap();
        // The directory exists but holds nothing — the real state on this
        // machine, and the case a naive `is_dir()` check would get wrong.
        std::fs::create_dir_all(dir.join("nand/system/Contents/registered")).unwrap();

        let req = check_requirements(&dir);
        assert!(req.prod_keys);
        assert!(!req.title_keys);
        assert!(!req.firmware, "empty registered dir must count as missing");
        assert_eq!(req.firmware_title_count, 0);
        // Keys alone are enough to play: missing firmware limits applets, it
        // does not stop games from booting.
        assert!(req.satisfied(), "missing firmware must not be treated as fatal");
        assert!(req.applets_unavailable());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_keys_is_the_real_blocker() {
        let dir = std::env::temp_dir().join(format!("eden-nokeys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let req = check_requirements(&dir);
        assert!(!req.prod_keys);
        assert!(!req.satisfied(), "without prod.keys nothing can run");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_satisfied_once_keys_and_firmware_are_present() {
        let dir = std::env::temp_dir().join(format!("eden-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("keys")).unwrap();
        std::fs::write(dir.join("keys/prod.keys"), b"x").unwrap();
        std::fs::write(dir.join("keys/title.keys"), b"x").unwrap();
        let reg = dir.join("nand/system/Contents/registered");
        std::fs::create_dir_all(&reg).unwrap();
        std::fs::write(reg.join("0100000000000809.nca"), b"x").unwrap();

        let req = check_requirements(&dir);
        assert!(req.satisfied());
        assert_eq!(req.firmware_title_count, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uses_native_per_game_config() {
        assert_eq!(Eden.per_game_config_style(), PerGameConfigStyle::NativeFile);
    }
}
