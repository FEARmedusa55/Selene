/* Social domain types + the client contract.
 *
 * The UI talks only to `SocialClient`. Today that's `mockClient` (in-memory, no
 * backend); later a Supabase-backed implementation of the same interface drops
 * in with a one-line provider swap. Keeping the contract here means the UI never
 * learns which one it's using. Field names mirror the SQL in
 * supabase/migrations/20260727000000_social_core.sql. */

export type Visibility = "friends" | "nobody";

/** A signed-in session. The token itself is held by the auth SDK, not here. */
export interface Session {
  userId: string;
}

/** Your own full profile, including privacy settings. */
export interface Profile {
  id: string;
  handle: string;
  displayName: string;
  avatarUrl?: string;
  presenceVisibility: Visibility;
  libraryVisibility: Visibility;
  showGameTitles: boolean;
  appearOffline: boolean;
}

/** The minimal, public-facing view of someone — all a non-friend lookup returns. */
export interface PublicProfile {
  id: string;
  handle: string;
  displayName: string;
  avatarUrl?: string;
}

/** An accepted friend. Live presence is fetched/subscribed separately. */
export interface Friend extends PublicProfile {
  since: string;
}

export type PresenceStatus = "offline" | "online" | "in_game";

/** A user's live presence. */
export interface Presence {
  userId: string;
  status: PresenceStatus;
  /** The game being played, when in_game and titles aren't hidden. */
  gameTitle?: string;
  /** ISO timestamp; doubles as the heartbeat for staleness. */
  updatedAt: string;
}

/** What the client publishes about itself. */
export interface PresenceUpdate {
  status: PresenceStatus;
  gameTitle?: string | null;
}

/** A pending request, in either direction. */
export interface FriendRequest {
  profile: PublicProfile;
  /** ISO timestamp the request was created. */
  at: string;
}

export interface FriendRequests {
  incoming: FriendRequest[];
  outgoing: FriendRequest[];
}

/** The fields a user may edit on their own profile. All optional = patch. */
export interface ProfilePatch {
  handle?: string;
  displayName?: string;
  avatarUrl?: string;
  presenceVisibility?: Visibility;
  libraryVisibility?: Visibility;
  showGameTitles?: boolean;
  appearOffline?: boolean;
}

/** Everything the Friends UI needs. Implemented by mock now, Supabase later. */
export interface SocialClient {
  /** Human label for the active backend, e.g. "mock" or "supabase". */
  readonly kind: string;

  // --- auth ---
  getSession(): Promise<Session | null>;
  /** Real: Discord OAuth. Mock: instantly signs in a seeded user. */
  signInWithDiscord(): Promise<void>;
  signOut(): Promise<void>;
  /** Subscribe to session changes; returns an unsubscribe fn. */
  onSessionChange(cb: (s: Session | null) => void): () => void;

  // --- profile ---
  getMyProfile(): Promise<Profile | null>;
  updateMyProfile(patch: ProfilePatch): Promise<Profile>;
  /** Is a handle free to claim? (case-insensitive) */
  isHandleAvailable(handle: string): Promise<boolean>;

  // --- friends ---
  /** Exact-handle lookup for adding someone. `null` if no such handle. */
  findByHandle(handle: string): Promise<PublicProfile | null>;
  listFriends(): Promise<Friend[]>;
  listRequests(): Promise<FriendRequests>;
  sendRequest(handle: string): Promise<void>;
  acceptRequest(userId: string): Promise<void>;
  declineRequest(userId: string): Promise<void>;
  removeFriend(userId: string): Promise<void>;

  // --- presence ---
  /** Publish your own live status. */
  publishPresence(update: PresenceUpdate): Promise<void>;
  /** Current presence for a set of users (your friends). */
  listPresence(userIds: string[]): Promise<Presence[]>;
  /** Subscribe to live presence changes for your friends; returns unsubscribe. */
  subscribePresence(onChange: (presence: Presence) => void): () => void;
}

/** An 'online'/'in_game' row older than this is treated as offline (missed the
 *  clean shutdown). Must exceed the publisher's heartbeat interval. */
export const PRESENCE_STALE_MS = 150_000;

/** A valid handle: 3–20 chars, letters/digits/underscore. Mirrors the SQL check. */
export const HANDLE_RE = /^[A-Za-z0-9_]{3,20}$/;
