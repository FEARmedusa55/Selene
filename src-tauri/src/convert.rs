//! File-format conversion for games a runner cannot read directly.
//!
//! Eden (this build) does not parse `.nsz` — those titles never appear in its
//! game list and fail to boot, confirmed against the user's own library. `.nsz`
//! is a zstd-compressed `.nsp`, so the fix is lossless decompression back to
//! `.nsp`, which Eden reads fine.
//!
//! Conversion is done by the external `nsz` CLI (nicoboss/nsz), never bundled:
//! consistent with the app supplying no keys, firmware, or game tooling. The
//! user's `prod.keys` is required and is staged where `nsz` looks for it.
//!
//! The original file is always kept; deleting it is a separate, explicit step.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Formats we can convert, and what they become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conversion {
    pub from_ext: &'static str,
    pub to_ext: &'static str,
}

/// If `path` is a convertible format, describe the conversion.
pub fn convertible(path: &Path) -> Option<Conversion> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        // nsz -> nsp, xcz -> xci: the two zstd-compressed Switch containers.
        "nsz" => Some(Conversion { from_ext: "nsz", to_ext: "nsp" }),
        "xcz" => Some(Conversion { from_ext: "xcz", to_ext: "xci" }),
        _ => None,
    }
}

/// Locate the `nsz` executable: PATH first, then the pip console-script dirs it
/// installs to, which are commonly not on PATH.
pub fn detect_nsz() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "nsz.exe" } else { "nsz" };

    // PATH.
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let c = dir.join(exe);
            if c.is_file() {
                return Some(c);
            }
        }
    }

    // pip Scripts / bin directories.
    let mut candidates = Vec::new();
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let py = PathBuf::from(&local).join("Python");
        // pythoncore-<ver>-64\Scripts\nsz.exe
        if let Ok(entries) = std::fs::read_dir(&py) {
            for e in entries.filter_map(Result::ok) {
                candidates.push(e.path().join("Scripts").join(exe));
            }
        }
        candidates.push(PathBuf::from(&local).join("Programs/Python").join(exe));
    }
    #[cfg(not(windows))]
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(&home).join(".local/bin").join(exe));
    }

    candidates.into_iter().find(|c| c.is_file())
}

/// `nsz` looks for `prod.keys` in `~/.switch/`. Stage the user's own key there
/// from the emulator's key directory if it is not already present.
///
/// The key never leaves the machine; this only copies it to the location the
/// tool expects. Returns an error if no key can be found, since conversion of
/// encrypted content cannot proceed without it.
pub fn ensure_keys(emulator_keys_dir: &Path) -> Result<()> {
    let home = dirs_home().context("cannot resolve home directory")?;
    let dest_dir = home.join(".switch");
    let dest = dest_dir.join("prod.keys");
    if dest.is_file() {
        return Ok(());
    }

    let src = emulator_keys_dir.join("prod.keys");
    if !src.is_file() {
        bail!(
            "prod.keys not found. Conversion needs your keys — expected them in {}",
            emulator_keys_dir.display()
        );
    }
    std::fs::create_dir_all(&dest_dir)?;
    std::fs::copy(&src, &dest)
        .with_context(|| format!("staging prod.keys into {}", dest_dir.display()))?;
    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// The path a conversion will produce: same directory and base name, new
/// extension.
pub fn output_path(input: &Path) -> Option<PathBuf> {
    let conv = convertible(input)?;
    let stem = input.file_stem()?;
    let mut name = stem.to_os_string();
    name.push(".");
    name.push(conv.to_ext);
    Some(input.with_file_name(name))
}

/// Decompress `input` in place (same folder), returning the produced file.
///
/// Blocking and potentially slow (multi-GB inputs take minutes), so callers run
/// it off the async runtime. The original is never touched.
pub fn convert(nsz_exe: &Path, input: &Path) -> Result<PathBuf> {
    let conv = convertible(input)
        .ok_or_else(|| anyhow::anyhow!("{} is not a convertible format", input.display()))?;
    let out_dir = input.parent().unwrap_or(Path::new("."));
    let expected = output_path(input)
        .ok_or_else(|| anyhow::anyhow!("cannot derive output path"))?;

    // Already converted: treat as success so the action is idempotent.
    if expected.is_file() {
        return Ok(expected);
    }

    let flag = match conv.from_ext {
        // -D decompresses; nsz picks nsp/xci output by the input container.
        _ => "-D",
    };

    let output = std::process::Command::new(nsz_exe)
        .arg(flag)
        .arg("-o")
        .arg(out_dir)
        .arg(input)
        .output()
        .with_context(|| format!("running {}", nsz_exe.display()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        // nsz reports key/verification failures on stdout, not stderr.
        bail!(
            "nsz failed ({}): {}",
            output.status,
            if !err.trim().is_empty() { err.trim() } else { out.trim() }
        );
    }

    if !expected.is_file() {
        bail!(
            "nsz reported success but {} was not produced",
            expected.display()
        );
    }
    Ok(expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn identifies_convertible_formats() {
        assert_eq!(
            convertible(Path::new("Terraria [0100E46006708000].nsz")),
            Some(Conversion { from_ext: "nsz", to_ext: "nsp" })
        );
        assert_eq!(
            convertible(Path::new("Game.xcz")).map(|c| c.to_ext),
            Some("xci")
        );
        assert!(convertible(Path::new("Game.NSZ")).is_some(), "case-insensitive");
        // Directly-runnable formats are not conversions.
        assert!(convertible(Path::new("Game.nsp")).is_none());
        assert!(convertible(Path::new("Game.xci")).is_none());
        assert!(convertible(Path::new("Game.iso")).is_none());
    }

    #[test]
    fn derives_output_path_beside_the_original() {
        let out = output_path(Path::new(r"D:\Switch\games\Terraria [0100E46006708000][v0] (0.16 GB).nsz"))
            .unwrap();
        assert_eq!(out.extension().unwrap(), "nsp");
        // Same folder, and the title ID survives for the scanner.
        assert!(out.to_string_lossy().contains("0100E46006708000"));
        assert!(out.to_string_lossy().ends_with(".nsp"));
        assert_eq!(out.parent(), Path::new(r"D:\Switch\games").into());
    }

    #[test]
    fn xcz_converts_to_xci() {
        let out = output_path(Path::new("Game [0100].xcz")).unwrap();
        assert_eq!(out.extension().unwrap(), "xci");
    }

    #[test]
    fn ensure_keys_errors_clearly_when_no_key_exists() {
        // Point at an empty dir with a home that has no ~/.switch/prod.keys.
        let tmp = std::env::temp_dir().join(format!("nokeys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        // Only assert the error path when the real home lacks a staged key,
        // which is the normal state; if a dev already has one, skip.
        if dirs_home().map(|h| h.join(".switch/prod.keys").is_file()) == Some(false) {
            let err = ensure_keys(&tmp).unwrap_err().to_string();
            assert!(err.contains("prod.keys"), "unhelpful error: {err}");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
