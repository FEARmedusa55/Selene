/* Provides the active SocialClient plus the current session/profile to the UI.
 *
 * `socialClient` is the single swap point: it's the mock today; point it at a
 * Supabase-backed client (same SocialClient interface) once the project exists,
 * and nothing else in the UI changes. */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import type { Profile, Session, SocialClient } from "./types";
import { mockClient } from "./mockClient";
import { supabaseSocialClient } from "./supabaseClient";
import { isSupabaseConfigured } from "./config";
import { inTauri } from "../api";

/* Use the real backend when it's configured AND we're in the desktop shell (the
 * OAuth loopback needs the Rust side). Otherwise — no keys, or a plain browser
 * preview — fall back to the in-memory mock. */
export const socialClient: SocialClient =
  isSupabaseConfigured && inTauri ? supabaseSocialClient : mockClient;

interface SocialCtx {
  client: SocialClient;
  session: Session | null;
  me: Profile | null;
  loading: boolean;
  refreshMe: () => Promise<void>;
}

const Ctx = createContext<SocialCtx | null>(null);

export function SocialProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<Session | null>(null);
  const [me, setMe] = useState<Profile | null>(null);
  const [loading, setLoading] = useState(true);

  const refreshMe = useCallback(async () => {
    setMe(await socialClient.getMyProfile());
  }, []);

  useEffect(() => {
    let active = true;
    void socialClient.getSession().then((s) => {
      if (!active) return;
      setSession(s);
      setLoading(false);
    });
    const unsub = socialClient.onSessionChange(setSession);
    return () => {
      active = false;
      unsub();
    };
  }, []);

  useEffect(() => {
    if (session) void refreshMe();
    else setMe(null);
  }, [session, refreshMe]);

  return (
    <Ctx.Provider value={{ client: socialClient, session, me, loading, refreshMe }}>
      {children}
    </Ctx.Provider>
  );
}

export function useSocial(): SocialCtx {
  const c = useContext(Ctx);
  if (!c) throw new Error("useSocial must be used within a SocialProvider");
  return c;
}
