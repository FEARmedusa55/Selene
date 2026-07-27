/* The real SocialClient, backed by Supabase. Same interface as the mock, so the
 * Friends UI is unchanged — SocialProvider just points here when configured.
 *
 * Auth is Discord OAuth over a loopback redirect: we open Discord in the system
 * browser and the Rust `oauth_capture` command catches the callback (see
 * lib.rs). Everything else is RLS'd Postgres queries; column names map to the
 * snake_case schema in supabase/migrations/20260727000000_social_core.sql. */

import { createClient, type SupabaseClient } from "@supabase/supabase-js";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { OAUTH_PORT, SUPABASE_ANON_KEY, SUPABASE_URL, isSupabaseConfigured } from "./config";
import type {
  Friend,
  FriendRequests,
  Profile,
  ProfilePatch,
  PublicProfile,
  SocialClient,
} from "./types";

const supabase: SupabaseClient = isSupabaseConfigured
  ? createClient(SUPABASE_URL!, SUPABASE_ANON_KEY!, {
      auth: {
        flowType: "pkce",
        detectSessionInUrl: false, // we complete the code exchange by hand
        persistSession: true,
        autoRefreshToken: true,
      },
    })
  : (undefined as unknown as SupabaseClient);

// --- row shapes + mappers (snake_case DB <-> camelCase domain) --------------

interface ProfileRow {
  id: string;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  presence_visibility: "friends" | "nobody";
  library_visibility: "friends" | "nobody";
  show_game_titles: boolean;
  appear_offline: boolean;
}

interface PublicRow {
  id: string;
  handle: string;
  display_name: string;
  avatar_url: string | null;
}

interface FriendshipRow {
  user_a: string;
  user_b: string;
  requested_by: string;
  status: "pending" | "accepted";
  created_at: string;
  responded_at: string | null;
}

function toProfile(r: ProfileRow): Profile {
  return {
    id: r.id,
    handle: r.handle,
    displayName: r.display_name,
    avatarUrl: r.avatar_url ?? undefined,
    presenceVisibility: r.presence_visibility,
    libraryVisibility: r.library_visibility,
    showGameTitles: r.show_game_titles,
    appearOffline: r.appear_offline,
  };
}

function toPublic(r: PublicRow): PublicProfile {
  return { id: r.id, handle: r.handle, displayName: r.display_name, avatarUrl: r.avatar_url ?? undefined };
}

function patchToRow(p: ProfilePatch): Record<string, unknown> {
  const row: Record<string, unknown> = {};
  if (p.handle !== undefined) row.handle = p.handle;
  if (p.displayName !== undefined) row.display_name = p.displayName;
  if (p.avatarUrl !== undefined) row.avatar_url = p.avatarUrl;
  if (p.presenceVisibility !== undefined) row.presence_visibility = p.presenceVisibility;
  if (p.libraryVisibility !== undefined) row.library_visibility = p.libraryVisibility;
  if (p.showGameTitles !== undefined) row.show_game_titles = p.showGameTitles;
  if (p.appearOffline !== undefined) row.appear_offline = p.appearOffline;
  return row;
}

/** Canonical pair ordering, matching the SQL `user_a < user_b` check. UUID text
 *  sorts the same as Postgres's uuid comparison, so a plain string compare is safe. */
function pair(a: string, b: string): [string, string] {
  return a < b ? [a, b] : [b, a];
}

function dbError(error: { message: string; code?: string }): Error {
  if (error.code === "23505" || /duplicate|unique/i.test(error.message)) {
    return new Error("That handle is taken.");
  }
  return new Error(error.message);
}

async function myId(): Promise<string> {
  const { data } = await supabase.auth.getUser();
  if (!data.user) throw new Error("Not signed in.");
  return data.user.id;
}

async function profilesByIds(ids: string[]): Promise<Map<string, PublicRow>> {
  if (ids.length === 0) return new Map();
  const { data, error } = await supabase
    .from("profiles")
    .select("id,handle,display_name,avatar_url")
    .in("id", ids);
  if (error) throw error;
  return new Map((data as PublicRow[]).map((p) => [p.id, p]));
}

const unknownRow = (id: string): PublicRow => ({
  id,
  handle: "unknown",
  display_name: "Unknown",
  avatar_url: null,
});

// ---------------------------------------------------------------------------

export const supabaseSocialClient: SocialClient = {
  kind: "supabase",

  async getSession() {
    const { data } = await supabase.auth.getSession();
    return data.session ? { userId: data.session.user.id } : null;
  },

  async signInWithDiscord() {
    // Arm the loopback listener before opening the browser so the redirect
    // can't arrive before we're listening.
    const codePromise = invoke<string>("oauth_capture", { port: OAUTH_PORT });
    const { data, error } = await supabase.auth.signInWithOAuth({
      provider: "discord",
      options: { redirectTo: `http://localhost:${OAUTH_PORT}/callback`, skipBrowserRedirect: true },
    });
    if (error) throw error;
    if (!data?.url) throw new Error("Could not start Discord sign-in.");
    await openUrl(data.url);
    const code = await codePromise;
    const { error: exchangeError } = await supabase.auth.exchangeCodeForSession(code);
    if (exchangeError) throw exchangeError;
  },

  async signOut() {
    const { error } = await supabase.auth.signOut();
    if (error) throw error;
  },

  onSessionChange(cb) {
    const { data } = supabase.auth.onAuthStateChange((_event, session) => {
      cb(session ? { userId: session.user.id } : null);
    });
    return () => data.subscription.unsubscribe();
  },

  async getMyProfile() {
    const { data: userData } = await supabase.auth.getUser();
    if (!userData.user) return null;
    const { data, error } = await supabase
      .from("profiles")
      .select("*")
      .eq("id", userData.user.id)
      .maybeSingle();
    if (error) throw error;
    return data ? toProfile(data as ProfileRow) : null;
  },

  async updateMyProfile(patch) {
    const id = await myId();
    const { data, error } = await supabase
      .from("profiles")
      .update(patchToRow(patch))
      .eq("id", id)
      .select()
      .single();
    if (error) throw dbError(error);
    return toProfile(data as ProfileRow);
  },

  async isHandleAvailable(handle) {
    const { data, error } = await supabase.rpc("find_profile_by_handle", { lookup: handle });
    if (error) throw error;
    const rows = (data ?? []) as PublicRow[];
    if (rows.length === 0) return true;
    try {
      return rows[0].id === (await myId()); // your own handle counts as available
    } catch {
      return false;
    }
  },

  async findByHandle(handle) {
    const clean = handle.replace(/^@/, "");
    const { data, error } = await supabase.rpc("find_profile_by_handle", { lookup: clean });
    if (error) throw error;
    const rows = (data ?? []) as PublicRow[];
    return rows.length ? toPublic(rows[0]) : null;
  },

  async listFriends() {
    const id = await myId();
    const { data, error } = await supabase.from("friendships").select("*").eq("status", "accepted");
    if (error) throw error;
    const rows = (data ?? []) as FriendshipRow[];
    const others = rows.map((r) => (r.user_a === id ? r.user_b : r.user_a));
    const byId = await profilesByIds(others);
    return rows.map<Friend>((r) => {
      const oid = r.user_a === id ? r.user_b : r.user_a;
      return { ...toPublic(byId.get(oid) ?? unknownRow(oid)), since: r.responded_at ?? r.created_at };
    });
  },

  async listRequests(): Promise<FriendRequests> {
    const id = await myId();
    const { data, error } = await supabase.from("friendships").select("*").eq("status", "pending");
    if (error) throw error;
    const rows = (data ?? []) as FriendshipRow[];
    const byId = await profilesByIds(rows.map((r) => (r.user_a === id ? r.user_b : r.user_a)));
    const incoming: FriendRequests["incoming"] = [];
    const outgoing: FriendRequests["outgoing"] = [];
    for (const r of rows) {
      const oid = r.user_a === id ? r.user_b : r.user_a;
      const entry = { profile: toPublic(byId.get(oid) ?? unknownRow(oid)), at: r.created_at };
      if (r.requested_by === id) outgoing.push(entry);
      else incoming.push(entry);
    }
    return { incoming, outgoing };
  },

  async sendRequest(handle) {
    const target = await this.findByHandle(handle);
    if (!target) throw new Error(`No one with the handle @${handle.replace(/^@/, "")}`);
    const id = await myId();
    if (target.id === id) throw new Error("That's you.");
    const [a, b] = pair(id, target.id);

    const { data: existing } = await supabase
      .from("friendships")
      .select("*")
      .eq("user_a", a)
      .eq("user_b", b)
      .maybeSingle();
    if (existing) {
      const e = existing as FriendshipRow;
      if (e.status === "accepted") throw new Error("Already friends.");
      if (e.requested_by === id) throw new Error("Request already sent.");
      return this.acceptRequest(target.id); // they asked first — accept
    }
    const { error } = await supabase
      .from("friendships")
      .insert({ user_a: a, user_b: b, requested_by: id, status: "pending" });
    if (error) throw dbError(error);
  },

  async acceptRequest(userId) {
    const id = await myId();
    const [a, b] = pair(id, userId);
    const { error } = await supabase
      .from("friendships")
      .update({ status: "accepted", responded_at: new Date().toISOString() })
      .eq("user_a", a)
      .eq("user_b", b);
    if (error) throw error;
  },

  async declineRequest(userId) {
    await this.removeFriend(userId); // decline/cancel/unfriend are all a row delete
  },

  async removeFriend(userId) {
    const id = await myId();
    const [a, b] = pair(id, userId);
    const { error } = await supabase.from("friendships").delete().eq("user_a", a).eq("user_b", b);
    if (error) throw error;
  },
};
