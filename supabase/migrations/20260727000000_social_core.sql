-- Selene online — Phase A: accounts, profiles, and the friend graph.
--
-- Design decisions encoded here (from the product spec):
--   * Add friends by a unique @handle (not a code / not Discord's graph).
--   * Open sign-in, PRIVATE social: anyone may sign in, but there is no public
--     discovery. You can only read a profile if it's your own or a friend's;
--     adding someone works only by their EXACT handle, via a definer function
--     so the table can't be enumerated.
--   * Default visibility: friends can see your presence AND library. The columns
--     exist so this can be tightened per-user later; defaults are the open ones.
--   * Profiles are seeded from Discord (name + avatar) on first sign-in, editable.
--
-- Apply with the Supabase CLI (`supabase db push`) or paste into the SQL editor.
-- Presence and activity are later migrations (Phase B/C); this is the core graph.

create extension if not exists citext;

-- ---------------------------------------------------------------------------
-- profiles: one row per auth user, seeded from Discord, edited by the user.
-- ---------------------------------------------------------------------------
create table if not exists public.profiles (
  id                  uuid primary key references auth.users (id) on delete cascade,
  -- Unique, case-insensitive handle. citext makes "Finn" and "finn" collide,
  -- while preserving the casing the user typed for display.
  handle              citext not null unique
                        check (handle ~ '^[A-Za-z0-9_]{3,20}$'),
  display_name        text not null default 'Player',
  avatar_url          text,
  -- Privacy. 'friends' = visible to accepted friends; 'nobody' = hidden.
  presence_visibility text not null default 'friends'
                        check (presence_visibility in ('friends','nobody')),
  library_visibility  text not null default 'friends'
                        check (library_visibility in ('friends','nobody')),
  -- Show the specific game title in presence vs a generic "In a game".
  show_game_titles    boolean not null default true,
  -- Appear offline to everyone without changing the settings above.
  appear_offline      boolean not null default false,
  created_at          timestamptz not null default now(),
  updated_at          timestamptz not null default now()
);

-- ---------------------------------------------------------------------------
-- friendships: one row per relationship, canonical (user_a < user_b) so a pair
-- can never have two reciprocal rows. `requested_by` records direction.
-- ---------------------------------------------------------------------------
create table if not exists public.friendships (
  user_a       uuid not null references public.profiles (id) on delete cascade,
  user_b       uuid not null references public.profiles (id) on delete cascade,
  requested_by uuid not null references public.profiles (id) on delete cascade,
  status       text not null default 'pending' check (status in ('pending','accepted')),
  created_at   timestamptz not null default now(),
  responded_at timestamptz,
  primary key (user_a, user_b),
  check (user_a < user_b)
);

create index if not exists friendships_user_b_idx on public.friendships (user_b);

-- ---------------------------------------------------------------------------
-- Helpers (SECURITY DEFINER so they can see across RLS in a controlled way).
-- ---------------------------------------------------------------------------

-- Are two users accepted friends? Used by the profiles read policy.
create or replace function public.are_friends(a uuid, b uuid)
returns boolean
language sql
stable
security definer
set search_path = public
as $$
  select exists (
    select 1 from public.friendships f
    where f.status = 'accepted'
      and f.user_a = least(a, b)
      and f.user_b = greatest(a, b)
  );
$$;

-- Resolve a profile by EXACT handle, returning only public fields. This is the
-- one hole in "private social": you can look someone up to add them, but only
-- if you know their exact handle — the table itself cannot be listed.
create or replace function public.find_profile_by_handle(lookup citext)
returns table (id uuid, handle citext, display_name text, avatar_url text)
language sql
stable
security definer
set search_path = public
as $$
  select p.id, p.handle, p.display_name, p.avatar_url
  from public.profiles p
  where p.handle = lookup
  limit 1;
$$;

-- Seed a profile from Discord metadata on sign-up. Handle must be unique, so we
-- derive a slug and add a short suffix on collision; the user renames it later.
create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
set search_path = public
as $$
declare
  meta        jsonb := coalesce(new.raw_user_meta_data, '{}'::jsonb);
  base        text;
  candidate   text;
begin
  base := lower(regexp_replace(
    coalesce(meta->>'preferred_username', meta->>'name', meta->>'full_name', 'player'),
    '[^A-Za-z0-9_]', '', 'g'));
  if length(base) < 3 then base := 'player'; end if;
  base := substr(base, 1, 14);

  candidate := base;
  while exists (select 1 from public.profiles where handle = candidate) loop
    candidate := base || '_' || substr(md5(random()::text), 1, 4);
  end loop;

  insert into public.profiles (id, handle, display_name, avatar_url)
  values (
    new.id,
    candidate,
    coalesce(meta->>'full_name', meta->>'name', meta->>'preferred_username', 'Player'),
    coalesce(meta->>'avatar_url', meta->>'picture')
  )
  on conflict (id) do nothing;
  return new;
end;
$$;

drop trigger if exists on_auth_user_created on auth.users;
create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function public.handle_new_user();

-- Keep updated_at fresh on profile edits.
create or replace function public.touch_updated_at()
returns trigger language plpgsql as $$
begin new.updated_at := now(); return new; end;
$$;

drop trigger if exists profiles_touch_updated on public.profiles;
create trigger profiles_touch_updated
  before update on public.profiles
  for each row execute function public.touch_updated_at();

-- ---------------------------------------------------------------------------
-- Row Level Security
-- ---------------------------------------------------------------------------
alter table public.profiles    enable row level security;
alter table public.friendships enable row level security;

-- profiles: read your own or a friend's; write only your own.
drop policy if exists profiles_select on public.profiles;
create policy profiles_select on public.profiles
  for select using (id = auth.uid() or public.are_friends(auth.uid(), id));

drop policy if exists profiles_insert_self on public.profiles;
create policy profiles_insert_self on public.profiles
  for insert with check (id = auth.uid());

drop policy if exists profiles_update_self on public.profiles;
create policy profiles_update_self on public.profiles
  for update using (id = auth.uid()) with check (id = auth.uid());

-- friendships: only the two people in a relationship can see or touch it.
drop policy if exists friendships_select on public.friendships;
create policy friendships_select on public.friendships
  for select using (auth.uid() = user_a or auth.uid() = user_b);

-- You may create a pending request that includes you, marked as sent by you.
drop policy if exists friendships_insert on public.friendships;
create policy friendships_insert on public.friendships
  for insert with check (
    requested_by = auth.uid()
    and (auth.uid() = user_a or auth.uid() = user_b)
    and status = 'pending'
  );

-- Only the RECIPIENT (not the requester) can accept a pending request.
drop policy if exists friendships_accept on public.friendships;
create policy friendships_accept on public.friendships
  for update using (
    (auth.uid() = user_a or auth.uid() = user_b) and auth.uid() <> requested_by
  ) with check (status = 'accepted');

-- Either party can decline / cancel / unfriend by deleting the row.
drop policy if exists friendships_delete on public.friendships;
create policy friendships_delete on public.friendships
  for delete using (auth.uid() = user_a or auth.uid() = user_b);

-- Supabase: grant table DML to signed-in users; RLS above does the real gating.
grant select, insert, update, delete on public.profiles    to authenticated;
grant select, insert, update, delete on public.friendships to authenticated;
grant execute on function public.find_profile_by_handle(citext) to authenticated;
grant execute on function public.are_friends(uuid, uuid)        to authenticated;
