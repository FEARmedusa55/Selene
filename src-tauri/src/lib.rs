//! Phase 1 core: storage, scanning, metadata, launching and per-game config.

pub mod addons;
pub mod config;
pub mod convert;
pub mod db;
pub mod goldberg;
pub mod launch;
pub mod metadata;
pub mod model;
pub mod pretendo;
pub mod runners;
pub mod scan;
pub mod store;
pub mod tray;

use metadata::igdb::IgdbClient;
use model::Game;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Emitter;

/// Shared app state. The connection is behind a mutex because rusqlite's
/// `Connection` is not `Sync`; long operations take it only for write bursts.
pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub config_dir: PathBuf,
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerInfo {
    id: &'static str,
    name: &'static str,
    platforms: Vec<&'static str>,
    extensions: Vec<&'static str>,
    available_on_this_os: bool,
    executable_detected: Option<String>,
    executable_configured: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    found: usize,
    inserted: usize,
    updated: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkReport {
    resolved: usize,
    unmatched: Vec<String>,
}

#[tauri::command]
fn list_runners(state: tauri::State<'_, AppState>) -> CmdResult<Vec<RunnerInfo>> {
    let conn = state.conn.lock().map_err(err)?;
    Ok(runners::all()
        .iter()
        .map(|r| RunnerInfo {
            id: r.id().as_str(),
            name: r.display_name(),
            platforms: r.platforms().iter().map(|p| p.as_str()).collect(),
            extensions: r.extensions().to_vec(),
            available_on_this_os: r.available_on_this_os(),
            executable_detected: r.detect_executable().map(|p| p.to_string_lossy().into_owned()),
            executable_configured: store::runner_executable(&conn, r.id().as_str())
                .ok()
                .flatten(),
        })
        .collect())
}

#[tauri::command]
fn list_games(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Game>> {
    let conn = state.conn.lock().map_err(err)?;
    store::list_games(&conn).map_err(err)
}

#[tauri::command]
fn list_scan_roots(state: tauri::State<'_, AppState>) -> CmdResult<Vec<(String, String)>> {
    let conn = state.conn.lock().map_err(err)?;
    store::list_scan_roots(&conn).map_err(err)
}

#[tauri::command]
fn add_scan_root(state: tauri::State<'_, AppState>, runner: String, path: String) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::add_scan_root(&conn, &runner, &path).map_err(err)
}

#[tauri::command]
fn remove_scan_root(
    state: tauri::State<'_, AppState>,
    runner: String,
    path: String,
) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::remove_scan_root(&conn, &runner, &path).map_err(err)
}

#[tauri::command]
fn set_runner_executable(
    state: tauri::State<'_, AppState>,
    runner: String,
    path: String,
) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::set_runner_executable(&conn, &runner, &path).map_err(err)
}

/// Walk every configured root and upsert what is found.
#[tauri::command]
fn scan_library(state: tauri::State<'_, AppState>) -> CmdResult<ScanReport> {
    let conn = state.conn.lock().map_err(err)?;
    let roots = store::list_scan_roots(&conn).map_err(err)?;
    let cemu_exe = store::runner_executable(&conn, "cemu").map_err(err)?;
    // Disc-header reads touch the filesystem, so release the lock while walking.
    drop(conn);

    let mut entries = scan::scan_all(&roots);

    // `.wua` archives carry no title ID in their filename; Cemu's own cache is
    // the only practical source, and it also supplies proper game names.
    if let Some(exe) = cemu_exe {
        if let Some(data_dir) = runners::wiiu::cemu_data_dir(std::path::Path::new(&exe)) {
            scan::enrich_wiiu(&mut entries, &data_dir);
        }
    }

    let conn = state.conn.lock().map_err(err)?;
    let (inserted, updated) = store::upsert_scanned(&conn, &entries).map_err(err)?;
    Ok(ScanReport {
        found: entries.len(),
        inserted,
        updated,
    })
}

#[tauri::command]
fn set_favorite(state: tauri::State<'_, AppState>, game_id: String, favorite: bool) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::set_favorite(&conn, &game_id, favorite).map_err(err)
}

/// A user-authored theme loaded from `<config>/themes/*.css`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTheme {
    /// Derived from the filename; also the `[data-theme="..."]` selector value.
    id: String,
    name: String,
    /// The raw CSS, injected by the frontend.
    css: String,
}

/// List user themes. A theme is one `.css` file declaring tokens under
/// `[data-theme="<filename>"]` — no code, exactly like the built-ins.
#[tauri::command]
fn list_user_themes(state: tauri::State<'_, AppState>) -> CmdResult<Vec<UserTheme>> {
    let dir = state.config_dir.join("themes");
    if !dir.is_dir() {
        // Create it so the user has somewhere obvious to drop theme files.
        let _ = std::fs::create_dir_all(&dir);
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(err)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("css") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let css = std::fs::read_to_string(&path).map_err(err)?;
        // Optional friendly name from a leading `/* name: ... */` comment.
        let name = css
            .lines()
            .take(5)
            .find_map(|l| l.split_once("name:").map(|(_, v)| v.trim_end_matches("*/").trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| prettify_id(&id));
        out.push(UserTheme { id, name, css });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Open the user-themes folder in the file manager.
#[tauri::command]
fn open_themes_dir(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let dir = state.config_dir.join("themes");
    std::fs::create_dir_all(&dir).map_err(err)?;
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(err)
}

fn prettify_id(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Persist a boolean-ish app preference.
#[tauri::command]
fn set_preference(state: tauri::State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    // Only allow a known allowlist of preference keys.
    const ALLOWED: &[&str] = &["minimize_on_play"];
    if !ALLOWED.contains(&key.as_str()) {
        return Err(format!("unknown preference: {key}"));
    }
    let conn = state.conn.lock().map_err(err)?;
    store::set_setting(&conn, &key, &value).map_err(err)
}

#[tauri::command]
fn get_preference(state: tauri::State<'_, AppState>, key: String) -> CmdResult<Option<String>> {
    let conn = state.conn.lock().map_err(err)?;
    store::get_setting(&conn, &key).map_err(err)
}

#[tauri::command]
fn igdb_configured(state: tauri::State<'_, AppState>) -> bool {
    metadata::load_credentials(&state.config_dir)
        .ok()
        .flatten()
        .is_some()
}

#[tauri::command]
fn set_igdb_credentials(
    state: tauri::State<'_, AppState>,
    client_id: String,
    client_secret: String,
) -> CmdResult<()> {
    metadata::save_credentials(
        &state.config_dir,
        &metadata::igdb::Credentials {
            client_id,
            client_secret,
        },
    )
    .map_err(err)
}

/// Resolve artwork for every game lacking a cover.
///
/// Titles that do not clear the confidence floor are returned as `unmatched`
/// rather than given plausible-but-wrong art.
#[tauri::command]
async fn fetch_artwork(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<ArtworkReport> {
    let creds = metadata::load_credentials(&state.config_dir)
        .map_err(err)?
        .ok_or("IGDB credentials are not configured")?;

    let pending: Vec<Game> = {
        let conn = state.conn.lock().map_err(err)?;
        store::list_games(&conn)
            .map_err(err)?
            .into_iter()
            .filter(|g| g.cover_url.is_none())
            .collect()
    };

    let mut client = IgdbClient::new(creds, None);
    let mut resolved = 0;
    let mut unmatched = Vec::new();

    for game in pending {
        let art = client
            .find_artwork(&game.title, game.platform)
            .await
            .map_err(err)?;

        match art {
            Some(a) => {
                let conn = state.conn.lock().map_err(err)?;
                store::set_artwork(&conn, &game.id, a.cover_url.as_deref(), a.hero_url.as_deref())
                    .map_err(err)?;
                // Adopt IGDB's canonical name; the scanned filename is messier.
                store::set_title(&conn, &game.id, &a.matched_name).map_err(err)?;
                resolved += 1;
            }
            None => unmatched.push(game.title.clone()),
        }
        // Let the UI show progress rather than freezing until every title is done.
        let _ = app.emit("artwork-progress", resolved);
    }

    Ok(ArtworkReport { resolved, unmatched })
}

#[tauri::command]
fn list_tags(state: tauri::State<'_, AppState>) -> CmdResult<Vec<(String, i64)>> {
    let conn = state.conn.lock().map_err(err)?;
    store::list_tags(&conn).map_err(err)
}

#[tauri::command]
fn add_tag(state: tauri::State<'_, AppState>, game_id: String, name: String) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::add_tag(&conn, &game_id, &name).map_err(err)
}

#[tauri::command]
fn remove_tag(state: tauri::State<'_, AppState>, game_id: String, name: String) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::remove_tag(&conn, &game_id, &name).map_err(err)
}

#[tauri::command]
fn rename_tag(state: tauri::State<'_, AppState>, from: String, to: String) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::rename_tag(&conn, &from, &to).map_err(err)
}

#[tauri::command]
fn delete_tag(state: tauri::State<'_, AppState>, name: String) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    store::delete_tag(&conn, &name).map_err(err)
}

/// Current manual artwork override, if any.
#[tauri::command]
fn get_art_override(
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<(Option<String>, Option<String>)> {
    let conn = state.conn.lock().map_err(err)?;
    store::art_override(&conn, &game_id).map_err(err)
}

/// Set artwork by hand. Values may be an http(s) URL or a local file path;
/// empty strings clear the slot and fall back to IGDB.
#[tauri::command]
fn set_art_override(
    state: tauri::State<'_, AppState>,
    game_id: String,
    cover: Option<String>,
    hero: Option<String>,
) -> CmdResult<()> {
    // A local path must exist, or the game silently renders a broken image.
    for v in [cover.as_deref(), hero.as_deref()].into_iter().flatten() {
        let v = v.trim();
        if v.is_empty() || v.starts_with("http://") || v.starts_with("https://") {
            continue;
        }
        if !std::path::Path::new(v).is_file() {
            return Err(format!("No such file: {v}"));
        }
    }
    let conn = state.conn.lock().map_err(err)?;
    store::set_art_override(&conn, &game_id, cover.as_deref(), hero.as_deref()).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExeCandidate {
    path: String,
    file_name: String,
    score: f64,
    reasons: Vec<String>,
    size_mb: f64,
}

/// Executables found in a PC game's folder, best guess first.
///
/// Exposed so the user can correct the choice: the heuristic is good but the
/// folder layouts it faces are not standardised.
/// Resolve the add-ons (updates/DLC) directory: the configured one, or a
/// sibling `updates` folder auto-detected next to an Eden scan root.
fn addons_dir(conn: &rusqlite::Connection) -> Option<PathBuf> {
    if let Ok(Some(dir)) = store::get_setting(conn, "addons_dir") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    let roots = store::list_scan_roots(conn).ok()?;
    roots
        .iter()
        .filter(|(runner, _)| runner == "eden")
        .find_map(|(_, path)| addons::detect_sibling(std::path::Path::new(path)))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonsView {
    dir: Option<String>,
    addons: Vec<addons::Addon>,
    /// How many still need decompression before they can be installed.
    needs_conversion: usize,
    converter_available: bool,
}

/// Updates and DLC available for a game, matched by title family.
#[tauri::command]
fn list_addons(state: tauri::State<'_, AppState>, game_id: String) -> CmdResult<AddonsView> {
    let conn = state.conn.lock().map_err(err)?;
    let game = store::game_by_id(&conn, &game_id)
        .map_err(err)?
        .ok_or("unknown game")?;
    let dir = addons_dir(&conn);
    drop(conn);

    let addons = match (&dir, &game.title_id) {
        (Some(d), Some(id)) => addons::for_game(d, id),
        _ => Vec::new(),
    };
    Ok(AddonsView {
        needs_conversion: addons.iter().filter(|a| a.needs_conversion).count(),
        converter_available: convert::detect_nsz().is_some(),
        dir: dir.map(|d| d.to_string_lossy().into_owned()),
        addons,
    })
}

#[tauri::command]
fn set_addons_dir(state: tauri::State<'_, AppState>, path: String) -> CmdResult<()> {
    let p = path.trim();
    if !p.is_empty() && !std::path::Path::new(p).is_dir() {
        return Err(format!("Not a folder: {p}"));
    }
    let conn = state.conn.lock().map_err(err)?;
    store::set_setting(&conn, "addons_dir", p).map_err(err)
}

/// Convert every compressed add-on in the folder to an installable format.
///
/// Emits `addon-convert-progress` as each finishes so the UI can track a long
/// batch. Originals are kept; failures are collected rather than aborting the
/// whole run.
#[tauri::command]
async fn convert_all_addons(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<String> {
    let (dir, eden_keys) = {
        let conn = state.conn.lock().map_err(err)?;
        let dir = addons_dir(&conn).ok_or(
            "No updates folder found. Set it in Settings, or place an 'updates' folder \
             next to your Switch games folder.",
        )?;
        let keys = store::runner_executable(&conn, "eden")
            .map_err(err)?
            .and_then(|exe| runners::eden::data_dir(std::path::Path::new(&exe)))
            .map(|d| d.join("keys"));
        (dir, keys)
    };

    let nsz = convert::detect_nsz().ok_or("No converter found. Install it with: pip install nsz")?;
    if let Some(keys) = eden_keys {
        convert::ensure_keys(&keys).map_err(err)?;
    }

    let pending: Vec<PathBuf> = addons::scan(&dir)
        .into_iter()
        .filter(|a| a.needs_conversion)
        .map(|a| PathBuf::from(a.path))
        .collect();

    let total = pending.len();
    if total == 0 {
        return Ok("Nothing to convert — every add-on is already installable.".into());
    }

    let mut done = 0;
    let mut failed: Vec<String> = Vec::new();
    for input in pending {
        let nsz = nsz.clone();
        let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let _ = app.emit("addon-convert-progress", (done, total, &name));

        let res = tauri::async_runtime::spawn_blocking(move || convert::convert(&nsz, &input))
            .await
            .map_err(err)?;
        match res {
            Ok(_) => done += 1,
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    let _ = app.emit("addon-convert-progress", (total, total, ""));

    if failed.is_empty() {
        Ok(format!("Converted {done} add-on(s). Now install them in Eden (see below)."))
    } else {
        Ok(format!(
            "Converted {done} of {total}; {} failed:\n{}",
            failed.len(),
            failed.join("\n")
        ))
    }
}

/// Open the add-ons folder in the system file manager, for the manual
/// Install-to-NAND step in Eden.
#[tauri::command]
fn open_addons_dir(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    let dir = addons_dir(&conn).ok_or("No updates folder configured")?;
    drop(conn);
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertInfo {
    /// This game's file is a format its runner cannot read, but is convertible.
    needs_conversion: bool,
    from_ext: Option<String>,
    to_ext: Option<String>,
    /// A converter (`nsz`) was found on the machine.
    converter_available: bool,
    converter_path: Option<String>,
}

/// Whether a game needs format conversion, and whether we can do it.
#[tauri::command]
fn convert_info(state: tauri::State<'_, AppState>, game_id: String) -> CmdResult<ConvertInfo> {
    let conn = state.conn.lock().map_err(err)?;
    let game = store::game_by_id(&conn, &game_id)
        .map_err(err)?
        .ok_or("unknown game")?;
    drop(conn);

    let conv = convert::convertible(std::path::Path::new(&game.path));
    let detected = convert::detect_nsz();
    Ok(ConvertInfo {
        needs_conversion: conv.is_some(),
        from_ext: conv.map(|c| c.from_ext.to_string()),
        to_ext: conv.map(|c| c.to_ext.to_string()),
        converter_available: detected.is_some(),
        converter_path: detected.map(|p| p.to_string_lossy().into_owned()),
    })
}

/// Convert a game to a runnable format, keeping the original.
///
/// On success the game row is repointed at the new file, preserving its
/// playtime, tags, favourite state and artwork. The original is left on disk.
#[tauri::command]
async fn convert_game(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<String> {
    let (input, eden_keys) = {
        let conn = state.conn.lock().map_err(err)?;
        let game = store::game_by_id(&conn, &game_id)
            .map_err(err)?
            .ok_or("unknown game")?;
        if convert::convertible(std::path::Path::new(&game.path)).is_none() {
            return Err("this game does not need conversion".into());
        }
        // Keys come from the runner's own key directory (Eden's).
        let eden_keys = store::runner_executable(&conn, "eden")
            .map_err(err)?
            .and_then(|exe| runners::eden::data_dir(std::path::Path::new(&exe)))
            .map(|d| d.join("keys"));
        (game.path.clone(), eden_keys)
    };

    let nsz = convert::detect_nsz().ok_or(
        "No converter found. Install it with:  pip install nsz",
    )?;
    if let Some(keys) = eden_keys {
        convert::ensure_keys(&keys).map_err(err)?;
    }

    let _ = app.emit("convert-started", &game_id);

    let input_path = std::path::PathBuf::from(input);
    let produced = tauri::async_runtime::spawn_blocking(move || convert::convert(&nsz, &input_path))
        .await
        .map_err(err)?
        .map_err(err)?;

    // Repoint the game at the runnable file; everything else about the row is
    // preserved.
    let produced_str = produced.to_string_lossy().into_owned();
    {
        let conn = state.conn.lock().map_err(err)?;
        store::set_game_path(&conn, &game_id, &produced_str).map_err(err)?;
    }
    let _ = app.emit("convert-finished", &game_id);
    Ok(produced_str)
}

#[tauri::command]
fn list_executables(
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<Vec<ExeCandidate>> {
    let conn = state.conn.lock().map_err(err)?;
    let game = store::game_by_id(&conn, &game_id)
        .map_err(err)?
        .ok_or("unknown game")?;
    drop(conn);

    Ok(runners::exe_pick::rank_executables(std::path::Path::new(&game.path))
        .into_iter()
        .map(|c| ExeCandidate {
            file_name: c
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size_mb: c
                .path
                .metadata()
                .map(|m| m.len() as f64 / 1_048_576.0)
                .unwrap_or(0.0),
            path: c.path.to_string_lossy().into_owned(),
            score: c.score,
            reasons: c.reasons,
        })
        .collect())
}

#[tauri::command]
fn get_launch_override(
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<Option<String>> {
    let conn = state.conn.lock().map_err(err)?;
    store::launch_override(&conn, &game_id).map_err(err)
}

#[tauri::command]
fn set_launch_override(
    state: tauri::State<'_, AppState>,
    game_id: String,
    exe: Option<String>,
) -> CmdResult<()> {
    if let Some(p) = exe.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !std::path::Path::new(p).is_file() {
            return Err(format!("No such file: {p}"));
        }
    }
    let conn = state.conn.lock().map_err(err)?;
    store::set_launch_override(&conn, &game_id, exe.as_deref()).map_err(err)
}

/// Cemu's data directory, or `None` when Cemu is not configured.
fn cemu_dir(conn: &rusqlite::Connection) -> Option<PathBuf> {
    let exe = store::runner_executable(conn, "cemu").ok().flatten()?;
    runners::wiiu::cemu_data_dir(std::path::Path::new(&exe))
}

#[tauri::command]
fn pretendo_status(state: tauri::State<'_, AppState>) -> CmdResult<pretendo::Status> {
    let conn = state.conn.lock().map_err(err)?;
    let dir = cemu_dir(&conn);
    drop(conn);
    Ok(pretendo::status(dir.as_deref()))
}

/// Switch Cemu between Nintendo Network and Pretendo.
#[tauri::command]
fn set_pretendo_service(
    state: tauri::State<'_, AppState>,
    service: pretendo::NetworkService,
    online: bool,
) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    let dir = cemu_dir(&conn).ok_or("Cemu is not configured")?;
    drop(conn);
    pretendo::set_service(&dir, service, online).map_err(err)
}

/// Set an account's PNID username. The password is handled by Cemu itself.
#[tauri::command]
fn set_pretendo_account_id(
    state: tauri::State<'_, AppState>,
    persistent_id: String,
    pnid: String,
) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    let dir = cemu_dir(&conn).ok_or("Cemu is not configured")?;
    drop(conn);
    pretendo::set_account_id(&dir, &persistent_id, &pnid).map_err(err)
}

// --- Online: desktop OAuth loopback ----------------------------------------

/// Catch the OAuth redirect for the sign-in flow.
///
/// Desktop apps can't take a web redirect the way a site does, so we open the
/// provider in the system browser and point its `redirect_to` at
/// `http://localhost:<port>/callback`. This binds that port, waits for the one
/// request carrying `?code=...`, answers with a friendly "you can close this"
/// page, and hands the code back to the frontend to exchange for a session.
#[tauri::command]
async fn oauth_capture(port: u16) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || capture_oauth_code(port))
        .await
        .map_err(err)?
}

fn capture_oauth_code(port: u16) -> CmdResult<String> {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|e| format!("could not bind localhost:{port}: {e}"))?;
    listener.set_nonblocking(true).ok();

    let deadline = Instant::now() + Duration::from_secs(180);
    let page = "<!doctype html><meta charset=utf-8><title>Selene</title>\
        <body style=\"font-family:system-ui;background:#15102a;color:#fff;text-align:center;padding-top:15vh\">\
        <h2>Signed in to Selene</h2><p>You can close this tab and return to the app.</p>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        page.len(),
        page
    );

    loop {
        if Instant::now() > deadline {
            return Err("Timed out waiting for sign-in.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                let code = path
                    .split_once('?')
                    .and_then(|(_, qs)| qs.split('&').find_map(|kv| kv.strip_prefix("code=")))
                    .map(|c| c.to_string());
                let _ = stream.write_all(response.as_bytes());
                // Browsers may preconnect or fetch a favicon; keep listening until
                // the request that actually carries the auth code arrives.
                if let Some(code) = code {
                    return Ok(code);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

// --- Goldberg (Steam API emulation for PC games) ---------------------------

/// The user-supplied Goldberg release folder, if one is configured and present.
fn goldberg_dir(conn: &rusqlite::Connection) -> Option<PathBuf> {
    let raw = store::get_setting(conn, "goldberg_dir").ok().flatten()?;
    let p = PathBuf::from(raw);
    p.is_dir().then_some(p)
}

#[tauri::command]
fn get_goldberg_dir(state: tauri::State<'_, AppState>) -> CmdResult<Option<String>> {
    let conn = state.conn.lock().map_err(err)?;
    store::get_setting(&conn, "goldberg_dir").map_err(err)
}

/// Point Selene at an extracted Goldberg release. Empty clears it.
#[tauri::command]
fn set_goldberg_dir(state: tauri::State<'_, AppState>, path: String) -> CmdResult<()> {
    let p = path.trim();
    if !p.is_empty() && !std::path::Path::new(p).is_dir() {
        return Err(format!("Not a folder: {p}"));
    }
    let conn = state.conn.lock().map_err(err)?;
    store::set_setting(&conn, "goldberg_dir", p).map_err(err)
}

/// Goldberg state for a game: which Steam DLLs it has, whether the emulator is
/// installed, the AppID, and any reason Apply is not yet available.
#[tauri::command]
fn goldberg_status(
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<goldberg::Status> {
    let (game, gb_dir) = {
        let conn = state.conn.lock().map_err(err)?;
        let game = store::game_by_id(&conn, &game_id)
            .map_err(err)?
            .ok_or("unknown game")?;
        (game, goldberg_dir(&conn))
    };
    // Only PC games have a Steam API DLL to emulate.
    if game.runner != model::RunnerId::NativePc {
        return Ok(goldberg::status(std::path::Path::new(""), None));
    }
    Ok(goldberg::status(
        std::path::Path::new(&game.path),
        gb_dir.as_deref(),
    ))
}

/// Install Goldberg into a game folder, backing up the originals first.
#[tauri::command]
fn goldberg_apply(
    state: tauri::State<'_, AppState>,
    game_id: String,
    app_id: String,
) -> CmdResult<()> {
    let (game, gb_dir) = {
        let conn = state.conn.lock().map_err(err)?;
        let game = store::game_by_id(&conn, &game_id)
            .map_err(err)?
            .ok_or("unknown game")?;
        let dir = goldberg_dir(&conn)
            .ok_or("No Goldberg emulator folder is set. Add it in Settings first.")?;
        (game, dir)
    };
    if game.runner != model::RunnerId::NativePc {
        return Err("Goldberg only applies to PC games.".into());
    }
    let release = goldberg::resolve_release(&gb_dir);
    goldberg::apply(std::path::Path::new(&game.path), &release, &app_id).map_err(err)
}

/// The global Goldberg player name — how you appear to friends on LAN.
#[tauri::command]
fn get_goldberg_account_name() -> CmdResult<Option<String>> {
    Ok(goldberg::account_name())
}

/// Set (empty clears) the global Goldberg player name.
#[tauri::command]
fn set_goldberg_account_name(name: String) -> CmdResult<()> {
    goldberg::set_account_name(&name).map_err(err)
}

/// Revert a Selene-applied Goldberg setup, restoring the original DLLs.
#[tauri::command]
fn goldberg_remove(state: tauri::State<'_, AppState>, game_id: String) -> CmdResult<()> {
    let game = {
        let conn = state.conn.lock().map_err(err)?;
        store::game_by_id(&conn, &game_id)
            .map_err(err)?
            .ok_or("unknown game")?
    };
    goldberg::remove(std::path::Path::new(&game.path)).map_err(err)
}

#[tauri::command]
fn get_game_config(state: tauri::State<'_, AppState>, game_id: String) -> CmdResult<String> {
    let conn = state.conn.lock().map_err(err)?;
    config::load_overrides(&conn, &game_id).map_err(err)
}

/// Save per-game overrides and write them into Dolphin's own GameSettings INI.
#[tauri::command]
fn set_game_config(
    state: tauri::State<'_, AppState>,
    game_id: String,
    overrides: String,
) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;

    // Read what we saved last time *before* overwriting it. Cemu needs it to
    // tell "the user cleared this override" apart from "we never managed this
    // key" -- without it, clearing a setting either leaves a stale line behind
    // or deletes values the emulator itself wrote.
    let previous_json = config::load_overrides(&conn, &game_id).map_err(err)?;
    config::save_overrides(&conn, &game_id, &overrides).map_err(err)?;

    let game = store::game_by_id(&conn, &game_id)
        .map_err(err)?
        .ok_or("unknown game")?;
    let Some(title_id) = game.title_id.as_deref() else {
        // Without a title ID there is no filename to key the per-game file on.
        // The override stays in the database and will apply once the ID is
        // known, but this must be reported as a failure: silently returning Ok
        // made the UI claim "Saved" while nothing reached the emulator.
        return Err(format!(
            "No title ID resolved for \"{}\", so {} has no per-game file to write. \
             For .wua archives the ID comes from Cemu's title cache — open Cemu \
             once so it scans the folder, then press Scan here.",
            game.title,
            game.runner.as_str()
        ));
    };

    let exe = store::runner_executable(&conn, game.runner.as_str())
        .map_err(err)?
        .ok_or_else(|| format!("no executable configured for {}", game.runner.as_str()))?;
    let exe = std::path::Path::new(&exe);

    // Each emulator's own per-game mechanism; none of these touch global config.
    match game.runner {
        model::RunnerId::Dolphin => {
            let dir = config::dolphin::user_dir(exe)
                .ok_or("could not locate Dolphin's user directory")?;
            let cfg = serde_json::from_str(&overrides).map_err(err)?;
            config::dolphin::apply(&dir, title_id, &cfg).map_err(err)
        }
        model::RunnerId::Cemu => {
            let dir = runners::wiiu::cemu_data_dir(exe)
                .ok_or("could not locate Cemu's data directory")?;
            let cfg = serde_json::from_str(&overrides).map_err(err)?;
            let previous = serde_json::from_str(&previous_json).unwrap_or_default();
            config::cemu::apply(&dir, title_id, &previous, &cfg).map_err(err)
        }
        model::RunnerId::Eden => {
            let dir = runners::eden::data_dir(exe)
                .ok_or("could not locate Eden's data directory")?;
            let cfg = serde_json::from_str(&overrides).map_err(err)?;
            config::eden::apply(&dir, title_id, &cfg).map_err(err)
        }
        model::RunnerId::NativePc => Ok(()),
    }
}

/// Whether Eden has the user-supplied keys and firmware it needs.
/// Reported plainly so a missing dump is visible before a game fails to boot.
#[tauri::command]
fn eden_requirements(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<runners::eden::Requirements>> {
    let conn = state.conn.lock().map_err(err)?;
    let Some(exe) = store::runner_executable(&conn, "eden").map_err(err)? else {
        return Ok(None);
    };
    Ok(runners::eden::data_dir(std::path::Path::new(&exe))
        .map(|d| runners::eden::check_requirements(&d)))
}

/// Graphic packs applying to a Wii U title.
///
/// Cemu models resolution scaling as a pack with named presets rather than a
/// setting, so the UI lists what actually exists instead of a fake dropdown.
#[tauri::command]
fn cemu_graphic_packs(
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<Vec<config::cemu::graphic_packs::GraphicPack>> {
    let conn = state.conn.lock().map_err(err)?;
    let game = store::game_by_id(&conn, &game_id)
        .map_err(err)?
        .ok_or("unknown game")?;
    let Some(title_id) = game.title_id.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(exe) = store::runner_executable(&conn, "cemu").map_err(err)? else {
        return Ok(Vec::new());
    };
    let Some(dir) = runners::wiiu::cemu_data_dir(std::path::Path::new(&exe)) else {
        return Ok(Vec::new());
    };
    Ok(config::cemu::graphic_packs::for_title(&dir, title_id))
}

/// Enable or disable a graphic pack and set its preset selections.
#[tauri::command]
fn set_cemu_graphic_pack(
    state: tauri::State<'_, AppState>,
    rules_path: String,
    enabled: bool,
    presets: Vec<(String, String)>,
) -> CmdResult<()> {
    let conn = state.conn.lock().map_err(err)?;
    let exe = store::runner_executable(&conn, "cemu")
        .map_err(err)?
        .ok_or("Cemu executable is not configured")?;
    let dir = runners::wiiu::cemu_data_dir(std::path::Path::new(&exe))
        .ok_or("could not locate Cemu's data directory")?;
    config::cemu::graphic_packs::set_enabled(&dir, &rules_path, enabled, &presets).map_err(err)
}

/// Launch a game and record the session. Blocks on a worker thread until the
/// whole process tree exits, then folds the duration into playtime.
#[tauri::command]
async fn play_game(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    game_id: String,
) -> CmdResult<i64> {
    let (plan, session) = {
        let conn = state.conn.lock().map_err(err)?;
        let game = store::game_by_id(&conn, &game_id)
            .map_err(err)?
            .ok_or("unknown game")?;

        let runner = runners::all()
            .into_iter()
            .find(|r| r.id() == game.runner)
            .ok_or("no such runner")?;

        if !runner.available_on_this_os() {
            return Err(format!(
                "{} is not available on this platform",
                runner.display_name()
            ));
        }

        // PC games are their own program; there is no emulator to configure.
        let exe = if game.runner == model::RunnerId::NativePc {
            String::new()
        } else {
            store::runner_executable(&conn, game.runner.as_str())
                .map_err(err)?
                .ok_or_else(|| format!("no executable configured for {}", game.runner.as_str()))?
        };

        // A file the runner cannot read yet: fail with an explanation rather
        // than handing the emulator something it errors on opaquely.
        if let Some(c) = convert::convertible(std::path::Path::new(&game.path)) {
            return Err(format!(
                "{} files are not playable in {} — convert this game to .{} first \
                 (there is a Convert button on its page).",
                c.from_ext.to_uppercase(),
                runner.display_name(),
                c.to_ext
            ));
        }

        // A user-chosen executable overrides auto-detection.
        let target = store::launch_override(&conn, &game_id)
            .map_err(err)?
            .unwrap_or_else(|| game.path.clone());

        let plan = runner
            .build_launch_plan(std::path::Path::new(&exe), std::path::Path::new(&target))
            .map_err(err)?;

        let session = store::begin_session(&conn, &game_id).map_err(err)?;
        (plan, session)
    };

    // Now-playing: tooltip + optional minimise while the game runs.
    let (title, minimize) = {
        let conn = state.conn.lock().map_err(err)?;
        let title = store::game_by_id(&conn, &game_id)
            .ok()
            .flatten()
            .map(|g| g.title)
            .unwrap_or_default();
        // Default on; a stored "false" turns it off.
        let minimize = store::get_setting(&conn, "minimize_on_play")
            .ok()
            .flatten()
            .map(|v| v != "false")
            .unwrap_or(true);
        (title, minimize)
    };
    tray::start_playing(&app, &title, minimize);
    let _ = app.emit("game-started", &game_id);

    // The wait is blocking, so it must not run on the async runtime's thread.
    let outcome = tauri::async_runtime::spawn_blocking(move || launch::run_and_wait(&plan)).await;

    // Restore the window whatever happened, before surfacing any error.
    tray::stop_playing(&app, minimize);
    let _ = app.emit("game-stopped", &game_id);

    let outcome = outcome.map_err(err)?.map_err(err)?;
    let conn = state.conn.lock().map_err(err)?;
    store::end_session(&conn, session, outcome.seconds).map_err(err)?;

    Ok(outcome.seconds)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_dir = db::config_dir().expect("resolving config directory");
    let conn = db::open(&config_dir).expect("opening library database");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            conn: Mutex::new(conn),
            config_dir,
        })
        .invoke_handler(tauri::generate_handler![
            list_runners,
            list_games,
            list_scan_roots,
            add_scan_root,
            remove_scan_root,
            set_runner_executable,
            scan_library,
            set_favorite,
            list_tags,
            add_tag,
            remove_tag,
            rename_tag,
            delete_tag,
            get_art_override,
            set_art_override,
            list_executables,
            get_launch_override,
            set_launch_override,
            convert_info,
            convert_game,
            list_addons,
            set_addons_dir,
            convert_all_addons,
            open_addons_dir,
            igdb_configured,
            set_igdb_credentials,
            fetch_artwork,
            get_game_config,
            set_game_config,
            eden_requirements,
            cemu_graphic_packs,
            set_cemu_graphic_pack,
            pretendo_status,
            set_pretendo_service,
            set_pretendo_account_id,
            oauth_capture,
            get_goldberg_dir,
            set_goldberg_dir,
            goldberg_status,
            goldberg_apply,
            goldberg_remove,
            get_goldberg_account_name,
            set_goldberg_account_name,
            list_user_themes,
            open_themes_dir,
            set_preference,
            get_preference,
            play_game,
        ])
        .setup(|app| {
            tray::build(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
