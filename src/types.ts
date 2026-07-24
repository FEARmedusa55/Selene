/* Domain model shared with the Rust core. These shapes mirror the SQLite
   schema and the serde structs in src-tauri/src/model.rs -- keep them in sync. */

export type Platform = "wiiu" | "wii" | "gamecube" | "switch" | "pc";

export type RunnerId = "cemu" | "dolphin" | "eden" | "native-pc";

/** Which runner owns which platforms, and what it scans for. Mirrors the
 *  `Runner` trait implementations in the Rust core. */
export interface RunnerInfo {
  id: RunnerId;
  name: string;
  platforms: Platform[];
  extensions: string[];
  /** false on Linux builds for native-pc -- no Wine/Proton layer by design. */
  availableOnThisOs: boolean;
}

export interface Game {
  id: string;
  title: string;
  platform: Platform;
  runner: RunnerId;
  /** ROM file, game folder, or PC executable. */
  path: string;
  /** Extracted from the filename where present -- e.g. "0100000000010000"
   *  (Switch), "AGMP01" (Wii U), "RMGE01" (GameCube/Wii). Primary key for
   *  metadata matching; far more reliable than fuzzy title search. */
  titleId?: string;
  coverUrl?: string;
  heroUrl?: string;
  playtimeSeconds: number;
  lastPlayedAt?: string;
  addedAt: string;
  tags: string[];
  favorite: boolean;
  /** True while the launched process tree is still alive. */
  running?: boolean;
  /** Artwork was set by hand rather than resolved from IGDB. */
  customArt?: boolean;
}

export type SortKey = "title" | "lastPlayed" | "playtime" | "added";

export interface LibraryFilter {
  search: string;
  platforms: Platform[];
  tags: string[];
  favoritesOnly: boolean;
}

export const PLATFORM_LABELS: Record<Platform, string> = {
  wiiu: "Wii U",
  wii: "Wii",
  gamecube: "GameCube",
  switch: "Switch",
  pc: "PC",
};

export const RUNNER_LABELS: Record<RunnerId, string> = {
  cemu: "Cemu",
  dolphin: "Dolphin",
  eden: "Eden",
  "native-pc": "Direct launch",
};
