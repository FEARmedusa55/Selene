import type { Game, SortKey } from "../types";
import { PLATFORM_LABELS } from "../types";
import { formatLastPlayed, formatPlaytimeShort } from "../format";
import { CoverArt } from "./CoverArt";

interface Props {
  games: Game[];
  onOpen: (id: string) => void;
  sort: SortKey;
  onSortChange: (s: SortKey) => void;
  total: number;
  selectedId?: string;
  runningId?: string;
}

const SORT_LABELS: Record<SortKey, string> = {
  title: "Alphabetical",
  lastPlayed: "Last played",
  playtime: "Playtime",
  added: "Recently added",
};

export function GameGrid({
  games,
  onOpen,
  sort,
  onSortChange,
  total,
  selectedId,
  runningId,
}: Props) {
  return (
    <div className="grid-view">
      <header className="grid-view__bar">
        <div className="grid-view__count">
          {games.length === total ? `${total} games` : `${games.length} of ${total} games`}
        </div>
        <label className="grid-view__sort">
          <span className="grid-view__sort-label">Sort by</span>
          <select
            className="select"
            value={sort}
            onChange={(e) => onSortChange(e.target.value as SortKey)}
          >
            {(Object.keys(SORT_LABELS) as SortKey[]).map((k) => (
              <option key={k} value={k}>
                {SORT_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
      </header>

      {games.length === 0 ? (
        <div className="empty">
          <div className="empty__title">No games match those filters</div>
          <div className="empty__body">Clear a filter or widen your search.</div>
        </div>
      ) : (
        <ul className="grid">
          {games.map((g) => (
            <li key={g.id}>
              <button
                className="card"
                data-selected={g.id === selectedId}
                onClick={() => onOpen(g.id)}
              >
                <CoverArt title={g.title} src={g.coverUrl} variant="cover" />

                <span className="card__badges">
                  {(g.running || g.id === runningId) && (
                    <span className="card__running" aria-label="Running" />
                  )}
                  {g.favorite && (
                    <span className="card__fav" aria-label="Favorite">
                      ★
                    </span>
                  )}
                </span>

                <span className="card__overlay">
                  <span className="card__title">{g.title}</span>
                  <span className="card__meta">
                    <span>{PLATFORM_LABELS[g.platform]}</span>
                    <span className="card__meta-sep">•</span>
                    <span>
                      {g.playtimeSeconds > 0
                        ? formatPlaytimeShort(g.playtimeSeconds)
                        : "Unplayed"}
                    </span>
                    {g.lastPlayedAt && (
                      <>
                        <span className="card__meta-sep">•</span>
                        <span>{formatLastPlayed(g.lastPlayedAt)}</span>
                      </>
                    )}
                  </span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
