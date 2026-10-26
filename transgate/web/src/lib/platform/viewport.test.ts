import { afterEach, describe, expect, it, vi } from "vitest";
import { measureViewport, watchViewport, type ViewportFacts } from "./viewport.svelte";

function resizeTo(width: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  window.dispatchEvent(new Event("resize"));
}

afterEach(() => {
  vi.useRealTimers();
  Object.defineProperty(window.navigator, "maxTouchPoints", { configurable: true, value: 0 });
});

describe("viewport facts", () => {
  it("reports touch from maxTouchPoints, as the source does", () => {
    Object.defineProperty(window.navigator, "maxTouchPoints", { configurable: true, value: 5 });
    expect(measureViewport().touch).toBe(true);
    Object.defineProperty(window.navigator, "maxTouchPoints", { configurable: true, value: 0 });
    expect(measureViewport().touch).toBe("ontouchstart" in window);
  });

  it("reports nothing at start, then one report per frame and only on change", () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    resizeTo(1280);
    const seen: ViewportFacts[] = [];
    const stop = watchViewport((facts) => seen.push(facts));
    expect(seen).toEqual([]);

    resizeTo(900);
    resizeTo(700);
    vi.advanceTimersToNextFrame();
    expect(seen.map((f) => f.width)).toEqual([700]);

    resizeTo(700);
    vi.advanceTimersToNextFrame();
    expect(seen).toHaveLength(1);

    stop();
    resizeTo(500);
    vi.advanceTimersToNextFrame();
    expect(seen).toHaveLength(1);
  });
});
