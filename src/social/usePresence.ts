/* Publishes *your own* presence while signed in: an online heartbeat, plus
 * in-game status driven by the same game start/stop events the launcher already
 * emits. Privacy is enforced here, at the source — nothing you've hidden is ever
 * written to the row:
 *   - appear_offline or presence_visibility='nobody'  -> always 'offline'
 *   - show_game_titles=false                          -> 'in_game' with no title
 * Reading friends' presence lives in the Friends tab; this file only writes. */

import { useCallback, useEffect, useRef } from "react";
import type { Game } from "../types";
import { api, inTauri } from "../api";
import { useSocial } from "./SocialProvider";
import type { PresenceUpdate } from "./types";

const HEARTBEAT_MS = 60_000;

export function usePresencePublisher(games: Game[]) {
  const { client, session, me } = useSocial();

  // Latest values read through refs so the effects below don't churn.
  const gamesRef = useRef(games);
  gamesRef.current = games;
  const meRef = useRef(me);
  meRef.current = me;
  const current = useRef<{ inGame: boolean; title: string | null }>({ inGame: false, title: null });

  const publish = useCallback(() => {
    const m = meRef.current;
    if (!m) return;
    let update: PresenceUpdate;
    if (m.appearOffline || m.presenceVisibility === "nobody") {
      update = { status: "offline", gameTitle: null };
    } else if (current.current.inGame) {
      update = { status: "in_game", gameTitle: m.showGameTitles ? current.current.title : null };
    } else {
      update = { status: "online", gameTitle: null };
    }
    void client.publishPresence(update).catch(() => {});
  }, [client]);

  // Online heartbeat + game hooks while signed in. Keyed on session only, so a
  // profile edit doesn't tear this down (toggles are read via meRef).
  useEffect(() => {
    if (!session) return;
    publish();
    const beat = setInterval(publish, HEARTBEAT_MS);

    let started: Promise<() => void> | undefined;
    let stopped: Promise<() => void> | undefined;
    if (inTauri) {
      started = api.onGameStarted((gameId) => {
        const g = gamesRef.current.find((x) => x.id === gameId);
        current.current = { inGame: true, title: g?.title ?? null };
        publish();
      });
      stopped = api.onGameStopped(() => {
        current.current = { inGame: false, title: null };
        publish();
      });
    }

    return () => {
      clearInterval(beat);
      void started?.then((un) => un());
      void stopped?.then((un) => un());
      // Sign-out / unmount: go offline immediately (best effort).
      void client.publishPresence({ status: "offline", gameTitle: null }).catch(() => {});
    };
  }, [session, publish, client]);

  // Re-publish the moment privacy toggles (or the loaded profile) change.
  useEffect(() => {
    if (session && me) publish();
  }, [session, me, publish]);

  // Best-effort offline when the window is closing.
  useEffect(() => {
    const onUnload = () => void client.publishPresence({ status: "offline", gameTitle: null });
    window.addEventListener("beforeunload", onUnload);
    return () => window.removeEventListener("beforeunload", onUnload);
  }, [client]);
}
