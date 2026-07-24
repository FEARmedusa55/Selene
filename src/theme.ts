/* Theme registry.

   A theme is a CSS file declaring tokens under [data-theme="<id>"], plus one
   entry in BUILTIN_THEMES. Applying a theme sets a single attribute on <html>
   -- there is no per-component theme logic anywhere in the app, which is what
   keeps "a theme is one file, no code" true.

   Phase 6 adds user-authored themes by loading token files from the config
   directory and appending them to this list at runtime. */

export interface ThemeMeta {
  id: string;
  name: string;
  description: string;
  /** Drives the swatch shown in Settings without hardcoding colors in TSX. */
  preview: { bg: string; surface: string; accent: string };
}

export const BUILTIN_THEMES: ThemeMeta[] = [
  {
    id: "lunar",
    name: "Lunar",
    description: "Deep violet night, drawn from the app icon. The default.",
    preview: { bg: "#15102a", surface: "#271f47", accent: "#a06ef5" },
  },
  {
    id: "steam-dark",
    name: "Dark",
    description: "Blue-tinted charcoal with a cyan accent.",
    preview: { bg: "#1b2838", surface: "#2a3f5a", accent: "#66c0f4" },
  },
  {
    id: "daylight",
    name: "Bright",
    description: "Light theme for bright rooms.",
    preview: { bg: "#f2f4f7", surface: "#ffffff", accent: "#1a6ea8" },
  },
];

export const DEFAULT_THEME = "lunar";

const STORAGE_KEY = "launcher.theme";

export function applyTheme(id: string): void {
  document.documentElement.setAttribute("data-theme", id);
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    // Private/locked-down storage: theme still applies for this session.
  }
}

export function loadStoredTheme(): string {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) return stored; // may be a user theme; validated once themes load
  } catch {
    // fall through to default
  }
  return DEFAULT_THEME;
}

/** Pull a token value out of a theme's CSS, for the Settings swatch. */
function tokenValue(css: string, name: string): string | undefined {
  const m = new RegExp(`--${name}\\s*:\\s*([^;]+);`).exec(css);
  return m?.[1].trim();
}

export interface UserTheme {
  id: string;
  name: string;
  css: string;
}

/** Inject user themes' CSS into the document and return picker metadata.
 *
 *  The CSS already scopes its tokens under [data-theme="<id>"], so injecting it
 *  is all that is needed — applyTheme(id) then works identically to a built-in.
 *  This keeps the "a theme is one file, no code" promise for user themes too. */
export function registerUserThemes(themes: UserTheme[]): ThemeMeta[] {
  const STYLE_ID = "user-themes";
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    document.head.appendChild(style);
  }
  style.textContent = themes.map((t) => t.css).join("\n\n");

  return themes.map((t) => ({
    id: t.id,
    name: t.name,
    description: "User theme",
    preview: {
      bg: tokenValue(t.css, "color-bg-base") ?? "#222",
      surface: tokenValue(t.css, "color-surface") ?? "#333",
      accent: tokenValue(t.css, "color-accent") ?? "#888",
    },
  }));
}
