import { describe, expect, it, vi } from "vitest";
import { AudioElementPlayer } from "./audio-player";
import { activeLineIndex, clockTime, PlayerSlot, PlayerState, type MountedPlayer } from "./player";
import { createMediaPlatform } from "./index";

class FakeMedia extends EventTarget {
  paused = true;
  ended = false;
  currentTime = 0;
  duration = Number.NaN;
  play = vi.fn(async () => {});
  pause = vi.fn();
}

function audio(media = new FakeMedia(), onError?: () => void) {
  return { media, player: new AudioElementPlayer(media as unknown as HTMLAudioElement, onError) };
}

describe("clockTime", () => {
  it("formats HH:MM:SS with whole seconds rounded down", () => {
    expect(clockTime(0)).toBe("00:00:00");
    expect(clockTime(59.99)).toBe("00:00:59");
    expect(clockTime(3661.5)).toBe("01:01:01");
    expect(clockTime(Number.NaN)).toBe("00:00:00");
  });
});

describe("activeLineIndex", () => {
  const ends = [2, 5, 9];
  it("follows the source rule: first line whose end is at or after the time", () => {
    expect(activeLineIndex(ends, 0)).toBe(-1);
    expect(activeLineIndex(ends, 0.1)).toBe(0);
    expect(activeLineIndex(ends, 2)).toBe(0);
    expect(activeLineIndex(ends, 2.01)).toBe(1);
    expect(activeLineIndex(ends, 9)).toBe(2);
    expect(activeLineIndex(ends, 9.5)).toBe(-1);
    expect(activeLineIndex([], 3)).toBe(-1);
  });
});

describe("PlayerState", () => {
  it("notifies only on an actual change", () => {
    const state = new PlayerState();
    const seen = vi.fn();
    state.subscribe(seen);
    state.set({ playing: false, time: 0 });
    state.set({ time: 1 });
    state.set({ time: 1 });
    expect(seen).toHaveBeenCalledOnce();
    expect(seen).toHaveBeenCalledWith({ playing: false, time: 1, duration: null });
  });
});

describe("AudioElementPlayer", () => {
  it("takes its state only from media events", () => {
    const { media, player } = audio();
    expect(player.snapshot()).toEqual({ playing: false, time: 0, duration: null });
    media.dispatchEvent(new Event("play"));
    media.currentTime = 4.5;
    media.dispatchEvent(new Event("timeupdate"));
    media.duration = 120;
    media.dispatchEvent(new Event("loadedmetadata"));
    expect([player.isPlaying(), player.currentTime(), player.duration()]).toEqual([true, 4.5, 120]);
    media.dispatchEvent(new Event("ended"));
    expect(player.isPlaying()).toBe(false);
  });

  it("does not mark playing when play() is refused, and seeks the element", async () => {
    const media = new FakeMedia();
    media.play = vi.fn(async () => {
      throw new DOMException("blocked", "NotAllowedError");
    });
    const { player } = audio(media);
    await expect(player.play()).resolves.toBeUndefined();
    expect(player.isPlaying()).toBe(false);
    player.seek(12);
    expect(media.currentTime).toBe(12);
    player.pause();
    expect(media.pause).toHaveBeenCalledOnce();
  });

  it("reports a load error so the view can ask for a fresh streaming URL", () => {
    const onError = vi.fn();
    const { media } = audio(new FakeMedia(), onError);
    media.dispatchEvent(new Event("error"));
    expect(onError).toHaveBeenCalledOnce();
  });

  it("stops listening after destroy", () => {
    const { media, player } = audio();
    const seen = vi.fn();
    player.subscribe(seen);
    player.destroy();
    media.dispatchEvent(new Event("play"));
    expect(player.isPlaying()).toBe(false);
    expect(seen).not.toHaveBeenCalled();
  });
});

function fake(kind: "audio" | "youtube" = "audio"): MountedPlayer {
  return {
    kind,
    play: async () => {},
    pause: () => {},
    seek: () => {},
    isPlaying: () => false,
    currentTime: () => 0,
    duration: () => null,
    snapshot: () => ({ playing: false, time: 0, duration: null }),
    subscribe: () => () => {},
    destroy: vi.fn(),
  };
}

describe("PlayerSlot", () => {
  it("keeps exactly one mounted player and destroys the replaced one", () => {
    const slot = new PlayerSlot();
    const first = fake();
    const second = fake("youtube");
    const detachFirst = slot.attach(first);
    slot.attach(second);
    expect(first.destroy).toHaveBeenCalledOnce();
    expect(slot.current()).toBe(second);
    detachFirst();
    expect(slot.current()).toBe(second);
    expect(second.destroy).not.toHaveBeenCalled();
  });

  it("detaches and destroys the mounted player", () => {
    const slot = new PlayerSlot();
    const player = fake();
    const detach = slot.attach(player);
    detach();
    expect(slot.current()).toBeNull();
    expect(player.destroy).toHaveBeenCalledOnce();
  });
});

describe("createMediaPlatform", () => {
  it("resolves handles to held Files and refuses unknown or forgotten ones without I/O", async () => {
    const media = createMediaPlatform({ createRequest: () => { throw new Error("no request expected"); } });
    const [held] = media.files.hold([new File(["x"], "a.mp3", { type: "audio/mpeg" })]);
    expect(held).toEqual({ handle: 1, name: "a.mp3", size: 1, mime: "audio/mpeg" });
    media.forget(held.handle);
    await expect(media.measure(held.handle)).resolves.toBeNull();
    await expect(media.put(held.handle, "https://storage.test/x", () => {})).resolves.toEqual({ ok: false, status: 0 });
    await expect(media.put(99, "https://storage.test/x", () => {})).resolves.toEqual({ ok: false, status: 0 });
  });

  it("gives distinct handles to same-named files and exposes the mounted player", () => {
    const media = createMediaPlatform();
    const held = media.files.hold([new File(["a"], "same.mp3"), new File(["b"], "same.mp3")]);
    expect(held.map((h) => h.handle)).toEqual([1, 2]);
    expect(media.player()).toBeNull();
    const player = fake();
    media.players.attach(player);
    expect(media.player()).toBe(player);
  });
});
