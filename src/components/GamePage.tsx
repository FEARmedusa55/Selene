import { useEffect, useState } from "react";
import type { Game } from "../types";
import { PLATFORM_LABELS, RUNNER_LABELS } from "../types";
import { formatLastPlayed, formatPlaytime } from "../format";
import { CoverArt } from "./CoverArt";
import { RunnerConfig } from "./RunnerConfig";
import { TagEditor } from "./TagEditor";
import { ArtOverride } from "./ArtOverride";
import { ExecutablePicker } from "./ExecutablePicker";
import { ConvertPanel } from "./ConvertPanel";
import { AddonsPanel } from "./AddonsPanel";
import { GoldbergPanel } from "./GoldbergPanel";
import { api, inTauri, type EdenRequirements } from "../api";

interface Props {
  game: Game;
  onBack: () => void;
  onToggleFavorite: (id: string) => void;
  onPlay: (id: string) => void;
  running: boolean;
  /** Reload the library after tags or artwork change. */
  onChanged: () => void;
}

export function GamePage({
  game,
  onBack,
  onToggleFavorite,
  onPlay,
  running,
  onChanged,
}: Props) {
  const isWiiU = game.platform === "wiiu";

  const [edenReq, setEdenReq] = useState<EdenRequirements | null>(null);

  useEffect(() => {
    if (!inTauri || game.runner !== "eden") return;
    void api.edenRequirements().then(setEdenReq).catch(() => setEdenReq(null));
  }, [game.runner]);

  // Firmware is a caveat, not a blocker: games run on prod.keys alone. What it
  // costs you is system applets, which fail partway through a game rather than
  // at launch -- so it is worth mentioning, quietly.
  const missingKeys = game.runner === "eden" && edenReq !== null && !edenReq.prodKeys;
  const noApplets = game.runner === "eden" && edenReq !== null && !edenReq.firmware;

  // A compressed container the runner can't read yet. Cheap to detect from the
  // extension, so the Play button can steer to Convert without a round trip.
  const needsConversion = /\.(nsz|xcz)$/i.test(game.path);

  return (
    <div className="gamepage">
      <div className="gamepage__hero">
        <CoverArt title={game.title} src={game.heroUrl} variant="hero" />
        <div className="gamepage__scrim" />
        <button className="gamepage__back" onClick={onBack}>
          ← Library
        </button>

        <div className="gamepage__heroinner">
          <CoverArt title={game.title} src={game.coverUrl} className="gamepage__cover" />
          <div className="gamepage__headline">
            <h1 className="gamepage__title">{game.title}</h1>
            <div className="gamepage__badges">
              <span className="badge">{PLATFORM_LABELS[game.platform]}</span>
              <span className="badge badge--muted">via {RUNNER_LABELS[game.runner]}</span>
              {game.titleId && <span className="badge badge--mono">{game.titleId}</span>}
            </div>
            <div className="gamepage__actions">
              <button
                className="btn btn--play"
                onClick={() => onPlay(game.id)}
                disabled={running || !inTauri || needsConversion}
                title={
                  needsConversion
                    ? "This format must be converted before it can run — see below"
                    : inTauri
                      ? undefined
                      : "Launching requires the desktop app"
                }
              >
                {running ? "● Running" : needsConversion ? "Needs conversion" : "▶ Play"}
              </button>
              <button
                className="btn btn--ghost"
                onClick={() => onToggleFavorite(game.id)}
                aria-pressed={game.favorite}
              >
                {game.favorite ? "★ Favorited" : "☆ Favorite"}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div className="gamepage__body">
        <ConvertPanel game={game} onConverted={onChanged} />

        <section className="panel">
          <h2 className="panel__title">Activity</h2>
          <dl className="statlist">
            <div className="stat">
              <dt>Play time</dt>
              <dd>{formatPlaytime(game.playtimeSeconds)}</dd>
            </div>
            <div className="stat">
              <dt>Last played</dt>
              <dd>{formatLastPlayed(game.lastPlayedAt)}</dd>
            </div>
            <div className="stat">
              <dt>Runner</dt>
              <dd>{RUNNER_LABELS[game.runner]}</dd>
            </div>
          </dl>
          <div className="pathrow">
            <span className="pathrow__label">File</span>
            <code className="pathrow__value" data-selectable>
              {game.path}
            </code>
          </div>
        </section>

        {(missingKeys || noApplets) && (
          <section className="panel">
            <h2 className="panel__title">Eden requirements</h2>
            {missingKeys ? (
              <div className="notice notice--warn">
                <strong>prod.keys is missing.</strong> Nothing will run without
                it. Keys must be dumped from a Switch you own — this app supplies
                none.
              </div>
            ) : (
              <div className="notice notice--info">
                Firmware is not installed. Games still run on your keys alone;
                what is unavailable is <strong>system applets</strong> — the
                on-screen keyboard, profile select, error dialogs — and amiibo.
                Titles that call one will fail at that moment rather than at
                launch.
              </div>
            )}
            <ul className="checklist">
              <li className="checklist__item" data-state={edenReq?.prodKeys ? "ok" : "missing"}>
                <span className="checklist__dot" />
                <span className="checklist__label">prod.keys</span>
                <span className="checklist__state">
                  {edenReq?.prodKeys ? "Installed" : "Missing"}
                </span>
              </li>
              <li className="checklist__item" data-state={edenReq?.titleKeys ? "ok" : "missing"}>
                <span className="checklist__dot" />
                <span className="checklist__label">title.keys</span>
                <span className="checklist__state">
                  {edenReq?.titleKeys ? "Installed" : "Missing"}
                </span>
              </li>
              <li className="checklist__item" data-state={edenReq?.firmware ? "ok" : "missing"}>
                <span className="checklist__dot" />
                <span className="checklist__label">System firmware</span>
                <span className="checklist__state">
                  {edenReq?.firmware
                    ? `${edenReq.firmwareTitleCount} titles`
                    : "Not installed"}
                </span>
              </li>
            </ul>
          </section>
        )}

        <section className="panel">
          <h2 className="panel__title">
            Tags
            <span className="panel__hint">Group titles however you like</span>
          </h2>
          <TagEditor gameId={game.id} tags={game.tags} onChanged={onChanged} />
        </section>

        <ArtOverride
          gameId={game.id}
          hasCustomArt={game.customArt ?? false}
          onChanged={onChanged}
        />

        {game.runner === "native-pc" && <ExecutablePicker gameId={game.id} />}

        <AddonsPanel game={game} />

        <RunnerConfig game={game} />

        {isWiiU && (
          <section className="panel">
            <h2 className="panel__title">Pretendo</h2>
            <div className="notice notice--warn">
              Requires online files dumped from a real Wii U, installed into Cemu.
              Never use the same PNID on a console and Cemu at the same time.
            </div>
            <label className="toggle">
              <input type="checkbox" disabled />
              <span>Use Pretendo network service for this title</span>
            </label>
          </section>
        )}

        {game.runner === "native-pc" && <GoldbergPanel game={game} />}
      </div>
    </div>
  );
}
