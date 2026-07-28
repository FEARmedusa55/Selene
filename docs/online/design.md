# Selene online — architecture & Phase A design

## Principles

- **Opt-in overlay.** Signed out, Selene is the same 100% local-first app it has
  always been — no network, no account. The social layer only activates when you
  sign in. Nothing in the local library/launch/config paths depends on it.
- **BaaS, not a server we babysit.** Supabase (Postgres + Auth + Realtime + Row
  Level Security) so there's no server to run, patch, or secure by hand. It's
  open-source and self-hostable later if we ever outgrow the free tier.
- **Security lives in the database.** RLS policies (see the migration) are the
  real access control, not the client. The desktop app holds only the anon key,
  which can do nothing a policy doesn't allow.

## Data model (Phase A)

- `profiles` — 1:1 with `auth.users`. Unique `@handle`, `display_name`,
  `avatar_url` (seeded from Discord), plus privacy columns.
- `friendships` — one row per pair, canonical `user_a < user_b`, `requested_by`
  for direction, `status` in `pending | accepted`.

**Private social:** the `profiles` read policy only exposes *you* and your
*accepted friends*. Adding someone works solely through
`find_profile_by_handle()` — an exact-match, definer function — so the table
can't be enumerated and there's no public directory.

## Client architecture

```
React UI ──> SocialClient (interface) ──> { MockClient (now) | SupabaseClient (later) }
                                             │
                                             └─ Supabase JS: auth session, RLS'd
                                                queries, realtime (Phase B presence)
```

- `src/social/types.ts` — domain types + the `SocialClient` contract.
- `src/social/mockClient.ts` — in-memory implementation with seed data, so the
  whole Friends UI is clickable with **no backend**. This is what's wired today.
- `SupabaseClient` (later) — same interface, backed by the Supabase JS SDK. Auth
  is Discord OAuth; the session token is kept by the SDK. Swapping mock → real is
  a one-line provider change once the URL/anon key exist.

## UI

A **Friends** tab (opt-in). Signed out → "Sign in with Discord". Signed in →
profile chip (avatar, name, editable `@handle`), **Add friend** by handle
(with availability/lookup), incoming/outgoing requests, friends list, and the
privacy toggles. Built from the existing `panel` / `notice` / `field` / `btn`
design-token classes so it themes with everything else.

## Phasing

- **A (this scaffold): accounts + profile + friends.** ← mock UI is done; wire
  Supabase when the project exists.
- **B: presence.** Selene already watches the launched game process, so
  online / "playing X" is pushed to friends over Supabase Realtime. Honors
  `appear_offline` / `show_game_titles`.
- **C: activity.** Recent sessions, friend playtime compare.
- **D: later.** Invites that hook into the Goldberg "get friends on" LAN flow,
  chat, shared collections.

## Chosen defaults (from the spec Q&A)

| Area | Choice |
| --- | --- |
| Add friends | unique `@handle` |
| Accounts | open sign-in, private social (no discovery) |
| Default visibility | friends see titles **and** library |
| Profile source | Discord (name + avatar), editable |
