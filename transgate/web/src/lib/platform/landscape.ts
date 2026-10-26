// Is this a phone the person physically rotated to landscape? Ported from the
// source useIsPhoneLandscape.ts (BUGS-112): screen edges and the orientation
// API, never viewport size, so an on-screen keyboard cannot trigger it; fails
// closed (false) when no signal can be read. Shell-local, like the theme.

/** Matches the source's `md` breakpoint exemption for tablets. */
export const PHONE_MAX_SCREEN_EDGE = 1060;

function rotated(win: Window): boolean {
  const type = win.screen?.orientation?.type;
  if (typeof type === "string") return type.startsWith("landscape");
  // Legacy iOS Safari (pre-16.4): window.orientation in degrees.
  const legacy = (win as Window & { orientation?: unknown }).orientation;
  return typeof legacy === "number" && Math.abs(legacy) === 90;
}

function phoneSized(win: Window): boolean {
  const width = win.screen?.width;
  const height = win.screen?.height;
  if (typeof width !== "number" || typeof height !== "number") return false;
  return Math.max(width, height) < PHONE_MAX_SCREEN_EDGE;
}

function finePointer(win: Window): boolean {
  return typeof win.matchMedia === "function" && win.matchMedia("(pointer: fine)").matches;
}

export function isPhoneLandscape(win: Window = window): boolean {
  return !finePointer(win) && phoneSized(win) && rotated(win);
}

/** Calls `report` with the current answer now and whenever it may have changed. */
export function watchPhoneLandscape(report: (landscape: boolean) => void, win: Window = window): () => void {
  const evaluate = () => report(isPhoneLandscape(win));
  evaluate();
  const orientation = win.screen?.orientation;
  win.addEventListener("orientationchange", evaluate);
  // Kept although nothing above reads the viewport: desktop device emulation
  // only re-evaluates through resize (source note).
  win.addEventListener("resize", evaluate);
  orientation?.addEventListener?.("change", evaluate);
  return () => {
    win.removeEventListener("orientationchange", evaluate);
    win.removeEventListener("resize", evaluate);
    orientation?.removeEventListener?.("change", evaluate);
  };
}
