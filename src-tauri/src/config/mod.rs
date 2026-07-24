//! Emulator configuration: global defaults plus per-game overrides.
//!
//! Overrides are written into each emulator's own per-game mechanism, so the
//! user's global configuration is never rewritten. See `runners::PerGameConfigStyle`.

pub mod cemu;
pub mod dolphin;
pub mod eden;
pub mod ini_merge;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Read a game's override blob as JSON text.
pub fn load_overrides(conn: &Connection, game_id: &str) -> Result<String> {
    Ok(conn
        .query_row(
            "SELECT overrides FROM game_config WHERE game_id = ?1",
            params![game_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_else(|| "{}".to_string()))
}

pub fn save_overrides(conn: &Connection, game_id: &str, json: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO game_config (game_id, overrides) VALUES (?1, ?2)
         ON CONFLICT(game_id) DO UPDATE SET overrides = excluded.overrides",
        params![game_id, json],
    )?;
    Ok(())
}
