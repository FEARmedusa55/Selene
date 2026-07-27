//! Goldberg Steam Emulator integration for delisted / Steam-gated PC games.
//!
//! Goldberg replaces a game's `steam_api(64).dll` with an emulated build, so a
//! title that refuses to run without a live Steam client launches offline. It is
//! the fix for abandonware that was pulled from the store — the game the user
//! owns still runs, without depending on Steam.
//!
//! This app bundles no binaries: the user supplies a Goldberg release and points
//! Settings at it. Our job is the *setup*, and to make it reversible:
//!   1. find the game's Steam DLL(s) and their bitness,
//!   2. back up the untouched original (once — never a re-applied emu DLL),
//!   3. drop in the matching-bitness emu DLL plus `steamclient`,
//!   4. write a minimal `steam_settings` (the AppID),
//!   5. record everything in a manifest so removal restores the original exactly
//!      and deletes only files we created.
//!
//! Bitness is read from the DLL *name* — `steam_api.dll` is 32-bit,
//! `steam_api64.dll` is 64-bit — which is the Steamworks convention every game
//! follows, and which decides which emu DLL fits.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Recorded next to the game folder so a setup can be reverted precisely.
const MANIFEST_NAME: &str = ".selene-goldberg.json";
/// Backup of a replaced DLL, beside it: `steam_api.dll` -> `steam_api.dll.selene-bak`.
const BACKUP_SUFFIX: &str = ".selene-bak";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X86,
    X64,
}

impl Arch {
    pub fn as_str(self) -> &'static str {
        match self {
            Arch::X86 => "x86",
            Arch::X64 => "x64",
        }
    }
    fn steamclient_name(self) -> &'static str {
        match self {
            Arch::X86 => "steamclient.dll",
            Arch::X64 => "steamclient64.dll",
        }
    }
}

/// Bitness implied by a `steam_api` DLL filename, or `None` if not one.
fn steam_api_arch(file_name: &str) -> Option<Arch> {
    match file_name.to_ascii_lowercase().as_str() {
        "steam_api64.dll" => Some(Arch::X64),
        "steam_api.dll" => Some(Arch::X86),
        _ => None,
    }
}

// --- byte helpers -----------------------------------------------------------

/// Naive substring search. DLLs here are a few MB; this runs a handful of times.
fn contains_bytes(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.len() >= needle.len() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Whether a DLL is a Goldberg build. Both the classic emulator and its steam
/// client stamp the author string into the binary.
fn is_goldberg_bytes(bytes: &[u8]) -> bool {
    contains_bytes(bytes, b"Goldberg") || contains_bytes(bytes, b"Mr_Goldberg")
}

fn is_goldberg_file(path: &Path) -> bool {
    std::fs::read(path).map(|b| is_goldberg_bytes(&b)).unwrap_or(false)
}

// --- game-side detection ----------------------------------------------------

#[derive(Debug, Clone)]
pub struct SteamDll {
    pub path: PathBuf,
    pub arch: Arch,
}

/// Every `steam_api(64).dll` under a game folder. Usually one; VR titles
/// occasionally ship the DLL in a nested `_Data/Plugins` folder.
pub fn find_steam_dlls(game_dir: &Path) -> Vec<SteamDll> {
    walkdir::WalkDir::new(game_dir)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            steam_api_arch(&e.file_name().to_string_lossy())
                .map(|arch| SteamDll { path: e.path().to_path_buf(), arch })
        })
        .collect()
}

/// Distinct bitnesses present, in first-seen order.
fn needed_archs(dlls: &[SteamDll]) -> Vec<Arch> {
    let mut out = Vec::new();
    for d in dlls {
        if !out.contains(&d.arch) {
            out.push(d.arch);
        }
    }
    out
}

fn digits_only(s: &str) -> Option<String> {
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    (!d.is_empty()).then_some(d)
}

/// AppID from a `steam_appid.txt` body (first line, digits).
fn parse_appid_txt(body: &str) -> Option<String> {
    body.lines().next().and_then(digits_only)
}

/// AppID from an `.ini` (`AppID=243560`, any case), as CPY/CODEX cracks write.
fn parse_appid_ini(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim().eq_ignore_ascii_case("appid")).then(|| digits_only(v))?
    })
}

/// Best-effort AppID for a game folder: an existing `steam_appid.txt` wins;
/// otherwise the AppID a crack left in its `.ini`. Returns `None` if neither is
/// present, in which case the user must supply it.
pub fn detect_app_id(game_dir: &Path) -> Option<String> {
    let mut ini_hit: Option<String> = None;
    for e in walkdir::WalkDir::new(game_dir)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        if name == "steam_appid.txt" {
            if let Some(id) = std::fs::read_to_string(e.path()).ok().and_then(|s| parse_appid_txt(&s)) {
                return Some(id);
            }
        } else if ini_hit.is_none() && name.ends_with(".ini") {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                ini_hit = parse_appid_ini(&s);
            }
        }
    }
    ini_hit
}

// --- Goldberg release (user-supplied binaries) ------------------------------

/// Emu DLLs found inside the user's Goldberg folder, one per role/bitness.
#[derive(Debug, Clone, Default)]
pub struct Release {
    steam_api_x86: Option<PathBuf>,
    steam_api_x64: Option<PathBuf>,
    steamclient_x86: Option<PathBuf>,
    steamclient_x64: Option<PathBuf>,
}

impl Release {
    pub fn has(&self, arch: Arch) -> bool {
        self.steam_api(arch).is_some()
    }
    fn steam_api(&self, arch: Arch) -> Option<&Path> {
        match arch {
            Arch::X86 => self.steam_api_x86.as_deref(),
            Arch::X64 => self.steam_api_x64.as_deref(),
        }
    }
    fn steamclient(&self, arch: Arch) -> Option<&Path> {
        match arch {
            Arch::X86 => self.steamclient_x86.as_deref(),
            Arch::X64 => self.steamclient_x64.as_deref(),
        }
    }
}

/// Prefer the `experimental` build (it also handles CPY-style cracks and is the
/// modern default), then `release`/`regular`, over anything else.
fn build_preference(path: &Path) -> i32 {
    let p = path.to_string_lossy().to_ascii_lowercase();
    let mut s = 0;
    if p.contains("experimental") {
        s += 3;
    }
    if p.contains("release") || p.contains("regular") {
        s += 2;
    }
    if p.contains("debug") {
        s -= 1;
    }
    s
}

/// Locate the four emu DLLs inside a Goldberg release folder. Only files that
/// are actually Goldberg builds are accepted, so a stray game DLL sitting under
/// the folder can never be mistaken for the emulator.
pub fn resolve_release(goldberg_dir: &Path) -> Release {
    let mut out = Release::default();
    // Highest build-preference score seen for each of the four slots.
    let mut best = [i32::MIN; 4];
    for e in walkdir::WalkDir::new(goldberg_dir)
        .max_depth(5)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        let slot = match name.as_str() {
            "steam_api.dll" => 0,
            "steam_api64.dll" => 1,
            "steamclient.dll" => 2,
            "steamclient64.dll" => 3,
            _ => continue,
        };
        let path = e.path();
        // Only the steam_api DLLs are verified as Goldberg builds: those can
        // collide with a game's own steam_api.dll, so the marker guards against
        // grabbing the wrong file. The steamclient DLLs carry no such marker
        // (they don't stamp the author string), and a file literally named
        // steamclient.dll inside the release folder is the emulator's — so
        // requiring the marker there would wrongly skip it every time.
        let is_steam_api = slot == 0 || slot == 1;
        if is_steam_api && !is_goldberg_file(path) {
            continue;
        }
        let score = build_preference(path);
        if score <= best[slot] {
            continue;
        }
        best[slot] = score;
        let p = Some(path.to_path_buf());
        match slot {
            0 => out.steam_api_x86 = p,
            1 => out.steam_api_x64 = p,
            2 => out.steamclient_x86 = p,
            _ => out.steamclient_x64 = p,
        }
    }
    out
}

// --- manifest ---------------------------------------------------------------

/// Exact record of a setup, so removal is a precise inverse rather than a guess.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Manifest {
    app_id: String,
    /// DLLs we overwrote; each has a sibling `*.selene-bak` original.
    replaced: Vec<PathBuf>,
    /// Files we created (steamclient, steam_settings entries) — safe to delete.
    created: Vec<PathBuf>,
    /// Directories we created (steam_settings) — removed only when left empty.
    created_dirs: Vec<PathBuf>,
}

fn manifest_path(game_dir: &Path) -> PathBuf {
    game_dir.join(MANIFEST_NAME)
}

fn read_manifest(game_dir: &Path) -> Option<Manifest> {
    let raw = std::fs::read_to_string(manifest_path(game_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_manifest(game_dir: &Path, m: &Manifest) -> Result<()> {
    std::fs::write(manifest_path(game_dir), serde_json::to_string_pretty(m)?)
        .context("writing Goldberg manifest")
}

fn backup_path(dll: &Path) -> PathBuf {
    let mut s = dll.as_os_str().to_os_string();
    s.push(BACKUP_SUFFIX);
    PathBuf::from(s)
}

fn push_unique(v: &mut Vec<PathBuf>, p: &Path) {
    if !v.iter().any(|x| x == p) {
        v.push(p.to_path_buf());
    }
}

// --- player (account) name --------------------------------------------------
//
// A Goldberg name is per-person, not per-game: when friends play the same game
// over LAN, each machine has one name it uses for every title. So this is a
// single global setting, stored where Goldberg reads it — the `settings` folder
// of its save directory — and it applies to every Goldberg game on the machine.

/// Goldberg's global save directory: `%APPDATA%\Goldberg SteamEmu Saves`.
pub fn saves_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Goldberg SteamEmu Saves"))
}

fn account_name_in(saves: &Path) -> Option<String> {
    std::fs::read_to_string(saves.join("settings").join("account_name.txt"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn set_account_name_in(saves: &Path, name: &str) -> Result<()> {
    let settings = saves.join("settings");
    let file = settings.join("account_name.txt");
    // Keep it to a single trimmed line; Steam display names top out at 32 chars.
    let name: String = name.trim().lines().next().unwrap_or("").trim().chars().take(32).collect();
    if name.is_empty() {
        // Clearing reverts Goldberg to its built-in default ("Goldberg").
        let _ = std::fs::remove_file(&file);
        return Ok(());
    }
    std::fs::create_dir_all(&settings)
        .with_context(|| format!("creating {}", settings.display()))?;
    std::fs::write(&file, name).context("writing account_name.txt")
}

/// The current global Goldberg player name, or `None` if unset (default).
pub fn account_name() -> Option<String> {
    saves_dir().and_then(|d| account_name_in(&d))
}

/// Set (or clear, when empty) the global Goldberg player name.
pub fn set_account_name(name: &str) -> Result<()> {
    let saves = saves_dir().ok_or_else(|| anyhow::anyhow!("could not resolve %APPDATA%"))?;
    set_account_name_in(&saves, name)
}

// --- status -----------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DllStatus {
    path: String,
    arch: String,
    /// The DLL currently in place is a Goldberg build.
    goldberg: bool,
    /// A Selene backup of the original sits beside it.
    backup: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This is a PC game with at least one Steam DLL — the panel shows only then.
    supported: bool,
    /// A Goldberg DLL is currently installed.
    applied: bool,
    /// Selene applied it and holds a manifest, so it can revert cleanly.
    managed: bool,
    app_id: Option<String>,
    dlls: Vec<DllStatus>,
    goldberg_dir: Option<String>,
    /// The configured release supplies every bitness this game needs.
    goldberg_ready: bool,
    /// The global Goldberg player name (how you appear to friends on LAN).
    account_name: Option<String>,
    /// Reasons Apply is currently unavailable, phrased for the user.
    blockers: Vec<String>,
}

/// Inspect a game folder and the configured release, and report what can be done.
pub fn status(game_dir: &Path, goldberg_dir: Option<&Path>) -> Status {
    let dlls = find_steam_dlls(game_dir);
    let supported = !dlls.is_empty();

    let dll_status: Vec<DllStatus> = dlls
        .iter()
        .map(|d| DllStatus {
            path: d.path.display().to_string(),
            arch: d.arch.as_str().to_string(),
            goldberg: is_goldberg_file(&d.path),
            backup: backup_path(&d.path).exists(),
        })
        .collect();
    let applied = dll_status.iter().any(|d| d.goldberg);

    let manifest = read_manifest(game_dir);
    let managed = manifest.is_some();
    let app_id = manifest
        .as_ref()
        .map(|m| m.app_id.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| detect_app_id(game_dir));

    let release = goldberg_dir.map(resolve_release);
    let missing_archs: Vec<Arch> = needed_archs(&dlls)
        .into_iter()
        .filter(|a| release.as_ref().map_or(true, |r| !r.has(*a)))
        .collect();
    let goldberg_ready = release.is_some() && supported && missing_archs.is_empty();

    let mut blockers = Vec::new();
    if goldberg_dir.is_none() {
        blockers.push("Set your Goldberg emulator folder in Settings.".to_string());
    } else if !missing_archs.is_empty() {
        let list = missing_archs.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(" and ");
        blockers.push(format!(
            "Your Goldberg folder has no {list} steam_api DLL for this game."
        ));
    }
    if app_id.is_none() {
        blockers.push("No Steam AppID was detected — enter one below.".to_string());
    }

    Status {
        supported,
        applied,
        managed,
        app_id,
        dlls: dll_status,
        goldberg_dir: goldberg_dir.map(|p| p.display().to_string()),
        goldberg_ready,
        account_name: account_name(),
        blockers,
    }
}

// --- apply / remove ---------------------------------------------------------

/// Install Goldberg into a game folder. Idempotent: re-applying refreshes the
/// emu DLLs and AppID without ever backing up an already-Goldberg DLL as if it
/// were the original.
pub fn apply(game_dir: &Path, release: &Release, app_id: &str) -> Result<()> {
    let app_id = app_id.trim();
    if app_id.is_empty() || !app_id.chars().all(|c| c.is_ascii_digit()) {
        bail!("A numeric Steam AppID is required.");
    }

    let dlls = find_steam_dlls(game_dir);
    if dlls.is_empty() {
        bail!("No steam_api DLL was found in this game's folder.");
    }
    for arch in needed_archs(&dlls) {
        if !release.has(arch) {
            bail!(
                "Your Goldberg folder has no {} steam_api DLL, which this game needs.",
                arch.as_str()
            );
        }
    }

    // Extend any existing manifest so re-applies keep the full created/replaced
    // history — otherwise removal would leak files added on a first pass.
    let mut manifest = read_manifest(game_dir).unwrap_or_default();
    manifest.app_id = app_id.to_string();

    for dll in &dlls {
        let emu = release.steam_api(dll.arch).expect("checked above");
        let backup = backup_path(&dll.path);

        // Preserve the *true* original exactly once: never overwrite an existing
        // backup, and never capture a DLL that is already Goldberg.
        if !backup.exists() && !is_goldberg_file(&dll.path) {
            std::fs::copy(&dll.path, &backup)
                .with_context(|| format!("backing up {}", dll.path.display()))?;
        }
        std::fs::copy(emu, &dll.path)
            .with_context(|| format!("installing Goldberg into {}", dll.path.display()))?;
        push_unique(&mut manifest.replaced, &dll.path);

        let dir = dll.path.parent().unwrap_or(game_dir);

        // steamclient beside the DLL, if the release ships it and the game does
        // not already have one (leave a pre-existing file alone).
        if let Some(sc) = release.steamclient(dll.arch) {
            let dest = dir.join(dll.arch.steamclient_name());
            if !dest.exists() {
                std::fs::copy(sc, &dest)
                    .with_context(|| format!("copying {}", dest.display()))?;
                push_unique(&mut manifest.created, &dest);
            }
        }

        // steam_settings/steam_appid.txt is where Goldberg looks first.
        let settings = dir.join("steam_settings");
        if !settings.exists() {
            std::fs::create_dir_all(&settings)
                .with_context(|| format!("creating {}", settings.display()))?;
            push_unique(&mut manifest.created_dirs, &settings);
        }
        let appid_file = settings.join("steam_appid.txt");
        let existed = appid_file.exists();
        std::fs::write(&appid_file, app_id)?;
        if !existed {
            push_unique(&mut manifest.created, &appid_file);
        }

        // Some games read steam_appid.txt from the working directory (beside the
        // exe/DLL). Add one only if the game did not already ship its own.
        let beside = dir.join("steam_appid.txt");
        if !beside.exists() {
            std::fs::write(&beside, app_id)?;
            push_unique(&mut manifest.created, &beside);
        }
    }

    write_manifest(game_dir, &manifest)
}

/// Revert a Selene-applied setup: restore every original DLL and delete only the
/// files we created. Requires the manifest — we will not guess at a setup we did
/// not record.
pub fn remove(game_dir: &Path) -> Result<()> {
    let manifest = read_manifest(game_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "This game's Goldberg setup was not applied by Selene, so there is \
             nothing recorded to revert. Restore the original DLL by hand if needed."
        )
    })?;

    for dll in &manifest.replaced {
        let backup = backup_path(dll);
        if backup.exists() {
            std::fs::copy(&backup, dll)
                .with_context(|| format!("restoring {}", dll.display()))?;
            let _ = std::fs::remove_file(&backup);
        }
    }
    for f in &manifest.created {
        let _ = std::fs::remove_file(f);
    }
    // remove_dir only succeeds on an empty directory, so a folder the user added
    // their own files to is left untouched.
    for d in &manifest.created_dirs {
        let _ = std::fs::remove_dir(d);
    }
    let _ = std::fs::remove_file(manifest_path(game_dir));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("goldberg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// A stand-in "real" Goldberg release: experimental + a decoy release dir.
    fn fake_release(root: &Path) {
        write(&root.join("experimental/steam_api.dll"), b"...Goldberg emu x86...");
        write(&root.join("experimental/steam_api64.dll"), b"...Goldberg emu x64...");
        // Real Goldberg steamclient DLLs carry no author marker, so the fixtures
        // must not either — this is what proves resolve_release still finds them.
        write(&root.join("experimental/steamclient.dll"), b"...emulated steamclient x86...");
        write(&root.join("experimental/steamclient64.dll"), b"...emulated steamclient x64...");
    }

    #[test]
    fn reads_bitness_from_the_dll_name() {
        assert_eq!(steam_api_arch("steam_api.dll"), Some(Arch::X86));
        assert_eq!(steam_api_arch("STEAM_API64.DLL"), Some(Arch::X64));
        assert_eq!(steam_api_arch("steamclient.dll"), None);
    }

    #[test]
    fn finds_nested_steam_dlls_with_their_arch() {
        let g = tmp("find");
        write(&g.join("Game/steam_api.dll"), b"orig");
        write(&g.join("Game/Game_Data/Plugins/steam_api64.dll"), b"orig");
        let mut found = find_steam_dlls(&g);
        found.sort_by_key(|d| d.arch as u8 as i32);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|d| d.arch == Arch::X86));
        assert!(found.iter().any(|d| d.arch == Arch::X64));
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn detects_appid_from_txt_then_ini() {
        let g = tmp("appid-txt");
        write(&g.join("exec/steam_appid.txt"), b"243560\n");
        assert_eq!(detect_app_id(&g).as_deref(), Some("243560"));
        let _ = std::fs::remove_dir_all(&g);

        // No txt: fall back to a crack's ini (the real CPY.ini shape).
        let g2 = tmp("appid-ini");
        write(
            &g2.join("exec/CPY.ini"),
            b"[Settings]\nLanguage=english\nAppID=243560\nPlayerName=CPY\n",
        );
        assert_eq!(detect_app_id(&g2).as_deref(), Some("243560"));
        let _ = std::fs::remove_dir_all(&g2);
    }

    #[test]
    fn release_resolution_prefers_experimental_and_requires_goldberg() {
        let r = tmp("release");
        fake_release(&r);
        // A game's own (non-Goldberg) DLL sitting under the folder must be ignored.
        write(&r.join("some_game/steam_api.dll"), b"valve original, not goldberg");
        let rel = resolve_release(&r);
        assert!(rel.has(Arch::X86) && rel.has(Arch::X64));
        assert!(rel.steam_api(Arch::X86).unwrap().to_string_lossy().contains("experimental"));
        assert!(rel.steamclient(Arch::X64).is_some());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn apply_then_remove_restores_the_original_exactly() {
        let r = tmp("rt-release");
        fake_release(&r);
        let rel = resolve_release(&r);

        let g = tmp("rt-game");
        let dll = g.join("executable/steam_api.dll");
        write(&dll, b"ORIGINAL CPY DLL BYTES");

        apply(&g, &rel, "243560").unwrap();

        // DLL swapped, backup + settings + steamclient created.
        assert!(is_goldberg_file(&dll), "emu DLL not installed");
        assert!(backup_path(&dll).exists(), "no backup taken");
        assert_eq!(
            std::fs::read_to_string(g.join("executable/steam_settings/steam_appid.txt")).unwrap(),
            "243560"
        );
        assert!(g.join("executable/steamclient.dll").exists());

        let st = status(&g, Some(&r));
        assert!(st.supported && st.applied && st.managed);
        assert!(st.blockers.is_empty(), "unexpected blockers: {:?}", st.blockers);

        remove(&g).unwrap();

        // Byte-for-byte original back; every created file gone.
        assert_eq!(std::fs::read(&dll).unwrap(), b"ORIGINAL CPY DLL BYTES");
        assert!(!backup_path(&dll).exists(), "backup left behind");
        assert!(!g.join("executable/steamclient.dll").exists());
        assert!(!g.join("executable/steam_settings").exists());
        assert!(!manifest_path(&g).exists());
        assert!(!status(&g, Some(&r)).applied);

        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn reapply_never_captures_the_emu_dll_as_the_original() {
        let r = tmp("re-release");
        fake_release(&r);
        let rel = resolve_release(&r);

        let g = tmp("re-game");
        let dll = g.join("bin/steam_api.dll");
        write(&dll, b"THE REAL ORIGINAL");

        apply(&g, &rel, "480").unwrap();
        apply(&g, &rel, "480").unwrap(); // second pass must not clobber the backup

        assert_eq!(
            std::fs::read(backup_path(&dll)).unwrap(),
            b"THE REAL ORIGINAL",
            "re-apply overwrote the backed-up original with a Goldberg DLL"
        );
        remove(&g).unwrap();
        assert_eq!(std::fs::read(&dll).unwrap(), b"THE REAL ORIGINAL");
        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn remove_without_a_manifest_is_an_explained_error() {
        let g = tmp("nomani");
        write(&g.join("steam_api.dll"), b"x");
        let e = remove(&g).unwrap_err().to_string();
        assert!(e.contains("not applied by Selene"), "{e}");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn apply_rejects_a_non_numeric_appid() {
        let r = tmp("bad-release");
        fake_release(&r);
        let rel = resolve_release(&r);
        let g = tmp("bad-game");
        write(&g.join("steam_api.dll"), b"orig");
        assert!(apply(&g, &rel, "not-a-number").is_err());
        assert!(apply(&g, &rel, "").is_err());
        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn status_blocks_when_release_lacks_the_needed_bitness() {
        // Game is 64-bit; release only has 32-bit.
        let r = tmp("half-release");
        write(&r.join("experimental/steam_api.dll"), b"...Goldberg x86...");
        let rel_dir = r.clone();
        let g = tmp("half-game");
        write(&g.join("steam_api64.dll"), b"orig64");
        let st = status(&g, Some(&rel_dir));
        assert!(st.supported && !st.applied);
        assert!(!st.goldberg_ready);
        assert!(st.blockers.iter().any(|b| b.contains("x64")), "{:?}", st.blockers);
        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn a_pc_game_without_a_steam_dll_is_unsupported() {
        let g = tmp("nosteam");
        write(&g.join("Game.exe"), b"MZ...");
        let st = status(&g, None);
        assert!(!st.supported);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn account_name_round_trips_and_clears() {
        let saves = tmp("saves");
        assert_eq!(account_name_in(&saves), None, "no name should read as None");

        set_account_name_in(&saves, "Finn").unwrap();
        assert_eq!(account_name_in(&saves).as_deref(), Some("Finn"));
        assert_eq!(
            std::fs::read_to_string(saves.join("settings/account_name.txt")).unwrap(),
            "Finn"
        );

        // Empty clears the file (reverts to Goldberg's default).
        set_account_name_in(&saves, "   ").unwrap();
        assert_eq!(account_name_in(&saves), None);
        assert!(!saves.join("settings/account_name.txt").exists());
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn account_name_is_one_trimmed_line_capped() {
        let saves = tmp("saves-clean");
        set_account_name_in(&saves, "  Jake\nthe Dog  ").unwrap();
        // First line only, trimmed — no newline injected into the lobby name.
        assert_eq!(account_name_in(&saves).as_deref(), Some("Jake"));

        set_account_name_in(&saves, &"x".repeat(100)).unwrap();
        assert_eq!(account_name_in(&saves).unwrap().len(), 32, "name not capped");
        let _ = std::fs::remove_dir_all(&saves);
    }
}
