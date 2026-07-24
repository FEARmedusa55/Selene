# Architecture

A Steam-like desktop launcher presenting emulated and PC games as one library.
Local-first: no account, no server, all data on disk.

## Stack

| Layer | Choice | Reason |
|---|---|---|
| Shell | Tauri v2 | Rust core for process supervision, scanning and config I/O; ~10 MB bundle |
| UI | React 19 + TypeScript + Vite | Webview UI makes CSS-variable theming first-class |
| Styling | Plain CSS custom properties | A theme must be one file with no code changes; utility-class frameworks bake color into markup |
| Storage | SQLite (`rusqlite`, bundled) | Local-first, no system dependency |
| State | React built-ins | The shell does not yet justify a state library |

The decisive argument for Rust over Node is playtime tracking: games routinely
fork and exit their launcher, so a correct timer must watch the whole process
*tree*. Windows Job Objects (and Linux process groups) make that reliable.

## Layout

```
src/
  styles/
    tokens.css          Token contract -- every color/space/font in the app
    themes/*.css        One file per theme, overriding tokens under [data-theme]
    base.css            Reset + element defaults
    app.css             Component styles (token-driven only)
  components/           Sidebar, GameGrid, GamePage, SettingsTab, PretendoTab
  types.ts              Domain model, mirrors src-tauri/src/model.rs
  theme.ts              Theme registry + persistence
  format.ts             Playtime/date formatting, placeholder-art helpers

src-tauri/src/
  db.rs                 SQLite schema + versioned migrations
  model.rs              Serde types shared with the frontend
  scan.rs               Walk scan roots; stable per-path ids
  store.rs              All library reads/writes
  launch.rs             Spawn + process-tree tracking for playtime
  config/
    dolphin.rs          Per-game GameSettings/<GameID>.ini writer
  metadata/
    igdb.rs             Twitch OAuth, token cache, tiered queries
    match_score.rs      Candidate scoring (contiguity-based)
  runners/
    mod.rs              The Runner trait -- the core abstraction
    disc.rs             GC/Wii header probing (game id + platform)
    titleid.rs          Title-ID extraction + filename cleanup
    dolphin.rs          Dolphin runner
```

### Data safety

`store::upsert_scanned` updates only scanner-owned columns (title, platform,
runner, title_id). Playtime, favourites, tags, artwork and `added_at` are never
touched by a rescan -- there is a test asserting exactly that, because it is the
kind of regression that silently destroys a user's library.

## The Runner trait

Every way to start a game implements one trait: Cemu, Dolphin, Eden, and plain
PC executables. Adding an emulator is a new module, not edits spread across the
scanner, launcher and config layers.

`available_on_this_os()` is how PC games are excluded from Linux builds -- there
is no Wine/Proton layer by design.

## Config strategy

Global defaults per emulator, with per-game overrides layered on top. Overrides
are written into **each emulator's own per-game mechanism** so the user's global
config is never overwritten:

| Emulator | Native mechanism |
|---|---|
| Dolphin | `<user>/GameSettings/<GameID>.ini` (uppercase ID) |
| Cemu | `<data>/gameProfiles/<titleid>.ini` (lowercase); resolution is a graphic pack |
| Eden | `<data>/config/custom/<TITLEID>.ini` (uppercase), `key\use_global` pairs |

**Writes are merges, not rewrites.** Cemu and Eden per-game files routinely
already contain hand-set values — this machine has a Cemu profile holding
`controller1 = ps4` and three Eden configs. `config::ini_merge` performs line
surgery: managed keys are updated in place, everything else (including comments
and ordering) is copied through. Rewriting from a struct would silently discard
every setting the app does not model.

Cemu never has its shipped defaults touched: `<install>/gameProfiles/default/`
is read-only, and overrides go to the *data* directory.

Eden inherits yuzu's inheritance flags, so overriding takes two lines and
clearing an override must flip the flag rather than delete the key:

```ini
[Renderer]
resolution_setup\use_global=false
resolution_setup=2
```

All three have native support, so `PerGameConfigStyle::SwapGlobal` (back up the
global config, write a modified copy, restore on exit) should never fire. It
exists as a documented fallback only. Any implementation using it must also
restore on startup, since a crash mid-session would otherwise strand the user's
real configuration.

## Metadata pipeline

Title IDs are embedded in most filenames (`[0100000000010000]`, `[AGMP01]`), and
an exact ID beats fuzzy-matching a mangled filename. So:

1. **Read the disc header** for GameCube/Wii images -- authoritative for both
   the game ID and the platform (`runners::disc`). Fall back to the filename.
2. Resolve the ID to an official name via a bundled title database.
3. Query IGDB with that clean name.
4. Fall back to `clean_filename()` when there is no ID.

Never query IGDB with a disc's *internal* title: Paper Mario: The Thousand-Year
Door stores just "Paper Mario", which matches the N64 game.

### Matching rules learned from the live API

- `search` and `where` **cannot be combined** -- IGDB silently returns zero rows.
- `category` no longer exists; it is `game_type` (0 = main game).
- Raw `search` ranks fan projects above real games ("Super Mario Odyssey
  F.L.U.D.D." outranks the real thing), so results are re-scored in-process.
- Scoring keys on **contiguity**: a real subtitle *extends* the query ("Job
  Simulator: The 2050 Archives"), an impostor *interrupts* it ("Job Application
  Simulator"). Bag-of-words cannot tell them apart.
- Platform is a **preference, not a filter**. Filtering hard loses VR titles
  (Vacation Simulator omits platform 6), but ignoring it picks the wrong edition
  where several entries share a name -- IGDB has three games called exactly
  "Paper Mario: The Thousand-Year Door". Platform-filtered tiers run first, then
  unfiltered, with a platform tie-break on equal scores.
- Below `MIN_CONFIDENCE` a game is left **unmatched** rather than given
  plausible-but-wrong art. Wrong art is worse than none.

IGDB auth is Twitch OAuth client-credentials; tokens last ~60 days and are
cached in `settings`. Covers become grid art, artworks/screenshots become hero
art, requested at a large image size. IGDB has no curated hero-banner or
transparent-logo assets, so a manual art override always remains available.

## Environment notes

- **`D:` is exFAT.** No symlinks, so pnpm requires `nodeLinker: hoisted`
  (`pnpm-workspace.yaml`). SQLite WAL may also be rejected on exFAT; `db::open`
  warns and falls back to the default journal rather than failing to start.
- **Eden (this build) does NOT read `.nsz`.** They never appear in its game
  list — which is metadata parsing, so firmware is not the cause — and fail to
  boot. An earlier assumption to the contrary was wrong; the user's own game
  list proved it. `.nsz` is a zstd-compressed `.nsp`, so `convert.rs` shells out
  to the external `nsz` CLI (nicoboss/nsz, user-installed, `pip install nsz`) to
  decompress losslessly, using the user's own `prod.keys` staged into
  `~/.switch/`. The original is kept; `play_game` refuses a `.nsz` with a
  pointed message rather than handing Eden a file it errors on; and the scanner
  drops a `.nsz` once a `.nsp` of the same title ID sits beside it, so a
  converted title does not appear twice.
- SAK (the tool the user first suggested) is GUI-only and archived — unusable
  for automation; the `nsz` CLI is the right choice.
- **Updates and DLC** (`addons.rs`) are not library entries — the Eden runner
  rejects non-base titles. They live in a separate `updates` folder (auto-
  detected as a sibling of the Eden games root) and are matched to their base
  game by **title family**: base, update, and DLC share the first 12 hex of the
  title ID (base low-3 = `000`, update = `800`, DLC = an index with the 13th
  char bumped). The compressed ones are converted like base games.
- **NAND install stays manual.** eden-cli's entire CLI is `-c -f -g -h` (read
  from the binary) — no install command. Eden applies add-ons only from its
  NAND, installed through its GUI (File → Install Files to NAND). The app
  converts and guides; it does not reimplement Nintendo's content installer.
- **Executable size is anti-correlated with being the game.** In both VR titles
  the largest `.exe` is `vcredist_2015-2019_x64.exe` at 14.3 MB, while the game
  itself is 0.6 MB -- a "biggest file" heuristic is wrong every time on this
  library. `runners::exe_pick` therefore hard-excludes redistributables, crash
  handlers and uninstallers, then ranks on engine markers (`<Name>_Data` beside
  `<Name>.exe`), name similarity to the folder, and proximity to
  `steam_api*.dll`. Size contributes almost nothing, and only as a tie-break.
- PC games nest unpredictably (`Game/Game/executable/x.exe`), so detection
  searches the whole folder rather than assuming a depth.
- A PC game is a **folder**, like Cemu's extracted dumps. `NativePc::accepts`
  requires a *plausible* executable inside, so a folder holding only an
  uninstaller never becomes a library entry.
- Hyphens in names are separators, not punctuation: `Job-Simulator` finds
  nothing on IGDB, `Job Simulator` resolves correctly.
- The Dolphin library lives at `d:\Games\Wii & Gamecube\` with **two scan roots**
  (`Wii Roms`, `Gamecube Roms`). The `&` in that path is a shell metacharacter:
  launches must go through `std::process::Command` argv, never a shell string.
  Emulator paths are never hardcoded -- they are detected, then stored in the
  `runners` table.
- IGDB credentials live in `<config>/credentials.json`, outside the repository.
  The loader strips a UTF-8 BOM, since Windows editors add one and `serde_json`
  rejects it.
- **A game is not always a file.** Cemu's extracted Wii U dumps are directories
  (`code/ content/ meta/`); the scanner records the directory and does not
  descend, or its hundreds of internal files each become a "game".
- **Wii U title IDs are not in filenames.** `.wua` archives resolve via Cemu's
  `title_list_cache.xml`; extracted folders via `meta/meta.xml`. Entries Cemu
  has never seen stay unresolved rather than guessed at.
- **Switch folders mix base games with updates and DLC.** Title IDs encode the
  kind in their low 12 bits (base `000`, update `800`, DLC counts up), so only
  base titles are scanned — otherwise A Hat in Time appears three times.
- `title_list_cache.xml` also supplies clean names, which beat filenames
  carrying region and version markers when querying IGDB.

## Phases

0. **Skeleton** (done) -- shell, tokens/themes, schema, Runner trait
1. **MVP** (done) -- Dolphin: scan, IGDB art, launch, playtime, per-game config
2. **Cemu + Eden** (done) -- merge-safe config, graphic packs, requirement checks
3. **PC games** (done, Windows only) -- folder-per-game scanning, scored exe
   detection, manual launch override
4. **Pretendo tab** (built) -- requirement checks, PNID, network-service switch

### Pretendo

Cemu has built-in Pretendo support; this app configures it and, more usefully,
reports what is missing. Cemu records the choice in `settings.xml`:

```xml
<Account>
    <PersistentId>2147483651</PersistentId>
    <OnlineEnabled>false</OnlineEnabled>
    <ActiveService>0</ActiveService>   <!-- 0 Nintendo, 1 Pretendo, 2 Custom -->
</Account>
```

Only that element is rewritten; the surrounding configuration (graphic packs,
game paths, window state) is passed through untouched, and writes are refused
while Cemu is running because it rewrites the file wholesale on exit.

`account.dat` is a plain `key=value` file. `MiiName` is hex-encoded UTF-16.
An account is only usable online when **both** `AccountId` and a non-zero
`PrincipalId` are set — a half-configured account must not read as ready.

The password is deliberately not written: Cemu stores a derived
`AccountPasswordCache`, and reproducing that derivation risks writing something
Cemu cannot use. Cemu's own account screen handles it.
5. Goldberg (gbe_fork)
6. **Polish** (done) -- tags, manual art override, system tray + now-playing,
   user-authored themes, gamepad menu navigation
7. Big-picture mode (stretch)

### Gamepad navigation (`src/gamepad.ts`)

Uses the Web Gamepad API (WebView2 supports it). A single rAF poll loop does
rising-edge detection on buttons and **spatial** focus movement: a direction
press jumps to the nearest focusable element that way by bounding-rect scoring,
which suits a grid + sidebar far better than DOM order. A / activate, B / back,
bumpers / switch tab. `:root.using-gamepad :focus` gives a stronger ring so
focus reads from across the room. The poll loop starts once; handlers read live
state through a ref so it never restarts.

### User themes

`list_user_themes` reads `<config>/themes/*.css`; each file declares tokens
under `[data-theme="<filename>"]`. The frontend injects the CSS into one
`<style>` and adds the theme to the picker, preview colours parsed from the
tokens. Still "one file, no code" -- the same contract as the built-ins.

### Now-playing

The tray persists for the app's lifetime (`tray.rs`). `play_game` sets the
tooltip to the title and minimises the window (toggle in Settings), restoring it
on exit **before** surfacing any launch error so the window can never be left
minimised.
