/* In-memory SocialClient. No backend: it seeds a signed-out state with a fake
 * friend graph so the Friends UI is fully clickable for review. Swapping this
 * for the Supabase-backed client (same interface) is a one-line change in
 * SocialProvider once the project URL + anon key exist. */

import type {
  Friend,
  FriendRequests,
  Profile,
  ProfilePatch,
  PublicProfile,
  Session,
  SocialClient,
} from "./types";
import { HANDLE_RE } from "./types";

const ME_ID = "me-0001";
const delay = (ms = 140) => new Promise((r) => setTimeout(r, ms));

function directoryEntry(id: string, handle: string, displayName: string): PublicProfile {
  return { id, handle, displayName };
}

// People you could look up by handle (mock "everyone else").
const DIRECTORY: PublicProfile[] = [
  directoryEntry("u-finn", "finn", "Finn the Human"),
  directoryEntry("u-jake", "jake", "Jake the Dog"),
  directoryEntry("u-marcy", "marceline", "Marceline"),
  directoryEntry("u-bmo", "bmo", "BMO"),
  directoryEntry("u-pb", "bubblegum", "Princess Bubblegum"),
];

let session: Session | null = null;
const listeners = new Set<(s: Session | null) => void>();

let me: Profile = {
  id: ME_ID,
  handle: "noob",
  displayName: "Noob",
  presenceVisibility: "friends",
  libraryVisibility: "friends",
  showGameTitles: true,
  appearOffline: false,
};

let friends: Friend[] = [
  { ...DIRECTORY[0], since: "2026-07-20T18:00:00Z" }, // finn
  { ...DIRECTORY[1], since: "2026-07-22T12:30:00Z" }, // jake
];
let incoming = [{ profile: DIRECTORY[2], at: "2026-07-26T21:10:00Z" }]; // marceline
let outgoing = [{ profile: DIRECTORY[3], at: "2026-07-27T02:05:00Z" }]; // bmo

function notify() {
  for (const cb of listeners) cb(session);
}

function handleTaken(handle: string): boolean {
  const h = handle.toLowerCase();
  if (me.handle.toLowerCase() === h) return true;
  return DIRECTORY.some((d) => d.handle.toLowerCase() === h);
}

export const mockClient: SocialClient = {
  kind: "mock",

  async getSession() {
    await delay(60);
    return session;
  },

  async signInWithDiscord() {
    await delay(250);
    session = { userId: ME_ID };
    notify();
  },

  async signOut() {
    await delay(80);
    session = null;
    notify();
  },

  onSessionChange(cb) {
    listeners.add(cb);
    return () => listeners.delete(cb);
  },

  async getMyProfile() {
    await delay();
    return session ? { ...me } : null;
  },

  async updateMyProfile(patch: ProfilePatch) {
    await delay();
    if (patch.handle !== undefined) {
      if (!HANDLE_RE.test(patch.handle)) throw new Error("Handle must be 3–20 letters, digits or _");
      if (patch.handle.toLowerCase() !== me.handle.toLowerCase() && handleTaken(patch.handle))
        throw new Error(`@${patch.handle} is taken`);
    }
    me = { ...me, ...patch };
    return { ...me };
  },

  async isHandleAvailable(handle: string) {
    await delay(90);
    if (!HANDLE_RE.test(handle)) return false;
    if (handle.toLowerCase() === me.handle.toLowerCase()) return true; // your own
    return !handleTaken(handle);
  },

  async findByHandle(handle: string) {
    await delay();
    const h = handle.toLowerCase().replace(/^@/, "");
    return DIRECTORY.find((d) => d.handle.toLowerCase() === h) ?? null;
  },

  async listFriends() {
    await delay();
    return [...friends];
  },

  async listRequests(): Promise<FriendRequests> {
    await delay();
    return { incoming: [...incoming], outgoing: [...outgoing] };
  },

  async sendRequest(handle: string) {
    await delay(180);
    const target = await this.findByHandle(handle);
    if (!target) throw new Error(`No one with the handle @${handle.replace(/^@/, "")}`);
    if (target.id === me.id) throw new Error("That's you.");
    if (friends.some((f) => f.id === target.id)) throw new Error("Already friends.");
    if (outgoing.some((r) => r.profile.id === target.id)) throw new Error("Request already sent.");
    if (incoming.some((r) => r.profile.id === target.id)) {
      // They already asked you — accept instead of sending a second request.
      return this.acceptRequest(target.id);
    }
    outgoing = [...outgoing, { profile: target, at: new Date().toISOString() }];
  },

  async acceptRequest(userId: string) {
    await delay(150);
    const req = incoming.find((r) => r.profile.id === userId);
    if (!req) throw new Error("No such request.");
    incoming = incoming.filter((r) => r.profile.id !== userId);
    friends = [...friends, { ...req.profile, since: new Date().toISOString() }];
  },

  async declineRequest(userId: string) {
    await delay(120);
    incoming = incoming.filter((r) => r.profile.id !== userId);
    outgoing = outgoing.filter((r) => r.profile.id !== userId);
  },

  async removeFriend(userId: string) {
    await delay(120);
    friends = friends.filter((f) => f.id !== userId);
  },
};
