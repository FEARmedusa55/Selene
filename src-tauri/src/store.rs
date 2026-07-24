//! Database access for the library.
//!
//! The central rule here: **a rescan must never destroy user data.** Upserts
//! touch only the fields the scanner owns (title, platform, runner, title_id)
//! and leave playtime, favourites, tags, artwork and `added_at` alone.

use crate::model::{Game, Platform, RunnerId, ScannedEntry};
use crate::scan;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

fn platform_from_str(s: &str) -> Platform {
    match s {
        "wiiu" => Platform::WiiU,
        "gamecube" => Platform::GameCube,
        "switch" => Platform::Switch,
        "pc" => Platform::Pc,
        _ => Platform::Wii,
    }
}

fn runner_from_str(s: &str) -> RunnerId {
    match s {
        "cemu" => RunnerId::Cemu,
        "eden" => RunnerId::Eden,
        "native-pc" => RunnerId::NativePc,
        _ => RunnerId::Dolphin,
    }
}

pub fn now_iso() -> String {
    // RFC3339-ish UTC without pulling in chrono; SQLite stores it as text and
    // the frontend parses it with `new Date(...)`.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_utc(secs)
}

fn format_unix_utc(secs: u64) -> String {
    // Days-from-civil algorithm (Howard Hinnant), avoiding a date dependency.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Insert new games, refresh scanner-owned fields on existing ones.
/// Returns (inserted, updated).
pub fn upsert_scanned(conn: &Connection, entries: &[ScannedEntry]) -> Result<(usize, usize)> {
    let now = now_iso();
    let mut inserted = 0;
    let mut updated = 0;

    for e in entries {
        let id = scan::id_for_path(Path::new(&e.path));
        let exists: bool = conn
            .query_row("SELECT 1 FROM games WHERE path = ?1", params![e.path], |_| Ok(()))
            .optional()
            .context("checking for existing game")?
            .is_some();

        if exists {
            // Only scanner-owned columns. Note the absence of playtime,
            // favorite, added_at, cover_url and hero_url.
            //
            // `title_id` uses COALESCE deliberately: a scan that cannot resolve
            // an ID must not erase one already known. Wii U `.wua` IDs come
            // from Cemu's title cache, so a rescan taken while that cache is
            // unavailable would otherwise null them out -- and a game with no
            // title ID silently loses its per-game config, since there is no
            // filename to key the profile on.
            conn.execute(
                "UPDATE games
                 SET platform = ?1, runner = ?2, title_id = COALESCE(?3, title_id)
                 WHERE path = ?4",
                params![e.platform.as_str(), e.runner.as_str(), e.title_id, e.path],
            )?;
            updated += 1;
        } else {
            conn.execute(
                "INSERT INTO games (id, title, platform, runner, path, title_id, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    e.cleaned_name,
                    e.platform.as_str(),
                    e.runner.as_str(),
                    e.path,
                    e.title_id,
                    now
                ],
            )?;
            inserted += 1;
        }
    }
    Ok((inserted, updated))
}

pub fn list_games(conn: &Connection) -> Result<Vec<Game>> {
    // A manual override wins over whatever IGDB resolved, but the IGDB value is
    // kept alongside it so the user can revert without re-fetching.
    let mut stmt = conn.prepare(
        "SELECT id, title, platform, runner, path, title_id,
                COALESCE(cover_override, cover_url),
                COALESCE(hero_override, hero_url),
                playtime_seconds, last_played_at, added_at, favorite,
                cover_override IS NOT NULL OR hero_override IS NOT NULL
         FROM games ORDER BY title COLLATE NOCASE",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(Game {
            id: r.get(0)?,
            title: r.get(1)?,
            platform: platform_from_str(&r.get::<_, String>(2)?),
            runner: runner_from_str(&r.get::<_, String>(3)?),
            path: r.get(4)?,
            title_id: r.get(5)?,
            cover_url: r.get(6)?,
            hero_url: r.get(7)?,
            playtime_seconds: r.get(8)?,
            last_played_at: r.get(9)?,
            added_at: r.get(10)?,
            tags: Vec::new(),
            favorite: r.get::<_, i64>(11)? != 0,
            running: None,
            custom_art: r.get::<_, i64>(12)? != 0,
        })
    })?;

    let mut games: Vec<Game> = rows.collect::<rusqlite::Result<_>>()?;

    // Tags come from a join table; fetched in one pass rather than per game.
    let mut tag_stmt = conn.prepare(
        "SELECT gt.game_id, t.name FROM game_tags gt JOIN tags t ON t.id = gt.tag_id",
    )?;
    let pairs = tag_stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut by_game: std::collections::HashMap<String, Vec<String>> = Default::default();
    for pair in pairs {
        let (game_id, tag) = pair?;
        by_game.entry(game_id).or_default().push(tag);
    }
    for g in &mut games {
        if let Some(tags) = by_game.remove(&g.id) {
            g.tags = tags;
        }
    }
    Ok(games)
}

pub fn set_favorite(conn: &Connection, game_id: &str, favorite: bool) -> Result<()> {
    conn.execute(
        "UPDATE games SET favorite = ?1 WHERE id = ?2",
        params![favorite as i64, game_id],
    )?;
    Ok(())
}

pub fn set_artwork(
    conn: &Connection,
    game_id: &str,
    cover: Option<&str>,
    hero: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE games SET cover_url = ?1, hero_url = ?2 WHERE id = ?3",
        params![cover, hero, game_id],
    )?;
    Ok(())
}

/// Set or clear the manual artwork override.
///
/// `None` clears that slot, falling back to whatever IGDB resolved. Values may
/// be an `http(s)` URL or a local file path.
pub fn set_art_override(
    conn: &Connection,
    game_id: &str,
    cover: Option<&str>,
    hero: Option<&str>,
) -> Result<()> {
    // A named fn rather than a closure: the closure's inferred lifetime cannot
    // be tied to the caller's `&str` arguments.
    fn clean(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }
    conn.execute(
        "UPDATE games SET cover_override = ?1, hero_override = ?2 WHERE id = ?3",
        params![clean(cover), clean(hero), game_id],
    )?;
    Ok(())
}

/// The raw override values, for showing what is currently set by hand.
pub fn art_override(conn: &Connection, game_id: &str) -> Result<(Option<String>, Option<String>)> {
    Ok(conn
        .query_row(
            "SELECT cover_override, hero_override FROM games WHERE id = ?1",
            params![game_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((None, None)))
}

/// The executable the user chose instead of the auto-detected one.
pub fn launch_override(conn: &Connection, game_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT launch_override FROM games WHERE id = ?1",
            params![game_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// Override which executable a game launches. `None` restores auto-detection.
pub fn set_launch_override(conn: &Connection, game_id: &str, exe: Option<&str>) -> Result<()> {
    fn clean(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }
    conn.execute(
        "UPDATE games SET launch_override = ?1 WHERE id = ?2",
        params![clean(exe), game_id],
    )?;
    Ok(())
}

/// Repoint a game at a different file, e.g. after format conversion.
/// Everything else about the row — playtime, tags, art, favourite — is kept.
pub fn set_game_path(conn: &Connection, game_id: &str, path: &str) -> Result<()> {
    conn.execute(
        "UPDATE games SET path = ?1 WHERE id = ?2",
        params![path, game_id],
    )?;
    Ok(())
}

/// Replace a game's title, e.g. after IGDB resolves the canonical name.
pub fn set_title(conn: &Connection, game_id: &str, title: &str) -> Result<()> {
    conn.execute(
        "UPDATE games SET title = ?1 WHERE id = ?2",
        params![title, game_id],
    )?;
    Ok(())
}

pub fn game_by_id(conn: &Connection, game_id: &str) -> Result<Option<Game>> {
    Ok(list_games(conn)?.into_iter().find(|g| g.id == game_id))
}

/// Open a play session; returns its row id.
pub fn begin_session(conn: &Connection, game_id: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO play_sessions (game_id, started_at) VALUES (?1, ?2)",
        params![game_id, now_iso()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Close a session and fold its duration into the game's aggregate playtime.
pub fn end_session(conn: &Connection, session_id: i64, seconds: i64) -> Result<()> {
    let ended = now_iso();
    conn.execute(
        "UPDATE play_sessions SET ended_at = ?1, duration_seconds = ?2 WHERE id = ?3",
        params![ended, seconds, session_id],
    )?;
    conn.execute(
        "UPDATE games
         SET playtime_seconds = playtime_seconds + ?1, last_played_at = ?2
         WHERE id = (SELECT game_id FROM play_sessions WHERE id = ?3)",
        params![seconds, ended, session_id],
    )?;
    Ok(())
}

// --- tags -------------------------------------------------------------------

/// Attach a tag to a game, creating the tag if it is new.
///
/// Names are trimmed and compared case-insensitively so "Co-op" and "co-op"
/// cannot both exist — a tag list that quietly splits in two is worse than
/// useless for organising a library.
pub fn add_tag(conn: &Connection, game_id: &str, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    // Reuse an existing tag whose name differs only by case.
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
            params![name],
            |r| r.get(0),
        )
        .optional()?;

    let tag_id = match existing {
        Some(id) => id,
        None => {
            conn.execute("INSERT INTO tags (name) VALUES (?1)", params![name])?;
            conn.last_insert_rowid()
        }
    };
    conn.execute(
        "INSERT OR IGNORE INTO game_tags (game_id, tag_id) VALUES (?1, ?2)",
        params![game_id, tag_id],
    )?;
    Ok(())
}

pub fn remove_tag(conn: &Connection, game_id: &str, name: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM game_tags
         WHERE game_id = ?1
           AND tag_id = (SELECT id FROM tags WHERE name = ?2 COLLATE NOCASE)",
        params![game_id, name.trim()],
    )?;
    prune_orphan_tags(conn)
}

/// Rename a tag everywhere it is used.
pub fn rename_tag(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let to = to.trim();
    if to.is_empty() {
        return Ok(());
    }
    // Merge if the target already exists, rather than failing the UNIQUE index.
    let target: Option<i64> = conn
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
            params![to],
            |r| r.get(0),
        )
        .optional()?;
    let source: Option<i64> = conn
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
            params![from.trim()],
            |r| r.get(0),
        )
        .optional()?;

    let Some(source) = source else { return Ok(()) };
    match target {
        Some(target) if target != source => {
            conn.execute(
                "UPDATE OR IGNORE game_tags SET tag_id = ?1 WHERE tag_id = ?2",
                params![target, source],
            )?;
            conn.execute("DELETE FROM tags WHERE id = ?1", params![source])?;
        }
        _ => {
            conn.execute("UPDATE tags SET name = ?1 WHERE id = ?2", params![to, source])?;
        }
    }
    Ok(())
}

/// Delete a tag and every assignment of it.
pub fn delete_tag(conn: &Connection, name: &str) -> Result<()> {
    conn.execute("DELETE FROM tags WHERE name = ?1 COLLATE NOCASE", params![name.trim()])?;
    Ok(())
}

/// All tags with how many games carry each.
pub fn list_tags(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(gt.game_id)
         FROM tags t LEFT JOIN game_tags gt ON gt.tag_id = t.id
         GROUP BY t.id ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Drop tags no game references any more, so the sidebar does not accumulate
/// dead entries.
fn prune_orphan_tags(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM game_tags)",
        [],
    )?;
    Ok(())
}

// --- scan roots & runner config -------------------------------------------

pub fn add_scan_root(conn: &Connection, runner: &str, path: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO scan_roots (runner, path) VALUES (?1, ?2)",
        params![runner, path],
    )?;
    Ok(())
}

pub fn list_scan_roots(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT runner, path FROM scan_roots ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn remove_scan_root(conn: &Connection, runner: &str, path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM scan_roots WHERE runner = ?1 AND path = ?2",
        params![runner, path],
    )?;
    Ok(())
}

pub fn set_runner_executable(conn: &Connection, runner: &str, exe: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO runners (id, executable) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET executable = excluded.executable",
        params![runner, exe],
    )?;
    Ok(())
}

/// App-level key/value settings (theme, addons dir, …).
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn runner_executable(conn: &Connection, runner: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT executable FROM runners WHERE id = ?1",
            params![runner],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Platform, RunnerId};

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::apply_migrations_for_test(&conn).unwrap();
        conn
    }

    fn entry(path: &str, name: &str, platform: Platform) -> ScannedEntry {
        ScannedEntry {
            path: path.into(),
            cleaned_name: name.into(),
            title_id: Some("RMGP01".into()),
            platform,
            runner: RunnerId::Dolphin,
            size_bytes: 1234,
        }
    }

    #[test]
    fn formats_unix_timestamps_as_utc_iso() {
        assert_eq!(format_unix_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn rescan_preserves_playtime_and_favorites() {
        let conn = mem();
        let e = entry(r"D:\Roms\Galaxy.iso", "Super Mario Galaxy", Platform::Wii);

        let (ins, upd) = upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();
        assert_eq!((ins, upd), (1, 0));

        // Simulate the user playing and favouriting it, plus resolved artwork.
        let id = list_games(&conn).unwrap()[0].id.clone();
        let session = begin_session(&conn, &id).unwrap();
        end_session(&conn, session, 3600).unwrap();
        set_favorite(&conn, &id, true).unwrap();
        set_artwork(&conn, &id, Some("cover.jpg"), Some("hero.jpg")).unwrap();

        // Rescan the same file.
        let (ins2, upd2) = upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();
        assert_eq!((ins2, upd2), (0, 1), "must update, not duplicate");

        let games = list_games(&conn).unwrap();
        assert_eq!(games.len(), 1, "rescan must not duplicate rows");
        let g = &games[0];
        assert_eq!(g.playtime_seconds, 3600, "playtime destroyed by rescan");
        assert!(g.favorite, "favorite destroyed by rescan");
        assert_eq!(g.cover_url.as_deref(), Some("cover.jpg"), "art destroyed");
        assert!(g.last_played_at.is_some());
    }

    #[test]
    fn rescan_refreshes_scanner_owned_fields() {
        let conn = mem();
        let mut e = entry(r"D:\Roms\x.iso", "X", Platform::Wii);
        upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();

        // Platform corrected by a disc-header read on the second pass.
        e.platform = Platform::GameCube;
        e.title_id = Some("G8ME01".into());
        upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();

        let g = &list_games(&conn).unwrap()[0];
        assert_eq!(g.platform, Platform::GameCube);
        assert_eq!(g.title_id.as_deref(), Some("G8ME01"));
    }

    #[test]
    fn rescan_never_erases_a_known_title_id() {
        let conn = mem();
        let mut e = entry(r"D:\Roms\Trap Team.wua", "Skylanders Trap Team", Platform::WiiU);
        e.title_id = Some("000500001017C600".into());
        upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();

        // A later scan taken while Cemu's title cache is unavailable resolves
        // no ID. It must not wipe the one we already have -- losing it silently
        // breaks per-game config, which is keyed on the ID.
        e.title_id = None;
        upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();

        assert_eq!(
            list_games(&conn).unwrap()[0].title_id.as_deref(),
            Some("000500001017C600"),
            "rescan erased a previously resolved title ID"
        );
    }

    #[test]
    fn rescan_still_fills_in_a_newly_resolved_title_id() {
        let conn = mem();
        let mut e = entry(r"D:\Roms\Giants.wua", "Skylanders Giants", Platform::WiiU);
        e.title_id = None;
        upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();
        assert!(list_games(&conn).unwrap()[0].title_id.is_none());

        // Once Cemu has built its cache, the ID should land.
        e.title_id = Some("000500001010D700".into());
        upsert_scanned(&conn, std::slice::from_ref(&e)).unwrap();
        assert_eq!(
            list_games(&conn).unwrap()[0].title_id.as_deref(),
            Some("000500001010D700")
        );
    }

    #[test]
    fn playtime_accumulates_across_sessions() {
        let conn = mem();
        upsert_scanned(&conn, &[entry(r"D:\a.iso", "A", Platform::Wii)]).unwrap();
        let id = list_games(&conn).unwrap()[0].id.clone();

        for secs in [600, 1200, 90] {
            let s = begin_session(&conn, &id).unwrap();
            end_session(&conn, s, secs).unwrap();
        }
        assert_eq!(list_games(&conn).unwrap()[0].playtime_seconds, 1890);
    }

    fn two_games(conn: &Connection) -> (String, String) {
        upsert_scanned(
            conn,
            &[
                entry(r"D:\a.iso", "Game A", Platform::Wii),
                entry(r"D:\b.iso", "Game B", Platform::Wii),
            ],
        )
        .unwrap();
        let g = list_games(conn).unwrap();
        (g[0].id.clone(), g[1].id.clone())
    }

    #[test]
    fn manual_art_wins_over_igdb_but_does_not_destroy_it() {
        let conn = mem();
        let (a, _) = two_games(&conn);
        set_artwork(&conn, &a, Some("igdb-cover.jpg"), Some("igdb-hero.jpg")).unwrap();

        set_art_override(&conn, &a, Some("mine.png"), None).unwrap();
        let g = list_games(&conn).unwrap();
        let g = g.iter().find(|x| x.id == a).unwrap();
        assert_eq!(g.cover_url.as_deref(), Some("mine.png"), "override should win");
        // The hero had no override, so it still comes from IGDB.
        assert_eq!(g.hero_url.as_deref(), Some("igdb-hero.jpg"));
        assert!(g.custom_art);

        // Reverting restores the IGDB art -- it was never overwritten.
        set_art_override(&conn, &a, None, None).unwrap();
        let g = list_games(&conn).unwrap();
        let g = g.iter().find(|x| x.id == a).unwrap();
        assert_eq!(g.cover_url.as_deref(), Some("igdb-cover.jpg"));
        assert!(!g.custom_art);
    }

    #[test]
    fn an_artwork_refresh_cannot_clobber_a_manual_override() {
        let conn = mem();
        let (a, _) = two_games(&conn);
        set_art_override(&conn, &a, Some("mine.png"), None).unwrap();

        // A later "Get artwork" run writes the IGDB columns.
        set_artwork(&conn, &a, Some("new-igdb.jpg"), None).unwrap();

        let g = list_games(&conn).unwrap();
        assert_eq!(
            g.iter().find(|x| x.id == a).unwrap().cover_url.as_deref(),
            Some("mine.png"),
            "refresh overwrote the user's chosen art"
        );
    }

    #[test]
    fn blank_override_values_are_treated_as_cleared() {
        let conn = mem();
        let (a, _) = two_games(&conn);
        set_artwork(&conn, &a, Some("igdb.jpg"), None).unwrap();
        set_art_override(&conn, &a, Some("   "), None).unwrap();
        let g = list_games(&conn).unwrap();
        let g = g.iter().find(|x| x.id == a).unwrap();
        assert_eq!(g.cover_url.as_deref(), Some("igdb.jpg"));
        assert!(!g.custom_art);
    }

    #[test]
    fn tags_round_trip_and_appear_on_games() {
        let conn = mem();
        let (a, _b) = two_games(&conn);

        add_tag(&conn, &a, "Co-op").unwrap();
        add_tag(&conn, &a, "Favorites").unwrap();
        let g = list_games(&conn).unwrap();
        let tags = &g.iter().find(|x| x.id == a).unwrap().tags;
        assert_eq!(tags.len(), 2, "{tags:?}");
        assert!(tags.contains(&"Co-op".to_string()));

        remove_tag(&conn, &a, "Co-op").unwrap();
        let g = list_games(&conn).unwrap();
        assert_eq!(g.iter().find(|x| x.id == a).unwrap().tags, vec!["Favorites"]);
    }

    #[test]
    fn tag_names_are_case_insensitive() {
        let conn = mem();
        let (a, b) = two_games(&conn);
        add_tag(&conn, &a, "Co-op").unwrap();
        add_tag(&conn, &b, "co-op").unwrap();

        // One tag, two games -- not two tags that look identical in the sidebar.
        let tags = list_tags(&conn).unwrap();
        assert_eq!(tags.len(), 1, "{tags:?}");
        assert_eq!(tags[0].1, 2, "count should include both games");
    }

    #[test]
    fn adding_the_same_tag_twice_is_idempotent() {
        let conn = mem();
        let (a, _) = two_games(&conn);
        add_tag(&conn, &a, "RPG").unwrap();
        add_tag(&conn, &a, "RPG").unwrap();
        assert_eq!(list_games(&conn).unwrap()[0].tags.len(), 1);
    }

    #[test]
    fn blank_tags_are_ignored() {
        let conn = mem();
        let (a, _) = two_games(&conn);
        add_tag(&conn, &a, "   ").unwrap();
        assert!(list_tags(&conn).unwrap().is_empty());
    }

    #[test]
    fn removing_the_last_use_prunes_the_tag() {
        let conn = mem();
        let (a, _) = two_games(&conn);
        add_tag(&conn, &a, "Shooter").unwrap();
        assert_eq!(list_tags(&conn).unwrap().len(), 1);
        remove_tag(&conn, &a, "Shooter").unwrap();
        assert!(list_tags(&conn).unwrap().is_empty(), "orphan tag left behind");
    }

    #[test]
    fn renaming_into_an_existing_tag_merges_them() {
        let conn = mem();
        let (a, b) = two_games(&conn);
        add_tag(&conn, &a, "Coop").unwrap();
        add_tag(&conn, &b, "Co-op").unwrap();
        assert_eq!(list_tags(&conn).unwrap().len(), 2);

        // Merging must not trip the UNIQUE index on tags.name.
        rename_tag(&conn, "Coop", "Co-op").unwrap();
        let tags = list_tags(&conn).unwrap();
        assert_eq!(tags.len(), 1, "{tags:?}");
        assert_eq!(tags[0].0, "Co-op");
        assert_eq!(tags[0].1, 2, "both games should carry the merged tag");
    }

    #[test]
    fn deleting_a_tag_removes_it_from_every_game() {
        let conn = mem();
        let (a, b) = two_games(&conn);
        add_tag(&conn, &a, "VR").unwrap();
        add_tag(&conn, &b, "VR").unwrap();
        delete_tag(&conn, "VR").unwrap();
        assert!(list_tags(&conn).unwrap().is_empty());
        assert!(list_games(&conn).unwrap().iter().all(|g| g.tags.is_empty()));
    }

    #[test]
    fn scan_roots_are_unique_per_runner_and_path() {
        let conn = mem();
        add_scan_root(&conn, "dolphin", r"D:\Wii Roms").unwrap();
        add_scan_root(&conn, "dolphin", r"D:\Wii Roms").unwrap();
        add_scan_root(&conn, "dolphin", r"D:\Gamecube Roms").unwrap();
        assert_eq!(list_scan_roots(&conn).unwrap().len(), 2);

        remove_scan_root(&conn, "dolphin", r"D:\Wii Roms").unwrap();
        assert_eq!(list_scan_roots(&conn).unwrap().len(), 1);
    }

    #[test]
    fn settings_round_trip_and_overwrite() {
        let conn = mem();
        assert_eq!(get_setting(&conn, "addons_dir").unwrap(), None);
        set_setting(&conn, "addons_dir", r"D:\Switch\updates").unwrap();
        set_setting(&conn, "addons_dir", r"D:\other").unwrap();
        assert_eq!(get_setting(&conn, "addons_dir").unwrap().as_deref(), Some(r"D:\other"));
    }

    #[test]
    fn runner_executable_round_trips_and_overwrites() {
        let conn = mem();
        assert_eq!(runner_executable(&conn, "dolphin").unwrap(), None);
        set_runner_executable(&conn, "dolphin", r"C:\a\Dolphin.exe").unwrap();
        set_runner_executable(&conn, "dolphin", r"D:\b\Dolphin.exe").unwrap();
        assert_eq!(
            runner_executable(&conn, "dolphin").unwrap().as_deref(),
            Some(r"D:\b\Dolphin.exe")
        );
    }
}
