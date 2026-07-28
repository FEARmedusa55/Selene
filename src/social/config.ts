/* Supabase connection, read from Vite env (.env, gitignored). Both values are
 * safe to ship in a client — RLS in the database is the real access control. */

export const SUPABASE_URL = (import.meta.env.VITE_SUPABASE_URL as string | undefined)?.replace(
  /\/+$/,
  "",
);
export const SUPABASE_ANON_KEY = import.meta.env.VITE_SUPABASE_ANON_KEY as string | undefined;

export const isSupabaseConfigured = Boolean(SUPABASE_URL && SUPABASE_ANON_KEY);

/** Fixed loopback port the desktop OAuth flow listens on. Must match the
 *  redirect URL allow-listed in Supabase (http://localhost:54321/callback). */
export const OAUTH_PORT = 54321;
