import { useEffect, useState } from "react";
import type { Game } from "../types";
import { api, inTauri, type AddonsView } from "../api";

interface Props {
  game: Game;
}

/* Updates and DLC for a game.

   Two honest boundaries are baked into the copy here:
   - We can convert the compressed (.nsz) add-ons — that is automated.
   - We cannot install them to Eden's NAND: that is a GUI-only step in Eden
     itself (its CLI exposes no install command), so the panel guides rather
     than pretends. */
export function AddonsPanel({ game }: Props) {
  const [view, setView] = useState<AddonsView | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const reload = () => {
    if (!inTauri) return;
    void api.listAddons(game.id).then(setView).catch(() => setView(null));
  };

  useEffect(reload, [game.id]);

  useEffect(() => {
    if (!inTauri) return;
    const un = api.onAddonProgress((done, total, name) => {
      setProgress(
        done >= total ? null : `Converting ${done + 1} of ${total}… ${name}`,
      );
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // Only Switch games have NAND add-ons.
  if (game.runner !== "eden") return null;
  if (!view || view.addons.length === 0) return null;

  const convertAll = async () => {
    setBusy(true);
    setNote(null);
    try {
      setNote(await api.convertAllAddons());
      reload();
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const updates = view.addons.filter((a) => a.kind === "update");
  const dlc = view.addons.filter((a) => a.kind === "dlc");

  const row = (a: (typeof view.addons)[number]) => (
    <li
      className="checklist__item"
      key={a.titleId}
      data-state={a.needsConversion ? "missing" : "ok"}
    >
      <span className="checklist__dot" />
      <span className="checklist__label">
        {a.name}
        {a.version != null && a.version > 0 && (
          <span className="checklist__sub"> — v{a.version}</span>
        )}
      </span>
      <span className="checklist__state">
        {a.needsConversion ? "needs conversion" : "ready to install"}
      </span>
    </li>
  );

  return (
    <section className="panel">
      <h2 className="panel__title">
        Updates &amp; DLC
        <span className="panel__hint">
          {view.addons.length} found in your updates folder
        </span>
      </h2>

      {view.needsConversion > 0 ? (
        <div className="notice notice--warn">
          {view.needsConversion} of these are compressed (<code>.nsz</code>) and
          must be decompressed before Eden can install them.
          {!view.converterAvailable && (
            <>
              {" "}
              The <code>nsz</code> converter isn&rsquo;t installed —{" "}
              <code>pip install nsz</code>.
            </>
          )}
        </div>
      ) : (
        <div className="notice notice--info">
          All add-ons are in an installable format.
        </div>
      )}

      {progress && <div className="notice notice--info">{progress}</div>}
      {note && <div className="notice notice--info">{note}</div>}

      {updates.length > 0 && (
        <>
          <div className="field__hint">Updates</div>
          <ul className="checklist">{updates.map(row)}</ul>
        </>
      )}
      {dlc.length > 0 && (
        <>
          <div className="field__hint">DLC</div>
          <ul className="checklist">{dlc.map(row)}</ul>
        </>
      )}

      <div className="row">
        {view.needsConversion > 0 && (
          <button
            className="btn btn--play"
            onClick={convertAll}
            disabled={!inTauri || busy || !view.converterAvailable}
          >
            {busy ? "● Converting…" : `Convert ${view.needsConversion} compressed add-on(s)`}
          </button>
        )}
        <button className="btn btn--ghost" onClick={() => void api.openAddonsDir()} disabled={busy}>
          Open updates folder
        </button>
      </div>

      <div className="notice notice--info">
        <strong>Installing:</strong> Eden applies updates and DLC only after
        they&rsquo;re installed to its NAND, which is a manual step —{" "}
        <strong>Eden → File → Install Files to NAND</strong>, then select the
        add-ons (open the folder above). This app can&rsquo;t automate that:
        Eden&rsquo;s command line has no install command.
      </div>
    </section>
  );
}
