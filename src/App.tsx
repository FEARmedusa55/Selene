import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Game, LibraryFilter, SortKey } from "./types";
import { api, inTauri } from "./api";
import { MOCK_GAMES } from "./mock";
import { applyTheme, BUILTIN_THEMES, loadStoredTheme, registerUserThemes, type ThemeMeta } from "./theme";
import { startGamepad } from "./gamepad";
import { Sidebar } from "./components/Sidebar";
import { GameGrid } from "./components/GameGrid";
import { GamePage } from "./components/GamePage";
import { SettingsTab } from "./components/SettingsTab";
import { PretendoTab } from "./components/PretendoTab";

type Tab = "library" | "pretendo" | "settings";

const TABS: { id: Tab; label: string }[] = [
  { id: "library", label: "Library" },
  { id: "pretendo", label: "Pretendo" },
  { id: "settings", label: "Settings" },
];

const EMPTY_FILTER: LibraryFilter = {
  search: "",
  platforms: [],
  tags: [],
  favoritesOnly: false,
};

export default function App() {
  const [theme, setTheme] = useState(loadStoredTheme);
  const [tab, setTab] = useState<Tab>("library");
  const [games, setGames] = useState<Game[]>([]);
  const [filter, setFilter] = useState<LibraryFilter>(EMPTY_FILTER);
  const [sort, setSort] = useState<SortKey>("title");
  const [openGameId, setOpenGameId] = useState<string | undefined>();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [runningId, setRunningId] = useState<string | undefined>();
  const [userThemes, setUserThemes] = useState<ThemeMeta[]>([]);

  const allThemes = useMemo(() => [...BUILTIN_THEMES, ...userThemes], [userThemes]);

  useEffect(() => applyTheme(theme), [theme]);

  // Load user-authored themes and inject their token CSS.
  const reloadThemes = useCallback(async () => {
    if (!inTauri) return;
    try {
      setUserThemes(registerUserThemes(await api.listUserThemes()));
    } catch (e) {
      console.error("failed to load user themes", e);
    }
  }, []);

  useEffect(() => {
    void reloadThemes();
  }, [reloadThemes]);

  // Gamepad navigation. The poll loop is started once; handlers read the latest
  // state through a ref so the loop never has to restart.
  const navRef = useRef({ onBack: () => {}, onTab: (_d: number) => {} });
  navRef.current.onBack = () => setOpenGameId(undefined);
  navRef.current.onTab = (delta: number) => {
    const i = TABS.findIndex((t) => t.id === tab);
    const next = TABS[(i + delta + TABS.length) % TABS.length];
    setTab(next.id);
    setOpenGameId(undefined);
  };
  useEffect(
    () =>
      startGamepad({
        onBack: () => navRef.current.onBack(),
        onTab: (d) => navRef.current.onTab(d),
      }),
    [],
  );

  const refresh = useCallback(async () => {
    // Outside the Tauri shell (plain `vite dev` in a browser) there is no core
    // to call, so fall back to fixtures rather than rendering an empty page.
    if (!inTauri) {
      setGames(MOCK_GAMES);
      return;
    }
    try {
      setGames(await api.listGames());
    } catch (e) {
      console.error("failed to load games", e);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!inTauri) return;
    const started = api.onGameStarted(setRunningId);
    const stopped = api.onGameStopped(() => {
      setRunningId(undefined);
      void refresh();
    });
    return () => {
      void started.then((un) => un());
      void stopped.then((un) => un());
    };
  }, [refresh]);

  const runScan = async () => {
    setBusy("Scanning library…");
    try {
      const report = await api.scanLibrary();
      await refresh();
      setBusy(
        report.found === 0
          ? "No games found — add a scan folder in Settings."
          : `Found ${report.found} (${report.inserted} new)`,
      );
    } catch (e) {
      setBusy(`Scan failed: ${e}`);
    } finally {
      setTimeout(() => setBusy(null), 4000);
    }
  };

  const runFetchArtwork = async () => {
    setBusy("Fetching artwork…");
    try {
      const report = await api.fetchArtwork();
      await refresh();
      setBusy(
        report.unmatched.length
          ? `Resolved ${report.resolved}; ${report.unmatched.length} need manual art`
          : `Resolved artwork for ${report.resolved} games`,
      );
    } catch (e) {
      setBusy(`Artwork failed: ${e}`);
    } finally {
      setTimeout(() => setBusy(null), 5000);
    }
  };

  const play = async (gameId: string) => {
    try {
      await api.playGame(gameId);
      await refresh();
    } catch (e) {
      setBusy(`Launch failed: ${e}`);
      setTimeout(() => setBusy(null), 6000);
    }
  };

  const toggleFavorite = async (id: string) => {
    const game = games.find((g) => g.id === id);
    if (!game) return;
    // Optimistic: the toggle should feel instant.
    setGames((prev) => prev.map((g) => (g.id === id ? { ...g, favorite: !g.favorite } : g)));
    if (inTauri) {
      try {
        await api.setFavorite(id, !game.favorite);
      } catch {
        void refresh();
      }
    }
  };

  const visible = useMemo(() => {
    const q = filter.search.trim().toLowerCase();
    const filtered = games.filter((g) => {
      if (q && !g.title.toLowerCase().includes(q)) return false;
      if (filter.platforms.length && !filter.platforms.includes(g.platform)) return false;
      if (filter.tags.length && !filter.tags.some((t) => g.tags.includes(t))) return false;
      if (filter.favoritesOnly && !g.favorite) return false;
      return true;
    });

    const sorted = [...filtered];
    sorted.sort((a, b) => {
      switch (sort) {
        case "playtime":
          return b.playtimeSeconds - a.playtimeSeconds;
        case "lastPlayed":
          // Never-played titles fall to the bottom rather than masquerading
          // as the oldest entries.
          return (
            new Date(b.lastPlayedAt ?? 0).getTime() - new Date(a.lastPlayedAt ?? 0).getTime()
          );
        case "added":
          return new Date(b.addedAt).getTime() - new Date(a.addedAt).getTime();
        default:
          return a.title.localeCompare(b.title);
      }
    });
    return sorted;
  }, [games, filter, sort]);

  const openGame = games.find((g) => g.id === openGameId);

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar__left">
          {tab === "library" && (
            <button
              className="iconbtn"
              onClick={() => setSidebarCollapsed((c) => !c)}
              title={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
              aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
            >
              ☰
            </button>
          )}
          <nav className="tabs">
            {TABS.map((t) => (
              <button
                key={t.id}
                className="tab"
                data-active={tab === t.id}
                onClick={() => {
                  setTab(t.id);
                  setOpenGameId(undefined);
                }}
              >
                {t.label}
              </button>
            ))}
          </nav>
        </div>
        <div className="topbar__right">
          {busy && <span className="topbar__status">{busy}</span>}
          {tab === "library" && inTauri && (
            <>
              <button className="btn btn--small" onClick={runScan}>
                Scan
              </button>
              <button className="btn btn--small" onClick={runFetchArtwork}>
                Get artwork
              </button>
            </>
          )}
          <span className="topbar__brand">Selene</span>
        </div>
      </header>

      <div className="app__body">
        {tab === "library" && (
          <>
            <Sidebar
              games={visible}
              allGames={games}
              filter={filter}
              onFilterChange={setFilter}
              selectedId={openGameId}
              onSelect={setOpenGameId}
              collapsed={sidebarCollapsed}
            />
            <main className="content">
              {openGame ? (
                <GamePage
                  game={openGame}
                  onBack={() => setOpenGameId(undefined)}
                  onToggleFavorite={toggleFavorite}
                  onPlay={play}
                  running={runningId === openGame.id}
                  onChanged={refresh}
                />
              ) : games.length === 0 ? (
                <div className="empty">
                  <div className="empty__title">Your library is empty</div>
                  <div className="empty__body">
                    Add a scan folder in Settings, then press Scan.
                  </div>
                </div>
              ) : (
                <GameGrid
                  games={visible}
                  total={games.length}
                  onOpen={setOpenGameId}
                  sort={sort}
                  onSortChange={setSort}
                  selectedId={openGameId}
                  runningId={runningId}
                />
              )}
            </main>
          </>
        )}

        {tab === "pretendo" && (
          <main className="content">
            <PretendoTab />
          </main>
        )}

        {tab === "settings" && (
          <main className="content">
            <SettingsTab
              theme={theme}
              onThemeChange={setTheme}
              onLibraryChanged={refresh}
              themes={allThemes}
              onReloadThemes={reloadThemes}
            />
          </main>
        )}
      </div>
    </div>
  );
}
