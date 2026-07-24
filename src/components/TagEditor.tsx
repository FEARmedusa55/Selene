import { useEffect, useRef, useState } from "react";
import { api, inTauri } from "../api";

interface Props {
  gameId: string;
  tags: string[];
  onChanged: () => void;
}

/* Tag editing for one game.
   Existing tags are offered as suggestions rather than left to free text --
   without them a library accumulates "Co-op", "co op" and "Coop" as three
   separate entries in the sidebar. (The backend also folds case, so the two
   defences are independent.) */
export function TagEditor({ gameId, tags, onChanged }: Props) {
  const [all, setAll] = useState<[string, number][]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!inTauri) return;
    void api.listTags().then(setAll).catch(() => setAll([]));
  }, [gameId, tags.length]);

  const commit = async (name: string) => {
    const clean = name.trim();
    if (!clean || busy) return;
    setBusy(true);
    try {
      await api.addTag(gameId, clean);
      setDraft("");
      onChanged();
    } finally {
      setBusy(false);
      inputRef.current?.focus();
    }
  };

  const drop = async (name: string) => {
    setBusy(true);
    try {
      await api.removeTag(gameId, name);
      onChanged();
    } finally {
      setBusy(false);
    }
  };

  // Suggest tags used elsewhere but not on this game.
  const suggestions = all
    .map(([name]) => name)
    .filter((n) => !tags.some((t) => t.toLowerCase() === n.toLowerCase()))
    .filter((n) => !draft || n.toLowerCase().includes(draft.trim().toLowerCase()))
    .slice(0, 8);

  return (
    <div className="tageditor">
      <div className="tageditor__current">
        {tags.length === 0 && <span className="tageditor__empty">No tags yet</span>}
        {tags.map((t) => (
          <span className="tagchip" key={t}>
            {t}
            <button
              className="tagchip__x"
              onClick={() => void drop(t)}
              aria-label={`Remove tag ${t}`}
              disabled={!inTauri || busy}
            >
              ×
            </button>
          </span>
        ))}
      </div>

      <div className="row">
        <input
          ref={inputRef}
          className="input"
          value={draft}
          placeholder="Add a tag…"
          disabled={!inTauri || busy}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void commit(draft);
            if (e.key === "Escape") setDraft("");
          }}
        />
        <button
          className="btn btn--ghost"
          onClick={() => void commit(draft)}
          disabled={!inTauri || busy || !draft.trim()}
        >
          Add
        </button>
      </div>

      {suggestions.length > 0 && (
        <div className="tageditor__suggest">
          <span className="tageditor__suggest-label">Existing:</span>
          {suggestions.map((s) => (
            <button
              key={s}
              className="tagchip tagchip--suggest"
              onClick={() => void commit(s)}
              disabled={busy}
            >
              + {s}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
