import { describe, expect, it, vi } from "vitest";
import { browserMeasureEnv, HTML5_TIMEOUT_MS, MAX_WEB_AUDIO_BYTES, measureFile, WEB_AUDIO_TIMEOUT_MS, type MeasureEnv } from "./measure";

class FakeAudio extends EventTarget {
  preload = "";
  duration = Number.NaN;
  src = "";
}

interface Harness {
  env: MeasureEnv;
  audio: FakeAudio;
  decode: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
  revoked: string[];
  /** Fires the earliest pending timer and returns its delay. */
  fireTimer(): number;
}

function harness(options: { context?: boolean; decoded?: Promise<{ duration: number }> } = {}): Harness {
  const audio = new FakeAudio();
  const timers: Array<{ id: number; ms: number; callback: () => void }> = [];
  let next = 0;
  const revoked: string[] = [];
  const close = vi.fn(async () => {});
  const decode = vi.fn(() => options.decoded ?? new Promise(() => {}));
  const env: MeasureEnv = {
    createAudio: () => audio as unknown as HTMLAudioElement,
    createObjectURL: () => "blob:clip",
    revokeObjectURL: (url) => revoked.push(url),
    createContext: () =>
      options.context === false ? null : ({ decodeAudioData: decode, close } as unknown as ReturnType<MeasureEnv["createContext"]>),
    setTimeout: (callback, ms) => {
      timers.push({ id: ++next, ms, callback });
      return next;
    },
    clearTimeout: (id) => {
      const index = timers.findIndex((t) => t.id === id);
      if (index >= 0) timers.splice(index, 1);
    },
  };
  return {
    env,
    audio,
    decode,
    close,
    revoked,
    fireTimer() {
      const timer = timers.shift();
      if (!timer) throw new Error("no pending timer");
      timer.callback();
      return timer.ms;
    },
  };
}

function clip(size = 1024): File {
  const file = new File([new Uint8Array(8)], "clip.mp3", { type: "audio/mpeg" });
  Object.defineProperty(file, "size", { value: size });
  return file;
}

describe("measureFile", () => {
  it("settles a stalled metadata read with the real browser environment timer", async () => {
    vi.useFakeTimers();
    try {
      const h = harness({ context: false });
      const env: MeasureEnv = {
        ...browserMeasureEnv(),
        createAudio: h.env.createAudio,
        createObjectURL: h.env.createObjectURL,
        revokeObjectURL: h.env.revokeObjectURL,
        createContext: h.env.createContext,
      };
      const result = measureFile(clip(MAX_WEB_AUDIO_BYTES + 1), env);
      const settled = expect(result).resolves.toBeNull();
      await vi.advanceTimersByTimeAsync(HTML5_TIMEOUT_MS);
      await settled;
      expect(h.audio.src).toBe("");
      expect(h.revoked).toEqual(["blob:clip"]);
    } finally {
      vi.useRealTimers();
    }
  });
  it("uses HTML5 metadata when it reports a finite positive duration, releasing the object URL", async () => {
    const h = harness();
    const result = measureFile(clip(), h.env);
    expect(h.audio.preload).toBe("metadata");
    expect(h.audio.src).toBe("blob:clip");
    h.audio.duration = 150.5;
    h.audio.dispatchEvent(new Event("loadedmetadata"));
    await expect(result).resolves.toBe(150.5);
    expect(h.revoked).toEqual(["blob:clip"]);
    expect(h.audio.src).toBe("");
    expect(h.decode).not.toHaveBeenCalled();
  });

  it("falls back to a Web Audio decode when metadata times out after 5 s", async () => {
    const h = harness({ decoded: Promise.resolve({ duration: 61 }) });
    const result = measureFile(clip(), h.env);
    expect(h.fireTimer()).toBe(HTML5_TIMEOUT_MS);
    await expect(result).resolves.toBe(61);
    expect(h.decode).toHaveBeenCalledOnce();
    expect(h.close).toHaveBeenCalledOnce();
  });

  it("treats an infinite metadata duration and a media error as failures of the first method", async () => {
    for (const trigger of ["infinite", "error"] as const) {
      const h = harness({ decoded: Promise.resolve({ duration: 30 }) });
      const result = measureFile(clip(), h.env);
      if (trigger === "infinite") {
        h.audio.duration = Number.POSITIVE_INFINITY;
        h.audio.dispatchEvent(new Event("loadedmetadata"));
      } else {
        h.audio.dispatchEvent(new Event("error"));
      }
      await expect(result).resolves.toBe(30);
    }
  });

  it("never decodes a file above 100 MiB", async () => {
    const h = harness({ decoded: Promise.resolve({ duration: 30 }) });
    const result = measureFile(clip(MAX_WEB_AUDIO_BYTES + 1), h.env);
    h.audio.dispatchEvent(new Event("error"));
    await expect(result).resolves.toBeNull();
    expect(h.decode).not.toHaveBeenCalled();
  });

  it("decodes a file of exactly 100 MiB", async () => {
    const h = harness({ decoded: Promise.resolve({ duration: 12 }) });
    const result = measureFile(clip(MAX_WEB_AUDIO_BYTES), h.env);
    h.audio.dispatchEvent(new Event("error"));
    await expect(result).resolves.toBe(12);
  });

  it("gives up with null when the decode does not finish in 10 s, and closes the context", async () => {
    const h = harness();
    const result = measureFile(clip(), h.env);
    h.audio.dispatchEvent(new Event("error"));
    // The decode timeout starts only after the file has been read.
    await vi.waitFor(() => expect(h.decode).toHaveBeenCalledOnce());
    expect(h.fireTimer()).toBe(WEB_AUDIO_TIMEOUT_MS);
    await expect(result).resolves.toBeNull();
    expect(h.close).toHaveBeenCalledOnce();
  });

  it("returns null when the decode rejects or reports no usable length, and when Web Audio is absent", async () => {
    const rejected = harness({ decoded: Promise.reject(new Error("bad data")) });
    const a = measureFile(clip(), rejected.env);
    rejected.audio.dispatchEvent(new Event("error"));
    await expect(a).resolves.toBeNull();

    const empty = harness({ decoded: Promise.resolve({ duration: 0 }) });
    const b = measureFile(clip(), empty.env);
    empty.audio.dispatchEvent(new Event("error"));
    await expect(b).resolves.toBeNull();

    const none = harness({ context: false });
    const c = measureFile(clip(), none.env);
    none.audio.dispatchEvent(new Event("error"));
    await expect(c).resolves.toBeNull();
  });

  it("ignores a late metadata event after the timeout already decided", async () => {
    const h = harness({ decoded: Promise.resolve({ duration: 5 }) });
    const result = measureFile(clip(), h.env);
    h.fireTimer();
    h.audio.duration = 999;
    h.audio.dispatchEvent(new Event("loadedmetadata"));
    await expect(result).resolves.toBe(5);
    expect(h.revoked).toEqual(["blob:clip"]);
  });
});
