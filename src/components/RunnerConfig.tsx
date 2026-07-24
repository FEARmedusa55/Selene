import { useCallback, useEffect, useState } from "react";
import type { Game } from "../types";
import { api, inTauri, type GameConfig, type GraphicPack } from "../api";

interface Props {
  game: Game;
}

/** "Inherit global" is the absence of a value, not a value. Writing an explicit
 *  default would freeze the game against later changes to the user's globals. */
const INHERIT = "";

interface SelectSpec {
  key: keyof GameConfig;
  label: string;
  options: [number | string, string][];
  /** Values are numbers unless this is set. */
  asString?: boolean;
}

function ConfigSelects({
  specs,
  cfg,
  onChange,
}: {
  specs: SelectSpec[];
  cfg: GameConfig;
  onChange: (key: keyof GameConfig, value: unknown) => void;
}) {
  return (
    <div className="placeholder-form">
      {specs.map((s) => (
        <div className="field" key={String(s.key)}>
          <label>{s.label}</label>
          <select
            className="select"
            value={(cfg[s.key] as string | number | undefined) ?? INHERIT}
            onChange={(e) =>
              onChange(
                s.key,
                e.target.value === INHERIT
                  ? undefined
                  : s.asString
                    ? e.target.value
                    : Number(e.target.value),
              )
            }
          >
            <option value={INHERIT}>Use global default</option>
            {s.options.map(([v, label]) => (
              <option key={String(v)} value={v}>
                {label}
              </option>
            ))}
          </select>
        </div>
      ))}
    </div>
  );
}

const DOLPHIN_SELECTS: SelectSpec[] = [
  {
    key: "graphicsBackend",
    label: "Graphics backend",
    asString: true,
    options: [
      ["OGL", "OpenGL"],
      ["Vulkan", "Vulkan"],
      ["D3D", "Direct3D 11"],
      ["D3D12", "Direct3D 12"],
      ["Software", "Software"],
    ],
  },
  {
    key: "internalResolution",
    label: "Internal resolution",
    options: [
      [1, "1x — native"],
      [2, "2x — 720p"],
      [3, "3x — 1080p"],
      [4, "4x — 1440p"],
      [6, "6x — 4K"],
    ],
  },
  {
    key: "msaa",
    label: "Anti-aliasing",
    options: [
      [1, "None"],
      [2, "2x MSAA"],
      [4, "4x MSAA"],
      [8, "8x MSAA"],
    ],
  },
];

const CEMU_SELECTS: SelectSpec[] = [
  {
    key: "cpuMode",
    label: "CPU mode",
    options: [
      [0, "Single-core interpreter"],
      [1, "Dual-core recompiler"],
      [3, "Triple-core recompiler"],
      [4, "Auto"],
    ],
  },
  {
    key: "accurateShaderMul",
    label: "Shader multiplication",
    options: [
      [0, "False — fastest"],
      [1, "True — accurate"],
      [2, "Auto"],
    ],
  },
  {
    key: "precompiledShaders",
    label: "Precompiled shaders",
    options: [
      [0, "Disabled"],
      [1, "Enabled"],
      [2, "Auto"],
    ],
  },
];

/** Tri-state boolean settings per runner. */
const BOOLEANS: Partial<
  Record<string, { key: keyof GameConfig & string; label: string }[]>
> = {
  dolphin: [
    { key: "widescreenHack", label: "Widescreen hack (16:9)" },
    { key: "vsync", label: "V-Sync" },
    { key: "dualCore", label: "Dual core" },
  ],
  cemu: [
    { key: "startWithPadView", label: "Start with GamePad view" },
    { key: "loadSharedLibraries", label: "Load shared libraries" },
  ],
  eden: [
    { key: "useDockedMode", label: "Docked mode" },
    { key: "useMultiCore", label: "Multicore CPU" },
  ],
};

const EDEN_SELECTS: SelectSpec[] = [
  {
    key: "graphicsBackend",
    label: "Graphics backend",
    options: [
      [0, "OpenGL"],
      [1, "Vulkan"],
      [2, "Null"],
    ],
  },
  {
    key: "resolutionSetup",
    label: "Resolution scale",
    options: [
      [0, "0.5x — 360p"],
      [1, "1x — 720p (native)"],
      [2, "2x — 1440p"],
      [3, "3x — 2160p"],
      [4, "4x — 2880p"],
    ],
  },
  {
    key: "scalingFilter",
    label: "Scaling filter",
    options: [
      [0, "Nearest"],
      [1, "Bilinear"],
      [2, "Bicubic"],
      [4, "ScaleForce"],
      [5, "AMD FSR"],
    ],
  },
  {
    key: "antiAliasing",
    label: "Anti-aliasing",
    options: [
      [0, "None"],
      [1, "FXAA"],
      [2, "SMAA"],
    ],
  },
  {
    key: "gpuAccuracy",
    label: "GPU accuracy",
    options: [
      [0, "Normal"],
      [1, "High"],
      [2, "Extreme"],
    ],
  },
];

/* Per-game configuration, routed by runner.
   Each emulator exposes the settings it actually supports -- deliberately not a
   shared generic form, since the three do not overlap much. */
export function RunnerConfig({ game }: Props) {
  const [cfg, setCfg] = useState<GameConfig>({});
  const [packs, setPacks] = useState<GraphicPack[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const [packNote, setPackNote] = useState<string | null>(null);

  const reloadPacks = useCallback(() => {
    if (!inTauri || game.runner !== "cemu") {
      setPacks([]);
      return;
    }
    void api.cemuGraphicPacks(game.id).then(setPacks).catch(() => setPacks([]));
  }, [game.id, game.runner]);

  useEffect(() => {
    if (!inTauri) return;
    void api.getGameConfig(game.id).then(setCfg).catch(() => setCfg({}));
    reloadPacks();
  }, [game.id, reloadPacks]);

  /** Current selections, defaulting each category to its first preset --
   *  Cemu expects a choice per category once a pack is on. */
  const selectionsFor = (p: GraphicPack): [string, string][] =>
    p.categories.map((cat) => [
      cat.name,
      p.activePresets.find(([c]) => c === cat.name)?.[1] ?? cat.presets[0] ?? "",
    ]);

  const writePack = async (
    p: GraphicPack,
    enabled: boolean,
    presets: [string, string][],
  ) => {
    try {
      await api.setCemuGraphicPack(p.rulesPath, enabled, presets);
      setPackNote(null);
      reloadPacks();
    } catch (e) {
      // Most often: Cemu is open, and would overwrite settings.xml on exit.
      setPackNote(String(e));
      reloadPacks();
    }
  };

  const togglePack = (p: GraphicPack, enabled: boolean) =>
    writePack(p, enabled, enabled ? selectionsFor(p) : []);

  const setPreset = (p: GraphicPack, category: string, preset: string) => {
    const next = selectionsFor(p).map(
      ([c, v]) => [c, c === category ? preset : v] as [string, string],
    );
    return writePack(p, true, next);
  };

  const save = async (next: GameConfig) => {
    setCfg(next);
    if (!inTauri) return;
    try {
      await api.setGameConfig(game.id, next);
      setNote("Saved — existing settings in the file were preserved");
    } catch (e) {
      setNote(`Could not save: ${e}`);
    }
    setTimeout(() => setNote(null), 3500);
  };

  const setKey = (key: keyof GameConfig, value: unknown) => {
    const next = { ...cfg };
    if (value === undefined) delete next[key];
    else (next as Record<string, unknown>)[key] = value;
    void save(next);
  };

  // PC games have no emulator, so there is nothing to configure here -- the
  // executable choice lives in its own panel. Without this the runner falls
  // through to Eden's settings, which would be nonsense.
  if (game.runner === "native-pc") return null;

  const noTitleId = !game.titleId;

  const targetFile =
    game.runner === "dolphin"
      ? `GameSettings/${game.titleId ?? "…"}.ini`
      : game.runner === "cemu"
        ? `gameProfiles/${(game.titleId ?? "…").toLowerCase()}.ini`
        : `config/custom/${game.titleId ?? "…"}.ini`;

  const runnerName =
    game.runner === "dolphin" ? "Dolphin" : game.runner === "cemu" ? "Cemu" : "Eden";

  const selects =
    game.runner === "dolphin"
      ? DOLPHIN_SELECTS
      : game.runner === "cemu"
        ? CEMU_SELECTS
        : EDEN_SELECTS;

  return (
    <>
      <section className="panel">
        <h2 className="panel__title">
          Configuration
          <span className="panel__hint">Overrides layered on your {runnerName} defaults</span>
        </h2>

        {noTitleId ? (
          <div className="notice notice--warn">
            No title ID resolved for this game yet, so there is nothing to key a
            per-game file on. For <code>.wua</code> archives this usually means
            Cemu has not scanned the folder yet — open Cemu once, then rescan.
          </div>
        ) : (
          <div className="notice notice--info">
            Written to {runnerName}&rsquo;s own <code>{targetFile}</code>. Existing
            settings in that file are merged, not overwritten, and your global
            configuration is never modified.
          </div>
        )}
        {note && <div className="notice notice--info">{note}</div>}

        <ConfigSelects specs={selects} cfg={cfg} onChange={setKey} />

        {/* Booleans are tri-state, not checkboxes. A checkbox has two states,
            but these settings have three: On, Off, and inherit-the-global.
            Using one meant "unchecked" collapsed into "inherit", so an override
            could be turned on but never explicitly off. */}
        <div className="placeholder-form">
          {BOOLEANS[game.runner]?.map(({ key, label }) => (
            <div className="field" key={key}>
              <label>{label}</label>
              <select
                className="select"
                value={cfg[key] === undefined ? INHERIT : cfg[key] ? "on" : "off"}
                onChange={(e) =>
                  setKey(
                    key,
                    e.target.value === INHERIT ? undefined : e.target.value === "on",
                  )
                }
              >
                <option value={INHERIT}>Use global default</option>
                <option value="on">On</option>
                <option value="off">Off</option>
              </select>
            </div>
          ))}
        </div>
      </section>

      {game.runner === "cemu" && (
        <section className="panel">
          <h2 className="panel__title">
            Graphic packs
            <span className="panel__hint">How Cemu does resolution and mods</span>
          </h2>
          <div className="notice notice--info">
            Cemu handles resolution scaling through graphic packs rather than a
            setting. Changes are written to Cemu&rsquo;s <code>settings.xml</code>;
            only the graphic-pack section is touched.
          </div>
          {packNote && <div className="notice notice--warn">{packNote}</div>}

          {packs.length === 0 ? (
            <div className="notice notice--warn">
              No graphic packs installed for this title. Download them from
              Cemu&rsquo;s own graphic pack manager.
            </div>
          ) : (
            <ul className="packlist">
              {packs.map((p) => (
                <li className="packlist__item" key={p.rulesPath} data-on={p.enabled}>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={p.enabled}
                      onChange={(e) => void togglePack(p, e.target.checked)}
                    />
                    <span className="packlist__name">{p.name || "(unnamed pack)"}</span>
                  </label>
                  {p.description && (
                    <div className="packlist__desc">{p.description.split("||")[0]}</div>
                  )}
                  {p.enabled && p.categories.length > 0 && (
                    <div className="packlist__presets">
                      {p.categories.map((cat) => (
                        <div className="field" key={cat.name || "_"}>
                          <label>{cat.name || "Preset"}</label>
                          <select
                            className="select"
                            value={
                              p.activePresets.find(([c]) => c === cat.name)?.[1] ??
                              cat.presets[0] ??
                              ""
                            }
                            onChange={(e) => void setPreset(p, cat.name, e.target.value)}
                          >
                            {cat.presets.map((preset) => (
                              <option key={preset} value={preset}>
                                {preset}
                              </option>
                            ))}
                          </select>
                        </div>
                      ))}
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
    </>
  );
}
