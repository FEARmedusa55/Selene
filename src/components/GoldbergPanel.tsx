import { useCallback, useEffect, useState } from "react";
import type { Game } from "../types";
import { api, inTauri, type GoldbergStatus } from "../api";

interface Props {
  game: Game;
}

/* Steam-API emulation for PC games via Goldberg. Shown only for PC games that
   actually carry a steam_api(64).dll — the backend decides `supported`.

   Goldberg replaces the game's Steam DLL with an emulated build so a delisted or
   Steam-gated title runs offline. Selene backs up the original and records every
   change, so Remove restores the game exactly. The emulator binaries are
   user-supplied (Settings → Steam emulation); this app bundles none. */
export function GoldbergPanel({ game }: Props) {
  const [status, setStatus] = useState<GoldbergStatus | null>(null);
  const [appId, setAppId] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!inTauri || game.runner !== "native-pc") return;
    try {
      const s = await api.goldbergStatus(game.id);
      setStatus(s);
      setAppId((prev) => prev || s.appId || "");
    } catch {
      setStatus(null);
    }
  }, [game.id, game.runner]);

  useEffect(() => {
    void load();
  }, [load, game.path]);

  // Render nothing until we know it's a Steam game.
  if (!status?.supported) return null;

  const flash = (msg: string) => {
    setNote(msg);
    setTimeout(() => setNote(null), 5000);
  };

  const apply = async () => {
    setBusy(true);
    try {
      await api.goldbergApply(game.id, appId.trim());
      await load();
      flash("Goldberg installed. Launch the game to confirm it boots offline.");
    } catch (e) {
      flash(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await api.goldbergRemove(game.id);
      await load();
      flash("Reverted — the original Steam DLL is back in place.");
    } catch (e) {
      flash(String(e));
    } finally {
      setBusy(false);
    }
  };

  const arches = [...new Set(status.dlls.map((d) => d.arch))].join(", ");
  const appIdValid = /^\d+$/.test(appId.trim());
  // Everything except the AppID (which the input supplies) must already be ready.
  const canApply = status.goldbergReady && appIdValid && !busy;

  return (
    <section className="panel">
      <h2 className="panel__title">
        Steam emulation (Goldberg)
        <span className="panel__hint">Offline play for delisted / Steam-gated PC games</span>
      </h2>

      <div className="notice notice--info">
        A Steam API DLL ({arches}) was detected for this game. Goldberg emulates
        Steamworks so it runs without the Steam client. The original DLL is backed
        up before any change, and Remove restores it.
      </div>

      {status.applied ? (
        <div className="notice notice--info">
          <strong>Goldberg is installed.</strong>{" "}
          {status.managed
            ? "Selene applied it and can revert cleanly."
            : "This was set up outside Selene, so there is no recorded backup to revert. Re-apply below to let Selene manage it."}
        </div>
      ) : (
        status.blockers.length > 0 &&
        status.blockers.map((b) => (
          <div className="notice notice--warn" key={b}>
            {b}
          </div>
        ))
      )}

      <div className="field">
        <label>Steam AppID</label>
        <div className="row">
          <input
            className="input"
            value={appId}
            onChange={(e) => setAppId(e.target.value)}
            placeholder="e.g. 243560"
            inputMode="numeric"
            spellCheck={false}
            disabled={busy}
          />
        </div>
        <span className="field__hint">
          {status.appId
            ? "Detected automatically; change it only if it's wrong."
            : "Find it on the game's SteamDB / store URL, then enter it here."}
        </span>
      </div>

      {status.applied && (
        <p className="field__hint">
          Player name for LAN: <strong>{status.accountName ?? "Goldberg (default)"}</strong>
          {" — "}set it in Settings → Steam emulation (applies to all Goldberg games).
        </p>
      )}

      {note && <div className="notice notice--info">{note}</div>}

      <div className="row">
        {status.applied && status.managed ? (
          <>
            <button className="btn btn--ghost" onClick={remove} disabled={busy}>
              {busy ? "● Working…" : "Remove Goldberg"}
            </button>
            <button className="btn btn--ghost" onClick={apply} disabled={!canApply}>
              Re-apply
            </button>
          </>
        ) : (
          <button className="btn btn--play" onClick={apply} disabled={!canApply}>
            {busy ? "● Working…" : status.applied ? "Manage with Selene" : "Apply Goldberg"}
          </button>
        )}
      </div>
    </section>
  );
}
