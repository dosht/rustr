import { render } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { createMediaPlatform } from "$lib/platform/media";
import { i18n } from "$lib/i18n/index.svelte";
import HomeContent from "./HomeContent.svelte";

beforeAll(() => i18n.use("en"));
afterEach(() => vi.unstubAllGlobals());

class FakeObserver {
  static live: FakeObserver[] = [];
  constructor(private readonly callback: (entries: { isIntersecting: boolean }[]) => void) { FakeObserver.live.push(this); }
  observe() {}
  disconnect() {}
  reveal(isIntersecting: boolean) { this.callback([{ isIntersecting }]); }
}

const allTranscripts = () => {
  const base = fixture("SignedInVm", "desktop_new");
  return { ...base, transcript: null, route: { page: "home", view: { kind: "all_transcripts" } }, jobs: { ...base.jobs, can_load_more: true, loading_more: false } } as typeof base;
};

it("desktop all-transcripts list asks core for the next page only when its end marker scrolls into view", () => {
  FakeObserver.live = [];
  vi.stubGlobal("IntersectionObserver", FakeObserver);
  const dispatch = vi.fn();
  render(HomeContent, { vm: allTranscripts(), dispatch, media: createMediaPlatform(), menuOpen: false });
  expect(FakeObserver.live).toHaveLength(1);
  FakeObserver.live[0].reveal(false);
  expect(dispatch).not.toHaveBeenCalled();
  FakeObserver.live[0].reveal(true);
  expect(dispatch).toHaveBeenCalledWith({ type: "jobs", intent: { type: "load_more" } });
});
