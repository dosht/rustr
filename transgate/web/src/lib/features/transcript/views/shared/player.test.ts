import { createRawSnippet } from "svelte";
import { render, screen, fireEvent } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import PlayerControls from "./PlayerControls.svelte";
import YouTubeSurface from "./YouTubeSurface.svelte";
beforeAll(() => i18n.use("en"));
beforeEach(() => localStorage.clear());
for (const phone of [false, true]) describe(`${phone ? "phone" : "desktop"} player controls`, () => {
  it("does not claim playback until the engine supplies a playing state", async () => {
    const props = { state: { playing: false, time: 0, duration: 90 }, title: fixture("TranscriptVm", "completed").title, phone, expandedEnabled: true, ontoggle: vi.fn(), onseek: vi.fn(), onskip: vi.fn(), onrate: vi.fn() };
    const { rerender } = render(PlayerControls, props);
    await userEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(screen.getByRole("button", { name: "Play" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
    await rerender({ ...props, state: { playing: true, time: 12, duration: 90 } });
    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
    expect(screen.getByTestId("player-time")).toHaveTextContent("00:00:12 / 00:01:30");
  });
});
for (const phone of [false, true]) it(`${phone ? "phone" : "desktop"} seek slider has its own name, not the Show Time toggle's`, () => {
  render(PlayerControls, { state: { playing: false, time: 5, duration: 90 }, title: "t", phone, ontoggle: vi.fn(), onseek: vi.fn(), onskip: vi.fn(), onrate: vi.fn() });
  expect(screen.queryByRole("slider", { name: "Show Time" })).toBeNull();
  expect(screen.getByRole("slider", { name: "Seek" })).toBeInTheDocument();
});
it("expansion retains the current playhead and exposes source skip and speed controls", async () => {
  const onskip = vi.fn(), onrate = vi.fn();
  render(PlayerControls, { state: { playing: true, time: 25, duration: 120 }, title: fixture("TranscriptVm", "completed").title, phone: true, expandedEnabled: true, ontoggle: vi.fn(), onseek: vi.fn(), onskip, onrate });
  await userEvent.click(screen.getByRole("button", { name: "Expand player" }));
  expect(screen.getByTestId("player-time")).toHaveTextContent("00:00:25 / 00:02:00");
  await userEvent.click(screen.getByRole("button", { name: "Skip backward 10 seconds" }));
  expect(onskip).toHaveBeenCalledWith(-10);
  await userEvent.selectOptions(screen.getByRole("combobox", { name: "Playback Speed" }), "1.5");
  expect(onrate).toHaveBeenCalledWith(1.5);
  const close = screen.getByRole("button", { name: "Close player" });
  // jsdom has no pointer-capture implementation; retain the actual pointer/click sequence.
  Object.defineProperty(close, "setPointerCapture", { value: vi.fn() });
  await userEvent.click(close);
  expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
});
it("mode switches and touch resize retain the same iframe node instead of restarting playback", async () => {
  const vm = fixture("TranscriptVm", "youtube");
  if (vm.media.kind !== "you_tube") throw new Error("YouTube fixture must project YouTube media");
  const player = createRawSnippet(() => ({ render: () => '<iframe title="Fixture player" src="about:blank"></iframe>' }));
  const controls = createRawSnippet(() => ({ render: () => '<div></div>' }));
  const props = { touch: false, videoId: vm.media.video_id, player, controls };
  const { rerender } = render(YouTubeSurface, props);
  const iframe = screen.getByTitle("Fixture player");
  await userEvent.click(screen.getByRole("button", { name: "Strip" }));
  expect(screen.getByTitle("Fixture player")).toBe(iframe);
  await userEvent.click(screen.getByRole("button", { name: "Float" }));
  expect(screen.getByTitle("Fixture player")).toBe(iframe);
  await rerender({ ...props, touch: true });
  const handle = screen.getByRole("slider", { name: "Resize video player (drag up or down)" });
  await fireEvent.keyDown(handle, { key: "ArrowDown" });
  await fireEvent.keyDown(handle, { key: "End" });
  expect(handle).toHaveAttribute("aria-valuenow", handle.getAttribute("aria-valuemax"));
  await fireEvent.keyDown(handle, { key: "ArrowUp" });
  expect(handle).toHaveAttribute("aria-valuenow", handle.getAttribute("aria-valuemax"));
  await fireEvent.keyDown(handle, { key: "Home" });
  expect(handle).toHaveAttribute("aria-valuenow", handle.getAttribute("aria-valuemin"));
  expect(screen.getByTestId("youtube-layout")).toHaveAttribute("data-mode", "strip");
  await fireEvent.keyDown(handle, { key: "ArrowUp" });
  expect(screen.getByTestId("youtube-layout")).toHaveAttribute("data-mode", "video");
  expect(screen.getByTitle("Fixture player")).toBe(iframe);
  expect(screen.queryByRole("button", { name: "Float" })).toBeNull();
});
