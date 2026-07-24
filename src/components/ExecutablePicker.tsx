import { useCallback, useEffect, useState } from "react";
import { api, inTauri, type ExeCandidate } from "../api";

interface Props {
  gameId: string;
}

/* Which executable a PC game launches.
   Detection is a heuristic — game folders are not standardised, and the
   largest binary is routinely a redistributable rather than the game — so the
   ranking is shown with its reasoning and can be overridden. */
export function ExecutablePicker({ gameId }: Props) {
  const [candidates, setCandidates] = useState<ExeCandidate[]>([]);
  const [override, setOverride] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    if (!inTauri) return;
    void api.listExecutables(gameId).then(setCandidates).catch(() => setCandidates([]));
    void api.getLaunchOverride(gameId).then(setOverride).catch(() => setOverride(null));
  }, [gameId]);

  useEffect(reload, [reload]);

  const choose = async (path: string | null) => {
    setBusy(true);
    try {
      await api.setLaunchOverride(gameId, path);
      setNote(path ? "Launch target set" : "Back to auto-detected");
      reload();
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
      setTimeout(() => setNote(null), 3500);
    }
  };

  // With no override the first candidate is what actually runs.
  const activePath = override ?? candidates[0]?.path ?? null;

  return (
    <section className="panel">
      <h2 className="panel__title">
        Executable
        <span className="panel__hint">What Play actually runs</span>
      </h2>

      {note && <div className="notice notice--info">{note}</div>}

      {candidates.length === 0 ? (
        <div className="notice notice--warn">
          No runnable executable found in this folder. Installers, crash
          handlers and redistributables are ignored, so a folder containing only
          those will show nothing here.
        </div>
      ) : (
        <>
          <div className="notice notice--info">
            Detected automatically. Size is deliberately ignored — the largest
            binary in a game folder is usually a Visual C++ redistributable, not
            the game.
          </div>
          <ul className="packlist">
            {candidates.map((c) => {
              const active = c.path === activePath;
              return (
                <li className="packlist__item" key={c.path} data-on={active}>
                  <label className="toggle">
                    <input
                      type="radio"
                      name={`exe-${gameId}`}
                      checked={active}
                      disabled={!inTauri || busy}
                      onChange={() => void choose(c.path)}
                    />
                    <span className="packlist__name">{c.fileName}</span>
                    <span className="checklist__state">{c.sizeMb.toFixed(1)} MB</span>
                  </label>
                  <div className="packlist__desc">
                    {c.reasons.length > 0 ? c.reasons.join(" · ") : "no strong signals"}
                  </div>
                  <div className="exepicker__path" data-selectable>
                    {c.path}
                  </div>
                </li>
              );
            })}
          </ul>
          {override && (
            <div className="row">
              <button
                className="btn btn--ghost"
                onClick={() => void choose(null)}
                disabled={busy}
              >
                Use auto-detected
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
