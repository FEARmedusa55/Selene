import { useState } from "react";
import type { Game, LibraryFilter, Platform } from "../types";
import { PLATFORM_LABELS } from "../types";
import { formatPlaytimeShort } from "../format";

interface Props {
  /** Games matching the current filter — shown in the title list. */
  games: Game[];
  /** The whole library. Filter controls are built from this, never from the
   *  filtered set: deriving them from `games` made a filter erase the very
   *  chips needed to undo it (selecting Favorites removed every tag that no
   *  favorite happened to carry). */
  allGames: Game[];
  filter: LibraryFilter;
  onFilterChange: (next: LibraryFilter) => void;
  selectedId?: string;
  onSelect: (id: string) => void;
  collapsed: boolean;
}

const PLATFORM_ORDER: Platform[] = ["wiiu", "wii", "gamecube", "switch", "pc"];

type SectionId = "platforms" | "collections" | "all";

function SearchIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="7" cy="7" r="4.5" stroke="currentColor" strokeWidth="1.6" />
      <path d="M10.5 10.5L14 14" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

/** Collapsible category header. The chevron rotates rather than swapping glyphs
 *  so the state change reads as motion instead of a flicker. */
function SectionHeader({
  label,
  count,
  open,
  onToggle,
}: {
  label: string;
  count?: number;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <button className="sidebar__heading" data-open={open} onClick={onToggle} aria-expanded={open}>
      <span className="sidebar__chevron">▼</span>
      <span>{label}</span>
      {count !== undefined && <span className="sidebar__count">{count}</span>}
    </button>
  );
}

/* Steam-like left rail: search, collapsible platform/collection filters, then
   the flat title list. Collapsing the whole rail is handled by the parent via a
   CSS width transition so the list keeps its scroll position across toggles. */
export function Sidebar({
  games,
  allGames,
  filter,
  onFilterChange,
  selectedId,
  onSelect,
  collapsed,
}: Props) {
  const [open, setOpen] = useState<Record<SectionId, boolean>>({
    platforms: true,
    collections: true,
    all: true,
  });

  const toggleSection = (id: SectionId) => setOpen((o) => ({ ...o, [id]: !o[id] }));

  // Counts and chip lists come from the full library so they stay stable while
  // a filter is applied.
  const counts = new Map<Platform, number>();
  for (const g of allGames) counts.set(g.platform, (counts.get(g.platform) ?? 0) + 1);

  const presentPlatforms = PLATFORM_ORDER.filter((p) => counts.has(p));
  const allTags = [...new Set(allGames.flatMap((g) => g.tags))].sort((a, b) =>
    a.localeCompare(b),
  );

  /** No filter of any kind is active. */
  const showingEverything =
    filter.platforms.length === 0 && filter.tags.length === 0 && !filter.favoritesOnly;

  /** "All" clears every filter, not just platforms — it was previously
   *  resetting platforms alone, leaving a tag or Favorites still applied with
   *  no obvious way back. Search is deliberately left alone. */
  const clearFilters = () =>
    onFilterChange({ ...filter, platforms: [], tags: [], favoritesOnly: false });

  const togglePlatform = (p: Platform) => {
    const has = filter.platforms.includes(p);
    onFilterChange({
      ...filter,
      platforms: has ? filter.platforms.filter((x) => x !== p) : [...filter.platforms, p],
    });
  };

  const toggleTag = (t: string) => {
    const has = filter.tags.includes(t);
    onFilterChange({
      ...filter,
      tags: has ? filter.tags.filter((x) => x !== t) : [...filter.tags, t],
    });
  };

  return (
    <aside className="sidebar" data-collapsed={collapsed} aria-hidden={collapsed}>
      <div className="sidebar__inner">
        <div className="search">
          <span className="search__icon">
            <SearchIcon />
          </span>
          <input
            type="search"
            className="search__input"
            placeholder="Search library"
            value={filter.search}
            onChange={(e) => onFilterChange({ ...filter, search: e.target.value })}
          />
        </div>

        <nav className="sidebar__section">
          <SectionHeader
            label="Platforms"
            open={open.platforms}
            onToggle={() => toggleSection("platforms")}
          />
          <div className="sidebar__collapse" data-open={open.platforms}>
            <div>
              <button
                className="chip"
                data-active={showingEverything}
                onClick={clearFilters}
              >
                <span>All</span>
                <span className="chip__count">{allGames.length}</span>
              </button>
              {presentPlatforms.map((p) => (
                <button
                  key={p}
                  className="chip"
                  data-active={filter.platforms.includes(p)}
                  onClick={() => togglePlatform(p)}
                >
                  <span>{PLATFORM_LABELS[p]}</span>
                  <span className="chip__count">{counts.get(p)}</span>
                </button>
              ))}
            </div>
          </div>
        </nav>

        <nav className="sidebar__section">
          <SectionHeader
            label="Collections"
            open={open.collections}
            onToggle={() => toggleSection("collections")}
          />
          <div className="sidebar__collapse" data-open={open.collections}>
            <div>
              <button
                className="chip"
                data-active={filter.favoritesOnly}
                onClick={() => onFilterChange({ ...filter, favoritesOnly: !filter.favoritesOnly })}
              >
                <span>Favorites</span>
                <span className="chip__count">
                  {allGames.filter((g) => g.favorite).length}
                </span>
              </button>
              {allTags.map((t) => (
                <button
                  key={t}
                  className="chip"
                  data-active={filter.tags.includes(t)}
                  onClick={() => toggleTag(t)}
                >
                  <span>{t}</span>
                  <span className="chip__count">
                    {allGames.filter((g) => g.tags.includes(t)).length}
                  </span>
                </button>
              ))}
            </div>
          </div>
        </nav>

        <div className="sidebar__section sidebar__section--grow">
          <SectionHeader
            label="All games"
            count={games.length}
            open={open.all}
            onToggle={() => toggleSection("all")}
          />
          <div className="sidebar__collapse" data-open={open.all}>
            <div>
              <ul className="titlelist">
                {games.map((g) => (
                  <li key={g.id}>
                    <button
                      className="titlelist__row"
                      data-selected={g.id === selectedId}
                      onClick={() => onSelect(g.id)}
                      title={g.title}
                    >
                      <span className="titlelist__name">{g.title}</span>
                      <span className="titlelist__meta">
                        {formatPlaytimeShort(g.playtimeSeconds)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </div>
    </aside>
  );
}
