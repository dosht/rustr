// Measures the two raw viewport facts the source layouts depend on and
// reports them. It decides nothing: core derives the frame, transcript and
// YouTube layouts from these facts plus session flags (source:
// useDeviceType.ts:79-92 touch detection, useMobileView.ts:19-20 width).

export interface ViewportFacts {
  /** Touch capable, as the source tests it: `ontouchstart` or `maxTouchPoints > 0`. */
  touch: boolean;
  /** `window.innerWidth` in CSS pixels. */
  width: number;
}

export function measureViewport(win: Window = window): ViewportFacts {
  const nav = win.navigator as Navigator & { msMaxTouchPoints?: number };
  return {
    touch: "ontouchstart" in win || nav.maxTouchPoints > 0 || (nav.msMaxTouchPoints ?? 0) > 0,
    width: Math.round(win.innerWidth),
  };
}

/**
 * Reports changes after start, at most once per animation frame and only when
 * a fact actually changed. The initial facts are not reported: the presenter
 * measures them itself at start (p22). Returns the stop function.
 */
export function watchViewport(report: (facts: ViewportFacts) => void, win: Window = window): () => void {
  let last = measureViewport(win);
  let frame: number | null = null;
  const check = () => {
    frame = null;
    const next = measureViewport(win);
    if (next.touch === last.touch && next.width === last.width) return;
    last = next;
    report(next);
  };
  const schedule = () => {
    frame ??= win.requestAnimationFrame(check);
  };
  win.addEventListener("resize", schedule);
  win.screen?.orientation?.addEventListener?.("change", schedule);
  return () => {
    win.removeEventListener("resize", schedule);
    win.screen?.orientation?.removeEventListener?.("change", schedule);
    if (frame !== null) win.cancelAnimationFrame(frame);
  };
}
