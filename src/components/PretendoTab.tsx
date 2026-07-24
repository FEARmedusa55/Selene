import { useCallback, useEffect, useState } from "react";
import { api, inTauri, type PretendoStatus } from "../api";

/* Pretendo tab.

   This app does not reimplement Pretendo — Cemu has built-in support. What is
   here is configuration plus an honest account of what is missing, because the
   alternative failure mode is a game that silently connects to nothing. */
export function PretendoTab() {
  const [status, setStatus] = useState<PretendoStatus | null>(null);
  const [pnidDraft, setPnidDraft] = useState<Record<string, string>>({});
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    if (!inTauri) return;
    void api
      .pretendoStatus()
      .then((s) => {
        setStatus(s);
        setPnidDraft(
          Object.fromEntries(s.accounts.map((a) => [a.persistentId, a.accountId])),
        );
      })
      .catch((e) => setNote(String(e)));
  }, []);

  useEffect(reload, [reload]);

  const flash = (msg: string) => {
    setNote(msg);
    setTimeout(() => setNote(null), 4500);
  };

  const setService = async (pretendo: boolean) => {
    setBusy(true);
    try {
      await api.setPretendoService(pretendo ? "pretendo" : "nintendo", pretendo);
      flash(pretendo ? "Cemu switched to Pretendo" : "Cemu switched back to Nintendo Network");
      reload();
    } catch (e) {
      flash(String(e));
    } finally {
      setBusy(false);
    }
  };

  const savePnid = async (persistentId: string) => {
    setBusy(true);
    try {
      await api.setPretendoAccountId(persistentId, pnidDraft[persistentId] ?? "");
      flash("PNID saved. Set the password in Cemu's own account settings.");
      reload();
    } catch (e) {
      flash(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!inTauri) {
    return (
      <div className="page">
        <h1 className="page__title">Pretendo</h1>
        <div className="notice notice--warn">
          Running outside the desktop shell, so Cemu cannot be inspected.
        </div>
      </div>
    );
  }

  if (!status?.cemuConfigured) {
    return (
      <div className="page">
        <h1 className="page__title">Pretendo</h1>
        <div className="notice notice--warn">
          Cemu is not configured. Set its executable under Settings → Emulators
          first; everything on this page is read from Cemu&rsquo;s own files.
        </div>
      </div>
    );
  }

  const onPretendo = status.service === "pretendo";
  const missing = status.files.filter((f) => !f.present);

  return (
    <div className="page">
      <h1 className="page__title">Pretendo</h1>

      <div className="notice notice--warn">
        <strong>Never use the same PNID on a real Wii U and Cemu at the same time.</strong>{" "}
        Simultaneous sessions on one account can get it banned from the network.
      </div>

      {note && <div className="notice notice--info">{note}</div>}

      {status.cemuRunning && (
        <div className="notice notice--warn">
          Cemu is running. It rewrites <code>settings.xml</code> when it closes,
          so changes made here would be discarded — close Cemu first.
        </div>
      )}

      <section className="panel">
        <h2 className="panel__title">
          Network service
          <span className="panel__hint">Written to Cemu&rsquo;s settings.xml</span>
        </h2>

        <div className="statlist">
          <div className="stat">
            <dt>Current service</dt>
            <dd>
              {status.service === "pretendo"
                ? "Pretendo"
                : status.service === "custom"
                  ? "Custom"
                  : "Nintendo Network"}
            </dd>
          </div>
          <div className="stat">
            <dt>Online</dt>
            <dd>{status.onlineEnabled ? "Enabled" : "Disabled"}</dd>
          </div>
        </div>

        {!status.filesComplete && (
          <div className="notice notice--warn">
            {missing.length} required file{missing.length === 1 ? "" : "s"} still
            missing. You can switch anyway, but online will not work until the
            list below is complete.
          </div>
        )}

        <div className="row">
          <button
            className="btn btn--play"
            onClick={() => void setService(true)}
            disabled={busy || onPretendo || status.cemuRunning}
          >
            {onPretendo ? "Using Pretendo" : "Switch to Pretendo"}
          </button>
          <button
            className="btn btn--ghost"
            onClick={() => void setService(false)}
            disabled={busy || !onPretendo || status.cemuRunning}
          >
            Back to Nintendo Network
          </button>
        </div>
      </section>

      <section className="panel">
        <h2 className="panel__title">
          Accounts
          <span className="panel__hint">Read from Cemu&rsquo;s mlc01 storage</span>
        </h2>

        {status.accounts.length === 0 ? (
          <div className="notice notice--warn">
            No Cemu accounts found. Create one in Cemu first.
          </div>
        ) : (
          status.accounts.map((a) => (
            <div className="field" key={a.persistentId}>
              <label>
                {a.miiName || "(no Mii name)"} — {a.persistentId}
                {a.isActive && " · active"}
                {a.principalId === 0 && " · not linked to a network ID"}
              </label>
              <div className="row">
                <input
                  className="input"
                  value={pnidDraft[a.persistentId] ?? ""}
                  placeholder="Your Pretendo Network ID"
                  spellCheck={false}
                  disabled={busy || status.cemuRunning}
                  onChange={(e) =>
                    setPnidDraft((d) => ({ ...d, [a.persistentId]: e.target.value }))
                  }
                  onKeyDown={(e) => e.key === "Enter" && void savePnid(a.persistentId)}
                />
                <button
                  className="btn btn--ghost"
                  onClick={() => void savePnid(a.persistentId)}
                  disabled={busy || status.cemuRunning}
                >
                  Save
                </button>
              </div>
            </div>
          ))
        )}

        <div className="notice notice--info">
          Only the PNID username is set here. The password is stored by Cemu as a
          derived value, so set it in <strong>Cemu → Options → Account</strong> —
          writing it from outside risks producing something Cemu cannot use.
        </div>
      </section>

      <section className="panel">
        <h2 className="panel__title">
          Required files
          <span className="panel__hint">Must be dumped from a Wii U you own</span>
        </h2>

        <div className={status.filesComplete ? "notice notice--info" : "notice notice--warn"}>
          {status.filesComplete
            ? "All required files are present."
            : "Pretendo needs online support files dumped from real hardware. This app bundles and downloads nothing — you supply your own dump."}
        </div>

        <ul className="checklist">
          {status.files.map((f) => (
            <li
              className="checklist__item"
              key={f.relativePath}
              data-state={f.present ? "ok" : "missing"}
            >
              <span className="checklist__dot" />
              <span className="checklist__label">
                {f.label}
                <span className="checklist__sub"> — {f.relativePath}</span>
              </span>
              <span className="checklist__state">{f.detail}</span>
            </li>
          ))}
        </ul>

        <div className="pathrow">
          <span className="pathrow__label">Cemu data</span>
          <code className="pathrow__value" data-selectable>
            {status.dataDir}
          </code>
        </div>
      </section>
    </div>
  );
}
