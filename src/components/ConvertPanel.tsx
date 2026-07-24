import { useEffect, useState } from "react";
import type { Game } from "../types";
import { api, inTauri, type ConvertInfo } from "../api";

interface Props {
  game: Game;
  onConverted: () => void;
}

/* Shown when a game's file format isn't readable by its runner but can be
   converted — e.g. Eden does not read .nsz, which is a compressed .nsp.

   Conversion is lossless decompression by the external `nsz` tool; the original
   file is kept, and on success the library entry repoints at the new file with
   its playtime, tags and artwork intact. */
export function ConvertPanel({ game, onConverted }: Props) {
  const [info, setInfo] = useState<ConvertInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri) return;
    void api.convertInfo(game.id).then(setInfo).catch(() => setInfo(null));
  }, [game.id, game.path]);

  // Only render for games that actually need conversion.
  if (!info?.needsConversion) return null;

  const from = (info.fromExt ?? "").toUpperCase();
  const to = (info.toExt ?? "").toLowerCase();

  const convert = async () => {
    setBusy(true);
    setNote(`Converting… large games take a few minutes. You can keep using the app.`);
    try {
      await api.convertGame(game.id);
      setNote(`Converted to .${to}. This title is now playable.`);
      onConverted();
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel">
      <h2 className="panel__title">
        Conversion needed
        <span className="panel__hint">This format isn&rsquo;t playable as-is</span>
      </h2>

      <div className="notice notice--warn">
        This game is a <strong>.{info.fromExt}</strong> file, which{" "}
        {game.runner === "eden" ? "Eden" : "its emulator"} cannot read — it will
        not appear in the emulator&rsquo;s own list or launch. {from} is a
        compressed <strong>.{to}</strong>; converting decompresses it losslessly.
        Your original file is kept.
      </div>

      {!info.converterAvailable ? (
        <div className="notice notice--warn">
          The <code>nsz</code> converter isn&rsquo;t installed. Install it with{" "}
          <code>pip install nsz</code>, then reopen this page.
        </div>
      ) : (
        <div className="notice notice--info">
          Converter found. Decompressing roughly doubles the file size (kept on
          the same drive), and uses your existing keys.
        </div>
      )}

      {note && <div className="notice notice--info">{note}</div>}

      <div className="row">
        <button
          className="btn btn--play"
          onClick={convert}
          disabled={!inTauri || busy || !info.converterAvailable}
        >
          {busy ? "● Converting…" : `Convert to .${to}`}
        </button>
      </div>
    </section>
  );
}
