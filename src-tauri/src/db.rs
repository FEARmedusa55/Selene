//! Local-first SQLite storage.
//!
//! Migrations are applied in order and tracked via `PRAGMA user_version`, so
//! upgrades are additive and never destroy a user's library.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Each entry is one schema version. Append only -- never edit a shipped entry.
const MIGRATIONS: &[&str] = &[
    // v1 -- initial schema
    r#"
    CREATE TABLE games (
        id                TEXT PRIMARY KEY,
        title             TEXT NOT NULL,
        platform          TEXT NOT NULL,
        runner            TEXT NOT NULL,
        path              TEXT NOT NULL UNIQUE,
        title_id          TEXT,
        cover_url         TEXT,
        hero_url          TEXT,
        playtime_seconds  INTEGER NOT NULL DEFAULT 0,
        last_played_at    TEXT,
        added_at          TEXT NOT NULL,
        favorite          INTEGER NOT NULL DEFAULT 0,
        -- Set when the user overrides auto-detection (e.g. picks a different
        -- .exe for a PC game). Kept separate so a rescan never clobbers it.
        launch_override   TEXT
    );
    CREATE INDEX idx_games_platform ON games(platform);
    CREATE INDEX idx_games_runner   ON games(runner);
    CREATE INDEX idx_games_title_id ON games(title_id);

    CREATE TABLE tags (
        id    INTEGER PRIMARY KEY AUTOINCREMENT,
        name  TEXT NOT NULL UNIQUE
    );

    CREATE TABLE game_tags (
        game_id  TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
        PRIMARY KEY (game_id, tag_id)
    );

    -- User-created collections, ordered manually rather than alphabetically.
    CREATE TABLE collections (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        name       TEXT NOT NULL UNIQUE,
        sort_order INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE collection_games (
        collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
        game_id       TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        sort_order    INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (collection_id, game_id)
    );

    -- Folders the scanner walks, one row per (runner, folder) pair. PC games
    -- support several roots, which is why this is not a single column.
    CREATE TABLE scan_roots (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        runner  TEXT NOT NULL,
        path    TEXT NOT NULL,
        UNIQUE (runner, path)
    );

    -- Where each emulator binary lives, plus its global default config as JSON.
    CREATE TABLE runners (
        id             TEXT PRIMARY KEY,
        executable     TEXT,
        global_config  TEXT NOT NULL DEFAULT '{}'
    );

    -- Per-game config overrides, layered on top of the runner's global config
    -- and then written into the emulator's own per-game mechanism at launch.
    CREATE TABLE game_config (
        game_id    TEXT PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
        overrides  TEXT NOT NULL DEFAULT '{}'
    );

    -- One row per play session. Aggregate playtime is denormalised onto
    -- games.playtime_seconds; this table keeps the history behind it.
    CREATE TABLE play_sessions (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        game_id           TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        started_at        TEXT NOT NULL,
        ended_at          TEXT,
        duration_seconds  INTEGER
    );
    CREATE INDEX idx_sessions_game ON play_sessions(game_id);

    -- App-level key/value: active theme, IGDB token cache, window state.
    CREATE TABLE settings (
        key    TEXT PRIMARY KEY,
        value  TEXT NOT NULL
    );
    "#,
    // v2 -- manual artwork override
    //
    // Kept in separate columns rather than overwriting cover_url/hero_url so a
    // later artwork refresh cannot silently discard the user's choice, and so
    // "revert to IGDB" is possible without re-querying.
    r#"
    ALTER TABLE games ADD COLUMN cover_override TEXT;
    ALTER TABLE games ADD COLUMN hero_override  TEXT;
    "#,
];

pub fn open(config_dir: &Path) -> Result<Connection> {
    std::fs::create_dir_all(config_dir)
        .with_context(|| format!("creating config dir {}", config_dir.display()))?;

    let path = config_dir.join("library.db");
    let conn = Connection::open(&path)
        .with_context(|| format!("opening database {}", path.display()))?;

    // WAL keeps the UI responsive while a scan writes in the background.
    // NOTE: WAL requires shared-memory support from the filesystem; exFAT
    // volumes can reject it, so fall back to the default journal rather than
    // failing to start.
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        log::warn!("WAL unavailable ({e}); continuing with default journal mode");
    }
    conn.pragma_update(None, "foreign_keys", "ON")?;

    migrate(&conn)?;
    Ok(conn)
}

/// Apply the schema to a bare connection. Exposed so tests can build an
/// in-memory database without touching the user's real library file.
pub fn apply_migrations_for_test(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let target = MIGRATIONS.len() as i64;

    if current > target {
        anyhow::bail!(
            "database schema v{current} is newer than this build supports (v{target}); \
             upgrade the app rather than downgrading the library"
        );
    }

    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = idx as i64 + 1;
        log::info!("applying schema migration v{version}");
        conn.execute_batch(sql)
            .with_context(|| format!("applying migration v{version}"))?;
        conn.pragma_update(None, "user_version", version)?;
    }
    Ok(())
}

/// Config directory: `%APPDATA%\Selene` on Windows, `~/.config/Selene` on
/// Linux. Holds the database, artwork cache, and user-authored theme files.
pub fn config_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "Selene")
        .context("could not resolve a config directory for this platform")?;
    let dir = dirs.config_dir().to_path_buf();
    adopt_legacy_dir(&dir);
    Ok(dir)
}

/// The app was called "launcher" before it was named Selene. Move that data
/// across once, so an existing library, artwork cache and themes survive the
/// rename instead of the app starting up empty.
fn adopt_legacy_dir(current: &Path) {
    if current.exists() {
        return;
    }
    let Some(legacy) = directories::ProjectDirs::from("", "", "launcher") else {
        return;
    };
    let legacy = legacy.config_dir();
    if !legacy.exists() {
        return;
    }
    if let Some(parent) = current.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(legacy, current) {
        Ok(()) => log::info!("migrated config from {}", legacy.display()),
        Err(e) => log::warn!("could not migrate config from {}: {e}", legacy.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let dir = std::env::temp_dir().join(format!("launcher-test-{}", std::process::id()));
        let conn = open(&dir).expect("first open should apply migrations");
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        drop(conn);

        // Reopening must be a no-op, not a re-run.
        let conn = open(&dir).expect("second open should skip applied migrations");
        let v2: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v2, MIGRATIONS.len() as i64);

        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
