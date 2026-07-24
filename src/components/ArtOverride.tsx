import { useEffect, useState } from "react";
import { api, inTauri } from "../api";

interface Props {
  gameId: string;
  hasCustomArt: boolean;
  onChanged: () => void;
}

/* Manual artwork override.
   IGDB has no curated hero-banner or transparent-logo assets, and its matches
   are occasionally wrong, so there has to be a way to set art by hand. The
   override is stored separately from the IGDB value, so "Revert" restores the
   fetched art without another lookup, and a later artwork refresh cannot
   silently overwrite the user's choice. */
export function ArtOverride({ gameId, hasCustomArt, onChanged }: Props) {
  const [cover, setCover] = useState("");
  const [hero, setHero] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!inTauri) return;
    void api
      .getArtOverride(gameId)
      .then(([c, h]) => {
        setCover(c ?? "");
        setHero(h ?? "");
      })
      .catch(() => {
        setCover("");
        setHero("");
      });
  }, [gameId]);

  const save = async (nextCover: string, nextHero: string) => {
    setBusy(true);
    try {
      await api.setArtOverride(gameId, nextCover || null, nextHero || null);
      setNote(nextCover || nextHero ? "Artwork updated" : "Reverted to IGDB artwork");
      onChanged();
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
      setTimeout(() => setNote(null), 3500);
    }
  };

  return (
    <section className="panel">
      <h2 className="panel__title">
        Artwork
        <span className="panel__hint">Override what IGDB found</span>
      </h2>

      <div className="notice notice--info">
        Paste an image URL or a full path to a local file. IGDB carries no
        hero-banner or logo assets, so anything it cannot supply goes here. Your
        choice is kept separately, so “Get artwork” will not overwrite it.
      </div>
      {note && <div className="notice notice--info">{note}</div>}

      <div className="field">
        <label>Cover (portrait, 3:4)</label>
        <input
          className="input"
          value={cover}
          onChange={(e) => setCover(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void save(cover, hero)}
          placeholder="https://…  or  D:\art\cover.png"
          spellCheck={false}
          disabled={!inTauri || busy}
        />
      </div>

      <div className="field">
        <label>Hero / background (wide)</label>
        <input
          className="input"
          value={hero}
          onChange={(e) => setHero(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void save(cover, hero)}
          placeholder="https://…  or  D:\art\hero.jpg"
          spellCheck={false}
          disabled={!inTauri || busy}
        />
      </div>

      <div className="row">
        <button
          className="btn btn--ghost"
          onClick={() => void save(cover, hero)}
          disabled={!inTauri || busy}
        >
          Save artwork
        </button>
        <button
          className="btn btn--ghost"
          onClick={() => {
            setCover("");
            setHero("");
            void save("", "");
          }}
          disabled={!inTauri || busy || !hasCustomArt}
          title={hasCustomArt ? undefined : "No manual artwork set"}
        >
          Revert to IGDB
        </button>
      </div>
    </section>
  );
}
