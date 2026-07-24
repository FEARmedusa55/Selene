/* Gamepad menu navigation via the Web Gamepad API (WebView2 supports it).

   Focus moves spatially — pressing a direction jumps to the nearest focusable
   element that way, which is what a grid + sidebar layout needs, rather than
   DOM order. Reuses the app's existing focus ring so controller focus looks
   identical to keyboard focus. */

type Dir = "up" | "down" | "left" | "right";

export interface GamepadHandlers {
  /** B / Circle — back out of the current view. */
  onBack: () => void;
  /** Bumpers — switch tab by -1 / +1. */
  onTab: (delta: number) => void;
}

const AXIS_DEADZONE = 0.55;
const REPEAT_DELAY = 380; // ms before a held direction repeats
const REPEAT_RATE = 140; // ms between repeats

/** Visible, enabled, focusable elements in DOM order. */
function focusables(): HTMLElement[] {
  const sel =
    'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';
  return Array.from(document.querySelectorAll<HTMLElement>(sel)).filter(
    (el) => el.offsetParent !== null && el.getClientRects().length > 0,
  );
}

/** Move focus to the nearest focusable element in `dir`. */
function moveFocus(dir: Dir): void {
  const list = focusables();
  if (list.length === 0) return;

  const cur = document.activeElement as HTMLElement | null;
  if (!cur || !list.includes(cur)) {
    list[0].focus();
    list[0].scrollIntoView({ block: "nearest", inline: "nearest" });
    return;
  }

  const c = cur.getBoundingClientRect();
  const ccx = c.left + c.width / 2;
  const ccy = c.top + c.height / 2;

  let best: HTMLElement | null = null;
  let bestScore = Infinity;

  for (const el of list) {
    if (el === cur) continue;
    const r = el.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height / 2;
    const dx = cx - ccx;
    const dy = cy - ccy;

    // Primary = distance along the pressed axis; the candidate must lie that
    // way. Secondary = how far off the perpendicular axis it sits, penalised
    // so the move stays roughly in line.
    let primary: number;
    let secondary: number;
    switch (dir) {
      case "right":
        if (dx <= 1) continue;
        primary = dx;
        secondary = Math.abs(dy);
        break;
      case "left":
        if (dx >= -1) continue;
        primary = -dx;
        secondary = Math.abs(dy);
        break;
      case "down":
        if (dy <= 1) continue;
        primary = dy;
        secondary = Math.abs(dx);
        break;
      case "up":
        if (dy >= -1) continue;
        primary = -dy;
        secondary = Math.abs(dx);
        break;
    }

    const score = primary + secondary * 2.2;
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }

  if (best) {
    best.focus();
    best.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
}

/** Start polling. Returns a stop function. */
export function startGamepad(handlers: GamepadHandlers): () => void {
  let raf = 0;
  // Previous pressed state, per (padIndex, buttonIndex).
  const wasPressed = new Map<string, boolean>();
  // For held directions: when the next repeat is allowed.
  const nextRepeat = new Map<Dir, number>();

  const setUsingGamepad = () => document.documentElement.classList.add("using-gamepad");

  /** Rising-edge detect for a button. */
  const edge = (padIdx: number, btn: number, pressed: boolean): boolean => {
    const key = `${padIdx}:${btn}`;
    const before = wasPressed.get(key) ?? false;
    wasPressed.set(key, pressed);
    return pressed && !before;
  };

  const poll = (t: number) => {
    const pads = navigator.getGamepads?.() ?? [];
    for (let p = 0; p < pads.length; p++) {
      const pad = pads[p];
      if (!pad) continue;

      const btn = (i: number) => pad.buttons[i]?.pressed ?? false;

      // --- directional: d-pad OR left stick, with repeat while held ---
      const dirs: Record<Dir, boolean> = {
        up: btn(12) || pad.axes[1] < -AXIS_DEADZONE,
        down: btn(13) || pad.axes[1] > AXIS_DEADZONE,
        left: btn(14) || pad.axes[0] < -AXIS_DEADZONE,
        right: btn(15) || pad.axes[0] > AXIS_DEADZONE,
      };
      for (const d of ["up", "down", "left", "right"] as Dir[]) {
        if (dirs[d]) {
          const due = nextRepeat.get(d) ?? 0;
          if (t >= due) {
            setUsingGamepad();
            moveFocus(d);
            // First press waits longer than subsequent repeats.
            nextRepeat.set(d, t + (nextRepeat.has(d) ? REPEAT_RATE : REPEAT_DELAY));
          }
        } else {
          nextRepeat.delete(d);
        }
      }

      // --- actions (rising edge only) ---
      if (edge(p, 0, btn(0))) {
        // A / Cross — activate.
        setUsingGamepad();
        (document.activeElement as HTMLElement | null)?.click();
      }
      if (edge(p, 1, btn(1))) handlers.onBack(); // B / Circle
      if (edge(p, 4, btn(4))) handlers.onTab(-1); // LB
      if (edge(p, 5, btn(5))) handlers.onTab(1); // RB
    }
    raf = requestAnimationFrame(poll);
  };

  raf = requestAnimationFrame(poll);
  return () => cancelAnimationFrame(raf);
}
