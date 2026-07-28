# Selene online — provisioning (Supabase + Discord)

You do these two once, while signed in. They give me the URL + keys I wire into
the app. Nothing here is destructive, and none of it touches the local-first app.

## 1. Supabase project (~5 min)

1. Go to <https://supabase.com> → sign in → **New project**.
2. Name it `selene`, pick a strong DB password (save it), choose the region
   closest to you. Free tier is fine.
3. Wait for it to finish provisioning.
4. **Project Settings → API** — copy these two (they're safe in a desktop app):
   - **Project URL** — `https://xxxx.supabase.co`
   - **anon public** key — a long JWT.
   (Do **not** copy the `service_role` key — that one is a secret and never ships
   in the client.)
5. Apply the schema: **SQL Editor → New query**, paste the contents of
   [`supabase/migrations/20260727000000_social_core.sql`](../../supabase/migrations/20260727000000_social_core.sql),
   and **Run**. It creates `profiles` + `friendships`, the security rules, and the
   Discord-profile trigger.

## 2. Discord OAuth app (~5 min)

1. Go to <https://discord.com/developers/applications> → **New Application** →
   name it `Selene`.
2. **OAuth2** tab → copy the **Client ID** and **Client Secret**.
3. Under **OAuth2 → Redirects**, add Supabase's callback URL (from Supabase
   **Authentication → Providers → Discord**, shown there) — it looks like:
   `https://xxxx.supabase.co/auth/v1/callback`
4. Save.

## 3. Connect Discord to Supabase

1. Supabase → **Authentication → Providers → Discord** → enable it.
2. Paste the Discord **Client ID** and **Client Secret** from step 2. Save.
3. Supabase → **Authentication → URL Configuration → Redirect URLs** → add:
   ```
   http://localhost:54321/callback
   ```
   The desktop app opens Discord in your browser and catches the sign-in on this
   loopback address (see `oauth_capture` in `src-tauri/src/lib.rs`).

## What to hand me

- Supabase **Project URL**
- Supabase **anon public** key
- Confirmation the SQL ran without errors

That's it — I plug those into the client config and the mock UI becomes the real
thing. The `service_role` key and the DB password stay with you.
