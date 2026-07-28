import { useCallback, useEffect, useState } from "react";
import { useSocial } from "../social/SocialProvider";
import type {
  Friend,
  FriendRequests,
  Presence,
  PresenceStatus,
  Profile,
  Visibility,
} from "../social/types";
import { HANDLE_RE, PRESENCE_STALE_MS } from "../social/types";

/** A stale online/in-game row (missed its clean shutdown) counts as offline. */
function effectiveStatus(p: Presence | undefined): PresenceStatus {
  if (!p) return "offline";
  if (p.status !== "offline" && Date.now() - new Date(p.updatedAt).getTime() > PRESENCE_STALE_MS) {
    return "offline";
  }
  return p.status;
}

function presenceText(p: Presence | undefined): string {
  switch (effectiveStatus(p)) {
    case "in_game":
      return p?.gameTitle ? `Playing ${p.gameTitle}` : "In a game";
    case "online":
      return "Online";
    default:
      return "Offline";
  }
}

/* The Friends tab — the opt-in social layer.
 *
 * Signed out, it's a single "sign in with Discord" card. Signed in, it shows
 * your profile (editable), add-by-@handle, incoming/outgoing requests, your
 * friends, and the privacy toggles. Today it's backed by the in-memory mock
 * client, so everything works with no backend — hence the preview banner. */
export function FriendsTab() {
  const { client, session, me, loading, refreshMe } = useSocial();

  if (loading) {
    return (
      <div className="page">
        <h1 className="page__title">Friends</h1>
        <div className="panel">Loading…</div>
      </div>
    );
  }

  return (
    <div className="page">
      <h1 className="page__title">Friends</h1>

      {client.kind === "mock" && (
        <div className="notice notice--warn">
          <strong>Preview.</strong> Not connected to a backend yet — this is the
          mock friend graph so the flow is clickable. Provision Supabase + Discord
          (see <code>docs/online/setup.md</code>) and it becomes real.
        </div>
      )}

      {!session || !me ? <SignedOut /> : <SignedIn me={me} refreshMe={refreshMe} />}
    </div>
  );
}

function SignedOut() {
  const { client } = useSocial();
  const [busy, setBusy] = useState(false);
  return (
    <section className="panel">
      <h2 className="panel__title">
        Sign in
        <span className="panel__hint">Selene stays fully local until you do</span>
      </h2>
      <div className="notice notice--info">
        Connect with friends: see who&rsquo;s online, what they&rsquo;re playing,
        and add each other by handle. Signed out, nothing leaves your PC.
      </div>
      <div className="row">
        <button
          className="btn btn--play"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await client.signInWithDiscord();
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "● Signing in…" : "Sign in with Discord"}
        </button>
      </div>
    </section>
  );
}

function Avatar({ url, name, size = 40 }: { url?: string; name: string; size?: number }) {
  const initials = name
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("");
  return url ? (
    <img className="avatar" src={url} alt="" style={{ width: size, height: size }} />
  ) : (
    <span className="avatar avatar--fallback" style={{ width: size, height: size }}>
      {initials || "?"}
    </span>
  );
}

function SignedIn({ me, refreshMe }: { me: Profile; refreshMe: () => Promise<void> }) {
  const { client } = useSocial();
  const [friends, setFriends] = useState<Friend[]>([]);
  const [requests, setRequests] = useState<FriendRequests>({ incoming: [], outgoing: [] });
  const [presence, setPresence] = useState<Map<string, Presence>>(new Map());
  const [note, setNote] = useState<string | null>(null);

  const flash = (m: string) => {
    setNote(m);
    setTimeout(() => setNote(null), 4000);
  };

  const reload = useCallback(async () => {
    const [f, r] = await Promise.all([client.listFriends(), client.listRequests()]);
    setFriends(f);
    setRequests(r);
  }, [client]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Fetch current presence whenever the friend set changes.
  useEffect(() => {
    const ids = friends.map((f) => f.id);
    if (ids.length === 0) {
      setPresence(new Map());
      return;
    }
    let active = true;
    client
      .listPresence(ids)
      .then((list) => {
        if (active) setPresence(new Map(list.map((p) => [p.userId, p])));
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [client, friends]);

  // Live updates: RLS scopes the stream to friends' rows.
  useEffect(() => {
    return client.subscribePresence((p) => {
      setPresence((prev) => new Map(prev).set(p.userId, p));
    });
  }, [client]);

  // Online / in-game friends float to the top, then alphabetical.
  const orderedFriends = [...friends].sort((a, b) => {
    const rank = (id: string) => {
      const s = effectiveStatus(presence.get(id));
      return s === "in_game" ? 0 : s === "online" ? 1 : 2;
    };
    return rank(a.id) - rank(b.id) || a.displayName.localeCompare(b.displayName);
  });

  return (
    <>
      {note && <div className="notice notice--info">{note}</div>}

      <ProfileCard me={me} refreshMe={refreshMe} onFlash={flash} />

      <AddFriend onDone={reload} onFlash={flash} />

      {(requests.incoming.length > 0 || requests.outgoing.length > 0) && (
        <section className="panel">
          <h2 className="panel__title">Requests</h2>
          {requests.incoming.map((r) => (
            <div className="friendrow" key={r.profile.id}>
              <Avatar url={r.profile.avatarUrl} name={r.profile.displayName} />
              <div className="friendrow__id">
                <span className="friendrow__name">{r.profile.displayName}</span>
                <span className="friendrow__handle">@{r.profile.handle}</span>
              </div>
              <span className="badge badge--muted">wants to connect</span>
              <button
                className="btn btn--small"
                onClick={async () => {
                  await client.acceptRequest(r.profile.id);
                  await reload();
                  flash(`You and ${r.profile.displayName} are now friends`);
                }}
              >
                Accept
              </button>
              <button
                className="btn btn--small btn--ghost"
                onClick={async () => {
                  await client.declineRequest(r.profile.id);
                  await reload();
                }}
              >
                Decline
              </button>
            </div>
          ))}
          {requests.outgoing.map((r) => (
            <div className="friendrow" key={r.profile.id}>
              <Avatar url={r.profile.avatarUrl} name={r.profile.displayName} />
              <div className="friendrow__id">
                <span className="friendrow__name">{r.profile.displayName}</span>
                <span className="friendrow__handle">@{r.profile.handle}</span>
              </div>
              <span className="badge badge--muted">pending</span>
              <button
                className="btn btn--small btn--ghost"
                onClick={async () => {
                  await client.declineRequest(r.profile.id);
                  await reload();
                }}
              >
                Cancel
              </button>
            </div>
          ))}
        </section>
      )}

      <section className="panel">
        <h2 className="panel__title">
          Friends
          <span className="panel__hint">{friends.length} connected</span>
        </h2>
        {friends.length === 0 ? (
          <div className="notice notice--info">
            No friends yet. Add someone by their <strong>@handle</strong> above.
          </div>
        ) : (
          orderedFriends.map((f) => {
            const p = presence.get(f.id);
            const status = effectiveStatus(p);
            const label = presenceText(p);
            return (
              <div className="friendrow" key={f.id}>
                <span className="presence" data-status={status} title={label} />
                <Avatar url={f.avatarUrl} name={f.displayName} />
                <div className="friendrow__id">
                  <span className="friendrow__name">{f.displayName}</span>
                  <span className="friendrow__sub" data-status={status}>
                    {status === "offline" ? `@${f.handle}` : label}
                  </span>
                </div>
                <button
                  className="btn btn--small btn--ghost"
                  onClick={async () => {
                    await client.removeFriend(f.id);
                    await reload();
                  }}
                >
                  Remove
                </button>
              </div>
            );
          })
        )}
      </section>

      <PrivacyCard me={me} refreshMe={refreshMe} />

      <section className="panel">
        <div className="row">
          <button className="btn btn--ghost" onClick={() => void client.signOut()}>
            Sign out
          </button>
        </div>
      </section>
    </>
  );
}

function ProfileCard({
  me,
  refreshMe,
  onFlash,
}: {
  me: Profile;
  refreshMe: () => Promise<void>;
  onFlash: (m: string) => void;
}) {
  const { client } = useSocial();
  const [displayName, setDisplayName] = useState(me.displayName);
  const [handle, setHandle] = useState(me.handle);
  const [err, setErr] = useState<string | null>(null);
  const dirty = displayName !== me.displayName || handle !== me.handle;

  const save = async () => {
    setErr(null);
    if (!HANDLE_RE.test(handle)) {
      setErr("Handle must be 3–20 letters, digits or _");
      return;
    }
    try {
      await client.updateMyProfile({ displayName: displayName.trim() || me.displayName, handle });
      await refreshMe();
      onFlash("Profile saved");
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    }
  };

  return (
    <section className="panel">
      <h2 className="panel__title">Your profile</h2>
      <div className="profilehead">
        <Avatar url={me.avatarUrl} name={me.displayName} size={64} />
        <div className="profilehead__meta">
          <span className="profilehead__name">{me.displayName}</span>
          <span className="profilehead__handle">@{me.handle}</span>
        </div>
      </div>
      <div className="field">
        <label>Display name</label>
        <input
          className="input"
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          maxLength={32}
        />
      </div>
      <div className="field">
        <label>Handle</label>
        <div className="row">
          <span className="handleprefix">@</span>
          <input
            className="input"
            value={handle}
            onChange={(e) => setHandle(e.target.value.replace(/^@/, ""))}
            spellCheck={false}
            maxLength={20}
          />
        </div>
        <span className="field__hint">
          How friends add you. Unique, 3–20 letters/digits/underscore.
        </span>
      </div>
      {err && <div className="notice notice--warn">{err}</div>}
      <div className="row">
        <button className="btn btn--ghost" onClick={save} disabled={!dirty}>
          Save profile
        </button>
      </div>
    </section>
  );
}

function AddFriend({
  onDone,
  onFlash,
}: {
  onDone: () => Promise<void>;
  onFlash: (m: string) => void;
}) {
  const { client } = useSocial();
  const [handle, setHandle] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const send = async () => {
    const h = handle.trim().replace(/^@/, "");
    if (!h) return;
    setBusy(true);
    setErr(null);
    try {
      await client.sendRequest(h);
      setHandle("");
      await onDone();
      onFlash(`Request sent to @${h}`);
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel">
      <h2 className="panel__title">
        Add a friend
        <span className="panel__hint">By their @handle</span>
      </h2>
      <div className="field">
        <div className="row">
          <span className="handleprefix">@</span>
          <input
            className="input"
            value={handle}
            onChange={(e) => setHandle(e.target.value.replace(/^@/, ""))}
            onKeyDown={(e) => e.key === "Enter" && void send()}
            placeholder="handle"
            spellCheck={false}
          />
          <button className="btn" onClick={send} disabled={busy || !handle.trim()}>
            {busy ? "● Sending…" : "Send request"}
          </button>
        </div>
        <span className="field__hint">
          Try <code>finn</code>, <code>jake</code>, <code>marceline</code>,{" "}
          <code>bmo</code> or <code>bubblegum</code> in this preview.
        </span>
      </div>
      {err && <div className="notice notice--warn">{err}</div>}
    </section>
  );
}

function PrivacyCard({ me, refreshMe }: { me: Profile; refreshMe: () => Promise<void> }) {
  const { client } = useSocial();
  const set = async (patch: Partial<Profile>) => {
    await client.updateMyProfile(patch);
    await refreshMe();
  };

  const visRow = (
    label: string,
    hint: string,
    value: Visibility,
    onChange: (v: Visibility) => void,
  ) => (
    <div className="field">
      <label>{label}</label>
      <div className="row">
        <select className="select" value={value} onChange={(e) => onChange(e.target.value as Visibility)}>
          <option value="friends">Friends</option>
          <option value="nobody">Nobody</option>
        </select>
      </div>
      <span className="field__hint">{hint}</span>
    </div>
  );

  return (
    <section className="panel">
      <h2 className="panel__title">
        Privacy
        <span className="panel__hint">What friends can see</span>
      </h2>
      {visRow(
        "Presence",
        "Whether friends can see you're online and in a game.",
        me.presenceVisibility,
        (v) => void set({ presenceVisibility: v }),
      )}
      {visRow(
        "Library",
        "Whether friends can browse your game list. Your library is mostly emulated/cracked titles — set to Nobody to keep it private.",
        me.libraryVisibility,
        (v) => void set({ libraryVisibility: v }),
      )}
      <label className="toggle">
        <input
          type="checkbox"
          checked={me.showGameTitles}
          onChange={(e) => void set({ showGameTitles: e.target.checked })}
        />
        <span>Show the specific game in presence (off = a generic “In a game”)</span>
      </label>
      <label className="toggle">
        <input
          type="checkbox"
          checked={me.appearOffline}
          onChange={(e) => void set({ appearOffline: e.target.checked })}
        />
        <span>Appear offline to everyone</span>
      </label>
    </section>
  );
}
