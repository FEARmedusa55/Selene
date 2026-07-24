/** Steam-style playtime: "12.4 hours", "48 minutes", "Never played". */
export function formatPlaytime(seconds: number): string {
  if (seconds <= 0) return "Never played";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"}`;
  const hours = seconds / 3600;
  return `${hours.toFixed(1)} hours`;
}

/** Compact playtime for dense rows: "12.4 h", "48 m", "--". */
export function formatPlaytimeShort(seconds: number): string {
  if (seconds <= 0) return "--";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} m`;
  return `${(seconds / 3600).toFixed(1)} h`;
}

export function formatLastPlayed(isoDate?: string): string {
  if (!isoDate) return "Never";
  const then = new Date(isoDate);
  const days = Math.floor((Date.now() - then.getTime()) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  if (days < 30) return `${Math.floor(days / 7)} weeks ago`;
  return then.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

/** Deterministic hue from a string, so a game with no cover art still gets a
 *  stable, distinct placeholder instead of flat grey. */
export function hueFromString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 360;
  return h;
}

export function initialsOf(title: string): string {
  return title
    .replace(/[^\w\s]/g, " ")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}
