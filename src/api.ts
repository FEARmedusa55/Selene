/* Typed wrappers over the Rust commands. Every call the UI makes to the core
   goes through here, so the invoke-name strings live in exactly one place. */

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Game, RunnerId } from "./types";

/** Resolve an artwork value for display.
 *
 *  Manual overrides may be a local path rather than a URL, and the webview
 *  cannot load `C:\...` directly — it has to go through Tauri's asset
 *  protocol. Remote URLs pass through untouched. */
export function artSrc(value: string | undefined): string | undefined {
  if (!value) return undefined;
  if (/^https?:\/\//i.test(value)) return value;
  if (!inTauri) return value;
  try {
    return convertFileSrc(value);
  } catch {
    return value;
  }
}

export interface RunnerInfo {
  id: RunnerId;
  name: string;
  platforms: string[];
  extensions: string[];
  availableOnThisOs: boolean;
  executableDetected: string | null;
  executableConfigured: string | null;
}

export interface ScanReport {
  found: number;
  inserted: number;
  updated: number;
}

export interface ArtworkReport {
  resolved: number;
  unmatched: string[];
}

/** Per-game Dolphin overrides. Every field optional: absent = inherit global. */
export interface DolphinGameConfig {
  graphicsBackend?: string;
  internalResolution?: number;
  msaa?: number;
  anisotropicFiltering?: number;
  vsync?: boolean;
  widescreenHack?: boolean;
  dualCore?: boolean;
  emulationSpeed?: number;
  progressiveScan?: boolean;
}

/** Per-game Cemu overrides. Resolution is NOT here -- Cemu models it as a
 *  graphic pack, surfaced separately via `cemuGraphicPacks`. */
export interface CemuGameConfig {
  cpuMode?: number;
  threadQuantum?: number;
  accurateShaderMul?: number;
  precompiledShaders?: number;
  loadSharedLibraries?: boolean;
  startWithPadView?: boolean;
}

/** Per-game Eden overrides. */
export interface EdenGameConfig {
  graphicsBackend?: number;
  resolutionSetup?: number;
  scalingFilter?: number;
  antiAliasing?: number;
  gpuAccuracy?: number;
  useVsync?: number;
  useMultiCore?: boolean;
  speedLimit?: number;
  useDockedMode?: boolean;
}

/** Any runner's per-game config. Which one is decided by `game.runner`. */
export type GameConfig = DolphinGameConfig & CemuGameConfig & EdenGameConfig;

/** What Eden needs the user to supply before games will boot. */
export interface EdenRequirements {
  prodKeys: boolean;
  titleKeys: boolean;
  firmware: boolean;
  firmwareTitleCount: number;
}

export type NetworkService = "nintendo" | "pretendo" | "custom";

export interface RequiredFile {
  label: string;
  relativePath: string;
  present: boolean;
  count: number;
  detail: string;
}

export interface CemuAccount {
  persistentId: string;
  miiName: string;
  /** PNID/NNID username. Empty when the account is local-only. */
  accountId: string;
  principalId: number;
  isActive: boolean;
}

export interface PretendoStatus {
  cemuConfigured: boolean;
  dataDir: string;
  service: NetworkService;
  onlineEnabled: boolean;
  accounts: CemuAccount[];
  files: RequiredFile[];
  filesComplete: boolean;
  cemuRunning: boolean;
}

export interface Addon {
  titleId: string;
  family: string;
  kind: "update" | "dlc";
  name: string;
  version: number | null;
  path: string;
  /** Compressed and not yet decompressed into an installable form. */
  needsConversion: boolean;
}

export interface AddonsView {
  dir: string | null;
  addons: Addon[];
  needsConversion: number;
  converterAvailable: boolean;
}

export interface ConvertInfo {
  /** The game's file is a format its runner can't read, but is convertible. */
  needsConversion: boolean;
  fromExt: string | null;
  toExt: string | null;
  /** A converter (nsz) was found on the machine. */
  converterAvailable: boolean;
  converterPath: string | null;
}

/** A candidate executable inside a PC game's folder. */
export interface ExeCandidate {
  path: string;
  fileName: string;
  /** Heuristic confidence; higher is better. Not a percentage. */
  score: number;
  /** Why it ranked where it did, e.g. "Unity data folder". */
  reasons: string[];
  sizeMb: number;
}

export interface PresetCategory {
  /** Empty for presets declared without a category. */
  name: string;
  presets: string[];
}

export interface GraphicPack {
  name: string;
  rulesPath: string;
  description: string;
  categories: PresetCategory[];
  /** An <Entry> in Cemu's settings.xml means enabled; there is no flag. */
  enabled: boolean;
  /** Active [category, preset] pairs. */
  activePresets: [string, string][];
}

export const api = {
  listGames: () => invoke<Game[]>("list_games"),
  listRunners: () => invoke<RunnerInfo[]>("list_runners"),

  listScanRoots: () => invoke<[string, string][]>("list_scan_roots"),
  addScanRoot: (runner: string, path: string) =>
    invoke<void>("add_scan_root", { runner, path }),
  removeScanRoot: (runner: string, path: string) =>
    invoke<void>("remove_scan_root", { runner, path }),

  setRunnerExecutable: (runner: string, path: string) =>
    invoke<void>("set_runner_executable", { runner, path }),

  scanLibrary: () => invoke<ScanReport>("scan_library"),
  setFavorite: (gameId: string, favorite: boolean) =>
    invoke<void>("set_favorite", { gameId, favorite }),

  listUserThemes: () => invoke<{ id: string; name: string; css: string }[]>("list_user_themes"),
  openThemesDir: () => invoke<void>("open_themes_dir"),
  getPreference: (key: string) => invoke<string | null>("get_preference", { key }),
  setPreference: (key: string, value: string) =>
    invoke<void>("set_preference", { key, value }),

  listTags: () => invoke<[string, number][]>("list_tags"),
  addTag: (gameId: string, name: string) => invoke<void>("add_tag", { gameId, name }),
  removeTag: (gameId: string, name: string) =>
    invoke<void>("remove_tag", { gameId, name }),
  renameTag: (from: string, to: string) => invoke<void>("rename_tag", { from, to }),
  deleteTag: (name: string) => invoke<void>("delete_tag", { name }),

  pretendoStatus: () => invoke<PretendoStatus>("pretendo_status"),
  setPretendoService: (service: NetworkService, online: boolean) =>
    invoke<void>("set_pretendo_service", { service, online }),
  setPretendoAccountId: (persistentId: string, pnid: string) =>
    invoke<void>("set_pretendo_account_id", { persistentId, pnid }),

  listExecutables: (gameId: string) =>
    invoke<ExeCandidate[]>("list_executables", { gameId }),
  getLaunchOverride: (gameId: string) =>
    invoke<string | null>("get_launch_override", { gameId }),
  setLaunchOverride: (gameId: string, exe: string | null) =>
    invoke<void>("set_launch_override", { gameId, exe }),

  getArtOverride: (gameId: string) =>
    invoke<[string | null, string | null]>("get_art_override", { gameId }),
  setArtOverride: (gameId: string, cover: string | null, hero: string | null) =>
    invoke<void>("set_art_override", { gameId, cover, hero }),

  igdbConfigured: () => invoke<boolean>("igdb_configured"),
  setIgdbCredentials: (clientId: string, clientSecret: string) =>
    invoke<void>("set_igdb_credentials", { clientId, clientSecret }),
  fetchArtwork: () => invoke<ArtworkReport>("fetch_artwork"),

  getGameConfig: async (gameId: string): Promise<GameConfig> => {
    const raw = await invoke<string>("get_game_config", { gameId });
    try {
      return JSON.parse(raw) as GameConfig;
    } catch {
      return {};
    }
  },
  setGameConfig: (gameId: string, cfg: GameConfig) =>
    invoke<void>("set_game_config", { gameId, overrides: JSON.stringify(cfg) }),

  edenRequirements: () => invoke<EdenRequirements | null>("eden_requirements"),
  cemuGraphicPacks: (gameId: string) =>
    invoke<GraphicPack[]>("cemu_graphic_packs", { gameId }),
  setCemuGraphicPack: (
    rulesPath: string,
    enabled: boolean,
    presets: [string, string][],
  ) => invoke<void>("set_cemu_graphic_pack", { rulesPath, enabled, presets }),

  /** Resolves with the session length in seconds once the game exits. */
  convertInfo: (gameId: string) => invoke<ConvertInfo>("convert_info", { gameId }),
  convertGame: (gameId: string) => invoke<string>("convert_game", { gameId }),

  listAddons: (gameId: string) => invoke<AddonsView>("list_addons", { gameId }),
  setAddonsDir: (path: string) => invoke<void>("set_addons_dir", { path }),
  convertAllAddons: () => invoke<string>("convert_all_addons"),
  openAddonsDir: () => invoke<void>("open_addons_dir"),
  onAddonProgress: (fn: (done: number, total: number, name: string) => void) =>
    listen<[number, number, string]>("addon-convert-progress", (e) =>
      fn(e.payload[0], e.payload[1], e.payload[2]),
    ),

  onConvertStarted: (fn: (gameId: string) => void) =>
    listen<string>("convert-started", (e) => fn(e.payload)),
  onConvertFinished: (fn: (gameId: string) => void) =>
    listen<string>("convert-finished", (e) => fn(e.payload)),

  playGame: (gameId: string) => invoke<number>("play_game", { gameId }),

  onGameStarted: (fn: (gameId: string) => void) =>
    listen<string>("game-started", (e) => fn(e.payload)),
  onGameStopped: (fn: (gameId: string) => void) =>
    listen<string>("game-stopped", (e) => fn(e.payload)),
  onArtworkProgress: (fn: (count: number) => void) =>
    listen<number>("artwork-progress", (e) => fn(e.payload)),
};

/** True when running inside the Tauri shell rather than a plain browser tab. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
