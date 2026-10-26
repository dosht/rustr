import { describe, expect, it, vi } from "vitest";
import { decorativePeaks, mountWaveform, type CreateWaveSurfer, type WaveSurferLike } from "./waveform-player";

type Listener = (value?: number) => void;

function surfer(length = 0) {
  const listeners = new Map<string, Set<Listener>>();
  const media = new EventTarget() as HTMLMediaElement;
  const fake = {
    length,
    emit(event: string, value?: number) {
      for (const listener of listeners.get(event) ?? []) listener(value);
    },
    listenerCount: () => [...listeners.values()].reduce((n, set) => n + set.size, 0),
    on(event: string, listener: Listener) {
      const set = listeners.get(event) ?? new Set<Listener>();
      set.add(listener);
      listeners.set(event, set);
      return () => set.delete(listener);
    },
    play: vi.fn(async () => {}),
    pause: vi.fn(),
    setTime: vi.fn(),
    getDuration: () => fake.length,
    getMediaElement: () => media,
    destroy: vi.fn(),
    media,
  };
  return fake;
}

function mount(fake = surfer(), onLoadError?: () => void, peaks: number[] = [0.2, 0.4]) {
  let options: Parameters<CreateWaveSurfer>[0] | null = null;
  const create: CreateWaveSurfer = (settings) => {
    options = settings;
    return fake as unknown as WaveSurferLike;
  };
  const result = mountWaveform(
    { container: document.createElement("div"), url: "https://storage.test/audio.mp3", peaks, duration: 3, waveColor: "#ccc", progressColor: "#00f" },
    onLoadError,
    create,
  );
  return { ...result, fake, options: options! };
}

describe("mountWaveform", () => {
  it("creates wavesurfer with the source options, at least 20 s and a fallback peak", () => {
    const { options } = mount(surfer(), undefined, []);
    expect(options).toMatchObject({
      url: "https://storage.test/audio.mp3",
      height: 40,
      dragToSeek: true,
      backend: "MediaElement",
      peaks: [[0.5]],
      duration: 20,
      waveColor: "#ccc",
      progressColor: "#00f",
    });
  });

  it("takes state only from engine events and reports media load errors", () => {
    const onError = vi.fn();
    const { player, fake } = mount(surfer(95), onError);
    fake.emit("play");
    fake.emit("timeupdate", 12.5);
    fake.emit("ready");
    expect([player.isPlaying(), player.currentTime(), player.duration()]).toEqual([true, 12.5, 95]);
    fake.emit("finish");
    expect(player.isPlaying()).toBe(false);
    fake.media.dispatchEvent(new Event("error"));
    expect(onError).toHaveBeenCalledOnce();
  });

  it("seeks through setTime and swallows a refused play", async () => {
    const fake = surfer();
    fake.play = vi.fn(async () => {
      throw new Error("NotAllowedError");
    });
    const { player } = mount(fake);
    await expect(player.play()).resolves.toBeUndefined();
    expect(player.isPlaying()).toBe(false);
    player.seek(30);
    expect(fake.setTime).toHaveBeenCalledWith(30);
  });

  it("dispose unsubscribes every listener and destroys the instance", () => {
    const onError = vi.fn();
    const { dispose, fake } = mount(surfer(), onError);
    dispose();
    expect(fake.listenerCount()).toBe(0);
    expect(fake.destroy).toHaveBeenCalledOnce();
    fake.media.dispatchEvent(new Event("error"));
    expect(onError).not.toHaveBeenCalled();
  });
});

describe("decorativePeaks", () => {
  // Park-Miller minimal standard LCG from seed 1: 16807, 282475249, 1622650073.
  const raw = [16807, 282475249, 1622650073].map((v) => ((v - 1) / 2147483646) * 2 - 1);

  it("follows the source LCG and leaves values unsmoothed with a window of 1", () => {
    expect(decorativePeaks(1, 1, 3, 1)).toEqual(raw);
  });

  it("smooths with the source centered moving average, shorter at the edges", () => {
    const smoothed = decorativePeaks(1, 1, 3, 3);
    expect(smoothed[0]).toBeCloseTo((raw[0] + raw[1]) / 2, 12);
    expect(smoothed[1]).toBeCloseTo((raw[0] + raw[1] + raw[2]) / 3, 12);
    expect(smoothed[2]).toBeCloseTo((raw[1] + raw[2]) / 2, 12);
  });

  it("is deterministic per job, 60 samples per unit by default and bounded", () => {
    const peaks = decorativePeaks(4321, 20);
    expect(peaks).toHaveLength(1200);
    expect(decorativePeaks(4321, 20)).toEqual(peaks);
    expect(decorativePeaks(4322, 20)).not.toEqual(peaks);
    expect(peaks.every((p) => p >= -1 && p <= 1)).toBe(true);
  });
});
