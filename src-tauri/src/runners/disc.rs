//! GameCube / Wii disc header probing.
//!
//! The header is authoritative where the filename is not. Verified against the
//! user's own discs:
//!   Super Mario Galaxy.iso                  -> RMGP01, Wii magic at 0x00
//!   [Nintendo Wii] Super Mario Galaxy 2.wbfs -> SB4E01, Wii magic at 0x200
//!   Paper Mario - The Thousand-Year Door.iso -> G8ME01, GameCube magic at 0x00
//!
//! Two things this buys us that filenames cannot:
//!   * The real region code. Galaxy's ID is RMG**P**01 (PAL) -- guessing "E"
//!     for USA from the filename would have been wrong.
//!   * The real platform. Dolphin runs both GameCube and Wii, and the folder a
//!     file happens to sit in is not proof of which it is.

use crate::model::Platform;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const WII_MAGIC: [u8; 4] = [0x5D, 0x1C, 0x9E, 0xA3];
const GC_MAGIC: [u8; 4] = [0xC2, 0x33, 0x9F, 0x3D];

/// Offsets a disc header can start at. `.wbfs` wraps the disc in its own
/// container, putting the real header at 0x200.
const HEADER_OFFSETS: [u64; 2] = [0x0, 0x200];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscInfo {
    /// Six-character game ID, e.g. "RMGP01", "G8ME01".
    pub game_id: String,
    pub platform: Platform,
}

/// Parse a 0x40-byte header block. Returns `None` if neither magic is present.
fn parse_header(buf: &[u8]) -> Option<DiscInfo> {
    if buf.len() < 0x20 {
        return None;
    }
    let platform = if buf[0x18..0x1C] == WII_MAGIC {
        Platform::Wii
    } else if buf[0x1C..0x20] == GC_MAGIC {
        Platform::GameCube
    } else {
        return None;
    };

    let game_id = std::str::from_utf8(&buf[0..6]).ok()?.trim().to_string();
    // IDs are ASCII alphanumeric; anything else means we misread the offset.
    if game_id.len() != 6 || !game_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }

    Some(DiscInfo { game_id, platform })
}

/// Read the disc header from an uncompressed image (`.iso`, `.gcm`, `.wbfs`).
///
/// Returns `None` for compressed containers (`.rvz`, `.gcz`, `.ciso`), whose
/// headers are not readable without decompressing -- callers fall back to
/// [`guess_platform`].
pub fn probe(path: &Path) -> Option<DiscInfo> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0u8; 0x40];
    for offset in HEADER_OFFSETS {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        if file.read_exact(&mut buf).is_err() {
            continue;
        }
        if let Some(info) = parse_header(&buf) {
            return Some(info);
        }
    }
    None
}

/// Best-effort platform when the header cannot be read (compressed formats).
/// Extension first, then the containing folder name; neither is authoritative,
/// which is why `probe` is always tried first.
pub fn guess_platform(path: &Path) -> Platform {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();

    match ext.as_str() {
        "wbfs" => return Platform::Wii,
        "gcm" => return Platform::GameCube,
        _ => {}
    }

    let folder = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();

    if folder.contains("gamecube") || folder.contains("gcn") {
        Platform::GameCube
    } else {
        Platform::Wii
    }
}

/// Header if readable, otherwise a guess. Always yields a platform.
pub fn identify(path: &Path) -> (Option<String>, Platform) {
    match probe(path) {
        Some(info) => (Some(info.game_id), info.platform),
        None => (None, guess_platform(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(magic_at: usize, magic: [u8; 4], id: &[u8; 6]) -> Vec<u8> {
        let mut buf = vec![0u8; 0x40];
        buf[0..6].copy_from_slice(id);
        buf[magic_at..magic_at + 4].copy_from_slice(&magic);
        buf
    }

    #[test]
    fn parses_wii_header() {
        let buf = header(0x18, WII_MAGIC, b"RMGP01");
        assert_eq!(
            parse_header(&buf),
            Some(DiscInfo { game_id: "RMGP01".into(), platform: Platform::Wii })
        );
    }

    #[test]
    fn parses_gamecube_header() {
        let buf = header(0x1C, GC_MAGIC, b"G8ME01");
        assert_eq!(
            parse_header(&buf),
            Some(DiscInfo { game_id: "G8ME01".into(), platform: Platform::GameCube })
        );
    }

    #[test]
    fn rejects_blocks_without_magic() {
        assert!(parse_header(&vec![0u8; 0x40]).is_none());
        // Magic in the wrong place must not be accepted.
        assert!(parse_header(&header(0x10, WII_MAGIC, b"RMGP01")).is_none());
    }

    #[test]
    fn rejects_non_ascii_ids() {
        let mut buf = header(0x18, WII_MAGIC, b"RMGP01");
        buf[2] = 0xFF;
        assert!(parse_header(&buf).is_none());
    }

    #[test]
    fn guesses_platform_from_extension_before_folder() {
        // .wbfs in a "Gamecube Roms" folder is still a Wii image.
        let p = Path::new(r"d:\Games\Wii & Gamecube\Gamecube Roms\x.wbfs");
        assert_eq!(guess_platform(p), Platform::Wii);
        assert_eq!(guess_platform(Path::new(r"c:\x\y\game.gcm")), Platform::GameCube);
    }

    #[test]
    fn guesses_platform_from_folder_for_ambiguous_iso() {
        assert_eq!(
            guess_platform(Path::new(r"d:\Games\Wii & Gamecube\Gamecube Roms\Paper Mario.iso")),
            Platform::GameCube
        );
        assert_eq!(
            guess_platform(Path::new(r"d:\Games\Wii & Gamecube\Wii Roms\Galaxy.iso")),
            Platform::Wii
        );
    }
}
