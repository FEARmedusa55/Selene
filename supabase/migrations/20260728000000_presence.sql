-- Selene online — Phase B: presence.
--
-- One row per user holding their live status. Friends read it (RLS), and
-- Supabase Realtime pushes changes to them. Privacy is enforced on the WRITE
-- side, not here: the client publishes 'offline' when you've set appear_offline
-- or presence_visibility='nobody', and leaves game_title null when
-- show_game_titles is off — so the row itself never carries what you've hidden,
-- and the read policy can stay simple.
--
-- `updated_at` doubles as a heartbeat: a reader treats an 'online'/'in_game' row
-- that hasn't ticked in a couple of minutes as offline, covering crashes that
-- never got to write 'offline'.

create table if not exists public.presence (
  user_id    uuid primary key references public.profiles (id) on delete cascade,
  status     text not null default 'offline' check (status in ('offline', 'online', 'in_game')),
  game_title text,
  updated_at timestamptz not null default now()
);

alter table public.presence enable row level security;

-- Read your own, or a friend's (accepted or pending — same helper as profiles).
drop policy if exists presence_select on public.presence;
create policy presence_select on public.presence
  for select using (user_id = auth.uid() or public.has_friendship(auth.uid(), user_id));

-- Publish only your own presence.
drop policy if exists presence_insert on public.presence;
create policy presence_insert on public.presence
  for insert with check (user_id = auth.uid());

drop policy if exists presence_update on public.presence;
create policy presence_update on public.presence
  for update using (user_id = auth.uid()) with check (user_id = auth.uid());

grant select, insert, update on public.presence to authenticated;

-- Stream row changes to subscribed clients (RLS still applies per-subscriber).
alter publication supabase_realtime add table public.presence;
