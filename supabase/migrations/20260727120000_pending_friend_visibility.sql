-- Fix: let you see the profile of anyone you have a friendship row with — not
-- just ACCEPTED friends, but PENDING ones too. Without this, an incoming or
-- outgoing friend request renders as "Unknown", because the other person's
-- profile isn't readable until you're already friends (chicken-and-egg).
--
-- Safe: you only gain visibility of people you already have a relationship row
-- with (you looked them up by exact handle to send the request, or they sent
-- one to you). Deleting the row (decline/cancel/unfriend) revokes it again.
--
-- Run this in the Supabase SQL editor after the initial migration.

create or replace function public.has_friendship(a uuid, b uuid)
returns boolean
language sql
stable
security definer
set search_path = public
as $$
  select exists (
    select 1 from public.friendships f 
    where f.user_a = least(a, b)
      and f.user_b = greatest(a, b)
  );
$$;

grant execute on function public.has_friendship(uuid, uuid) to authenticated;

-- Repoint the profiles read policy at "any friendship" instead of "accepted".
drop policy if exists profiles_select on public.profiles;
create policy profiles_select on public.profiles
  for select using (id = auth.uid() or public.has_friendship(auth.uid(), id));
