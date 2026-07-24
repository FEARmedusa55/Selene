//! Domain model. These types are serialised straight to the frontend, so they
//! mirror `src/types.ts` -- keep the two in sync.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    WiiU,
    Wii,
    GameCube,
    Switch,
    Pc,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::WiiU => "wiiu",
            Platform::Wii => "wii",
            Platform::GameCube => "gamecube",
            Platform::Switch => "switch",
            Platform::Pc => "pc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunnerId {
    Cemu,
    Dolphin,
    Eden,
    NativePc,
}

impl RunnerId {
    pub fn as_str(self) -> &'static str {
        match self {
            RunnerId::Cemu => "cemu",
            RunnerId::Dolphin => "dolphin",
            RunnerId::Eden => "eden",
            RunnerId::NativePc => "native-pc",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub title: String,
    pub platform: Platform,
    pub runner: RunnerId,
    /// ROM file, game folder, or PC executable.
    pub path: String,
    /// Extracted from the filename where present -- e.g. "0100000000010000"
    /// (Switch), "AGMP01" (Wii U), "RMGE01" (GameCube/Wii). This is the primary
    /// key for metadata matching; it beats fuzzy title search by a wide margin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hero_url: Option<String>,
    pub playtime_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played_at: Option<String>,
    pub added_at: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    /// True while the launched process tree is still alive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    /// Artwork was set by hand rather than resolved from IGDB.
    pub custom_art: bool,
}

/// A scan result before it has been matched against metadata or persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedEntry {
    pub path: String,
    /// Filename with extension, version and region markers stripped -- the
    /// string handed to IGDB when no title ID is available.
    pub cleaned_name: String,
    pub title_id: Option<String>,
    pub platform: Platform,
    pub runner: RunnerId,
    pub size_bytes: u64,
}
