import { hueFromString, initialsOf } from "../format";
import { artSrc } from "../api";

interface Props {
  title: string;
  src?: string;
  variant?: "cover" | "hero";
  className?: string;
}

/* Renders cover/hero art, falling back to a deterministic generated placeholder
   when IGDB has no asset (or, in Phase 0, when nothing is fetched yet).

   The placeholder derives its hue from the title so each game keeps a stable,
   distinguishable tile instead of a wall of identical grey boxes. Saturation
   and lightness are fixed so placeholders never fight the active theme. */
export function CoverArt({ title, src, variant = "cover", className }: Props) {
  const cls = ["art", `art--${variant}`, className].filter(Boolean).join(" ");

  const resolved = artSrc(src);
  if (resolved) {
    return <img className={cls} src={resolved} alt={title} loading="lazy" draggable={false} />;
  }

  const hue = hueFromString(title);
  return (
    <div
      className={`${cls} art--placeholder`}
      style={{
        // Inline because the value is derived per-game at runtime; the
        // surrounding surface, text and border still come from tokens.
        background: `linear-gradient(150deg,
          hsl(${hue} 32% 30%) 0%,
          hsl(${(hue + 40) % 360} 28% 16%) 100%)`,
      }}
      aria-label={title}
      role="img"
    >
      <span className="art__initials">{initialsOf(title)}</span>
    </div>
  );
}
