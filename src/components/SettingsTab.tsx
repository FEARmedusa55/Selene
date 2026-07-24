import { useCallback, useEffect, useState } from "react";
import type { ThemeMeta } from "../theme";
import { api, inTauri, type EdenRequirements, type RunnerInfo } from "../api";

interface Props {
  theme: string;
  onThemeChange: (id: string) => void;
  onLibraryChanged: () => void;
  themes: ThemeMeta[];
  onReloadThemes: () => void;
}

export function SettingsTab({
  theme,
  onThemeChange,
  onLibraryChanged,
  themes,
  onReloadThemes,
}: Props) {
  const [minimizeOnPlay, setMinimizeOnPlay] = useState(true);
  const [runners, setRunners] = useState<RunnerInfo[]>([]);
  const [roots, setRoots] = useState<[string, string][]>([]);
  const [newRoot, setNewRoot] = useState("");
  const [newRootRunner, setNewRootRunner] = useState("dolphin");
  /** Editable executable path per runner id. */
  const [exePaths, setExePaths] = useState<Record<string, string>>({});
  const [edenReq, setEdenReq] = useState<EdenRequirements | null>(null);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [igdbReady, setIgdbReady] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!inTauri) return;
    try {
      const [r, sr, ok] = await Promise.all([
        api.listRunners(),
        api.listScanRoots(),
        api.igdbConfigured(),
      ]);
      setRunners(r);
      setRoots(sr);
      setIgdbReady(ok);
      setExePaths(
        Object.fromEntries(
          r.map((x) => [x.id, x.executableConfigured ?? x.executableDetected ?? ""]),
        ),
      );
      setEdenReq(await api.edenRequirements().catch(() => null));
      setMinimizeOnPlay((await api.getPreference("minimize_on_play")) !== "false");
    } catch (e) {
      console.error("failed to load settings", e);
    }
  }, []);

  const toggleMinimize = async (on: boolean) => {
    setMinimizeOnPlay(on);
    if (inTauri) await api.setPreference("minimize_on_play", on ? "true" : "false");
  };

  useEffect(() => {
    void reload();
  }, [reload]);

  const flash = (msg: string) => {
    setNote(msg);
    setTimeout(() => setNote(null), 4000);
  };

  const saveExe = async (runnerId: string) => {
    await api.setRunnerExecutable(runnerId, (exePaths[runnerId] ?? "").trim());
    await reload();
    flash("Emulator path saved");
  };

  const addRoot = async () => {
    const path = newRoot.trim();
    if (!path) return;
    await api.addScanRoot(newRootRunner, path);
    setNewRoot("");
    await reload();
    onLibraryChanged();
    flash("Scan folder added — press Scan in the Library tab");
  };

  const removeRoot = async (runner: string, path: string) => {
    await api.removeScanRoot(runner, path);
    await reload();
    flash("Scan folder removed");
  };

  const saveCreds = async () => {
    await api.setIgdbCredentials(clientId.trim(), clientSecret.trim());
    setClientId("");
    setClientSecret("");
    await reload();
    flash("IGDB credentials saved");
  };

  return (
    <div className="page">
      <h1 className="page__title">Settings</h1>
      {note && <div className="notice notice--info">{note}</div>}

      <section className="panel">
        <h2 className="panel__title">
          Theme
          <span className="panel__hint">Every color, space and font is a design token</span>
        </h2>
        <ul className="themegrid">
          {themes.map((t) => (
            <li key={t.id}>
              <button
                className="themecard"
                data-active={t.id === theme}
                onClick={() => onThemeChange(t.id)}
              >
                <span
                  className="themecard__swatch"
                  style={{ background: t.preview.bg }}
                  aria-hidden="true"
                >
                  <span style={{ background: t.preview.surface }} />
                  <span style={{ background: t.preview.accent }} />
                </span>
                <span className="themecard__name">{t.name}</span>
                <span className="themecard__desc">{t.description}</span>
              </button>
            </li>
          ))}
        </ul>
        {inTauri && (
          <>
            <div className="notice notice--info">
              Drop a <code>.css</code> file in the themes folder — declaring your
              tokens under <code>[data-theme=&quot;your-name&quot;]</code> — and it
              appears here. No code, one file.
            </div>
            <div className="row">
              <button className="btn btn--ghost" onClick={() => void api.openThemesDir()}>
                Open themes folder
              </button>
              <button className="btn btn--ghost" onClick={onReloadThemes}>
                Reload themes
              </button>
            </div>
          </>
        )}
      </section>

      {inTauri && (
        <section className="panel">
          <h2 className="panel__title">
            While playing
            <span className="panel__hint">Steam-like behaviour</span>
          </h2>
          <label className="toggle">
            <input
              type="checkbox"
              checked={minimizeOnPlay}
              onChange={(e) => void toggleMinimize(e.target.checked)}
            />
            <span>Minimize Selene while a game is running</span>
          </label>
          <div className="notice notice--info">
            The tray icon shows what&rsquo;s playing; click it to bring Selene
            back. The window also restores automatically when the game exits.
          </div>
        </section>
      )}

      {!inTauri ? (
        <section className="panel">
          <div className="notice notice--warn">
            Running outside the desktop shell, so emulator and library settings
            are unavailable. Showing fixture data.
          </div>
        </section>
      ) : (
        <>
          <section className="panel">
            <h2 className="panel__title">
              Emulators
              <span className="panel__hint">Point each at its executable</span>
            </h2>
            {runners.map((r) => (
              <div className="field" key={r.id}>
                <label>
                  {r.name} — {r.platforms.join(", ")}
                </label>
                <div className="row">
                  <input
                    className="input"
                    value={exePaths[r.id] ?? ""}
                    onChange={(e) =>
                      setExePaths((p) => ({ ...p, [r.id]: e.target.value }))
                    }
                    placeholder={`Full path to ${r.name}`}
                    spellCheck={false}
                  />
                  <button className="btn btn--ghost" onClick={() => saveExe(r.id)}>
                    Save
                  </button>
                </div>
                <span className="field__hint">
                  Scans {r.extensions.map((e) => `.${e}`).join(", ")}
                  {r.executableConfigured
                    ? " — configured."
                    : r.executableDetected
                      ? " — auto-detected; press Save to confirm."
                      : " — paste the full path above."}
                </span>
              </div>
            ))}
          </section>

          {edenReq && (
            <section className="panel">
              <h2 className="panel__title">
                Eden requirements
                <span className="panel__hint">Must be dumped from a Switch you own</span>
              </h2>
              {!edenReq.prodKeys ? (
                <div className="notice notice--warn">
                  <strong>prod.keys is missing.</strong> Nothing will run without
                  it. This app bundles no keys or firmware — you supply your own.
                </div>
              ) : (
                !edenReq.firmware && (
                  <div className="notice notice--info">
                    Firmware is not installed. Games run on your keys alone; it
                    is only needed for <strong>system applets</strong> (on-screen
                    keyboard, profile select, error dialogs) and amiibo.
                  </div>
                )
              )}
              <ul className="checklist">
                <li className="checklist__item" data-state={edenReq.prodKeys ? "ok" : "missing"}>
                  <span className="checklist__dot" />
                  <span className="checklist__label">prod.keys</span>
                  <span className="checklist__state">
                    {edenReq.prodKeys ? "Installed" : "Missing"}
                  </span>
                </li>
                <li className="checklist__item" data-state={edenReq.titleKeys ? "ok" : "missing"}>
                  <span className="checklist__dot" />
                  <span className="checklist__label">title.keys</span>
                  <span className="checklist__state">
                    {edenReq.titleKeys ? "Installed" : "Missing"}
                  </span>
                </li>
                <li className="checklist__item" data-state={edenReq.firmware ? "ok" : "missing"}>
                  <span className="checklist__dot" />
                  <span className="checklist__label">System firmware</span>
                  <span className="checklist__state">
                    {edenReq.firmware ? `${edenReq.firmwareTitleCount} titles` : "Not installed"}
                  </span>
                </li>
              </ul>
            </section>
          )}

          <section className="panel">
            <h2 className="panel__title">
              Game folders
              <span className="panel__hint">Scanned recursively, per emulator</span>
            </h2>
            <div className="field">
              <div className="row">
                <select
                  className="select"
                  value={newRootRunner}
                  onChange={(e) => setNewRootRunner(e.target.value)}
                >
                  {runners.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name}
                    </option>
                  ))}
                </select>
                <input
                  className="input"
                  value={newRoot}
                  onChange={(e) => setNewRoot(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void addRoot()}
                  placeholder="d:\Games\Wii U\Roms"
                  spellCheck={false}
                />
                <button className="btn btn--ghost" onClick={addRoot}>
                  Add
                </button>
              </div>
            </div>
            {roots.length === 0 ? (
              <div className="notice notice--warn">
                No folders configured. Add one per library, e.g. your Wii Roms,
                Gamecube Roms, Wii U Roms and Switch games folders.
              </div>
            ) : (
              <ul className="checklist">
                {roots.map(([runner, path]) => (
                  <li className="checklist__item" key={`${runner}:${path}`} data-state="ok">
                    <span className="checklist__dot" />
                    <span className="checklist__label" data-selectable>
                      <span className="badge badge--muted">
                        {runners.find((r) => r.id === runner)?.name ?? runner}
                      </span>{" "}
                      {path}
                    </span>
                    <button
                      className="btn btn--small btn--ghost"
                      onClick={() => removeRoot(runner, path)}
                    >
                      Remove
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="panel">
            <h2 className="panel__title">
              Artwork &amp; metadata
              <span className="panel__hint">IGDB via Twitch OAuth</span>
            </h2>
            {igdbReady ? (
              <div className="notice notice--info">
                Credentials are configured. Use “Get artwork” in the Library tab.
              </div>
            ) : (
              <div className="notice notice--warn">
                Register an application on the Twitch developer console, then
                paste its Client ID and Secret here. They are stored in your
                config folder, never in the project.
              </div>
            )}
            <div className="placeholder-form">
              <div className="field">
                <label>Client ID</label>
                <input
                  className="input"
                  value={clientId}
                  onChange={(e) => setClientId(e.target.value)}
                  spellCheck={false}
                />
              </div>
              <div className="field">
                <label>Client Secret</label>
                <input
                  className="input"
                  type="password"
                  value={clientSecret}
                  onChange={(e) => setClientSecret(e.target.value)}
                  spellCheck={false}
                />
              </div>
            </div>
            <div className="row">
              <button
                className="btn btn--ghost"
                onClick={saveCreds}
                disabled={!clientId.trim() || !clientSecret.trim()}
              >
                {igdbReady ? "Replace credentials" : "Save credentials"}
              </button>
            </div>
          </section>
        </>
      )}
    </div>
  );
}
