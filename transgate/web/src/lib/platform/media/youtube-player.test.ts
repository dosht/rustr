import { describe, expect, it, vi } from "vitest";
import {
  IFRAME_API_URL,
  loadYouTubeIframeApi,
  mountYouTube,
  POLL_MS,
  YouTubePlayer,
  YT_BUFFERING,
  YT_ENDED,
  YT_PAUSED,
  YT_PLAYING,
  type Timers,
  type YouTubeApiPlayer,
  type YouTubeNamespace,
  type YouTubePlayerOptions,
} from "./youtube-player";

function api(overrides: Partial<YouTubeApiPlayer> = {}): YouTubeApiPlayer & { time: number; length: number } {
  const state: YouTubeApiPlayer & { time: number; length: number } = {
    time: 0,
    length: 0,
    playVideo: vi.fn(),
    pauseVideo: vi.fn(),
    seekTo: vi.fn(),
    destroy: vi.fn(),
    getCurrentTime() {
      return state.time;
    },
    getDuration() {
      return state.length;
    },
    ...overrides,
  };
  return state;
}

function timers() {
  const intervals = new Map<number, { ms: number; callback: () => void }>();
  let next = 0;
  const fake: Timers & { tick(): void; intervals: typeof intervals } = {
    intervals,
    setInterval: (callback, ms) => {
      intervals.set(++next, { ms, callback });
      return next;
    },
    clearInterval: (id) => {
      intervals.delete(id as number);
    },
    tick() {
      for (const { callback } of intervals.values()) callback();
    },
  };
  return fake;
}

describe("YouTubePlayer", () => {
  it("treats BUFFERING as playing and polls time every 250 ms only while playing", () => {
    const yt = api({ getDuration: () => 0 });
    const clock = timers();
    const player = new YouTubePlayer(yt, clock);
    expect(player.duration()).toBeNull();
    player.handleStateChange(YT_BUFFERING);
    player.handleStateChange(YT_PLAYING);
    expect(player.isPlaying()).toBe(true);
    expect([...clock.intervals.values()].map((i) => i.ms)).toEqual([POLL_MS]);
    yt.time = 3.25;
    clock.tick();
    expect(player.currentTime()).toBe(3.25);
  });

  it("takes a final time sample and stops polling on pause or end", () => {
    const yt = api();
    const clock = timers();
    const player = new YouTubePlayer(yt, clock);
    player.handleStateChange(YT_PLAYING);
    yt.time = 7;
    player.handleStateChange(YT_PAUSED);
    expect([player.isPlaying(), player.currentTime(), clock.intervals.size]).toEqual([false, 7, 0]);
    player.handleStateChange(YT_PLAYING);
    player.handleStateChange(YT_ENDED);
    expect(clock.intervals.size).toBe(0);
  });

  it("reads the duration at construction or on the first play", () => {
    const yt = api();
    const player = new YouTubePlayer(yt, timers());
    expect(player.duration()).toBeNull();
    yt.length = 300;
    player.handleStateChange(YT_PLAYING);
    expect(player.duration()).toBe(300);
    yt.length = 300;
    expect(new YouTubePlayer(yt, timers()).duration()).toBe(300);
  });

  it("reflects play, pause and seek intent at once and survives a not-ready iframe", async () => {
    const yt = api();
    const player = new YouTubePlayer(yt, timers());
    await player.play();
    expect([yt.playVideo, player.isPlaying()]).toEqual([yt.playVideo, true]);
    player.seek(42);
    expect(yt.seekTo).toHaveBeenCalledWith(42, true);
    expect(player.currentTime()).toBe(42);
    player.pause();
    expect(player.isPlaying()).toBe(false);

    const broken = api({
      playVideo: () => {
        throw new Error("not ready");
      },
      getDuration: () => {
        throw new Error("not ready");
      },
    });
    const notReady = new YouTubePlayer(broken, timers());
    await notReady.play();
    expect(notReady.isPlaying()).toBe(false);
  });

  it("destroy stops polling without destroying the iframe it does not own", () => {
    const yt = api();
    const clock = timers();
    const player = new YouTubePlayer(yt, clock);
    player.handleStateChange(YT_PLAYING);
    player.destroy();
    expect(clock.intervals.size).toBe(0);
    expect(yt.destroy).not.toHaveBeenCalled();
  });
});

describe("mountYouTube", () => {
  it("creates one iframe with the source options, adapts it on ready, and disposes both", async () => {
    let created: YouTubePlayerOptions | null = null;
    const iframe = api();
    const namespace: YouTubeNamespace = {
      Player: class {
        constructor(_host: HTMLElement, options: YouTubePlayerOptions) {
          created = options;
          queueMicrotask(() => options.events.onReady({ target: iframe }));
          return iframe;
        }
      } as unknown as YouTubeNamespace["Player"],
    };
    const clock = timers();
    const mount = await mountYouTube(document.createElement("div"), "abc123", namespace, clock);
    expect(created).toMatchObject({ videoId: "abc123", width: "100%", playerVars: { autoplay: 0, rel: 0, modestbranding: 1 } });
    created!.events.onStateChange({ data: YT_PLAYING });
    expect(mount.player.isPlaying()).toBe(true);
    mount.dispose();
    expect(clock.intervals.size).toBe(0);
    expect(iframe.destroy).toHaveBeenCalledOnce();
  });

  it("destroys an iframe abandoned before ready and ignores a late ready event", async () => {
    const iframe = api();
    let options!: YouTubePlayerOptions;
    const namespace: YouTubeNamespace = {
      Player: class {
        constructor(_host: HTMLElement, value: YouTubePlayerOptions) { options = value; return iframe; }
      } as unknown as YouTubeNamespace["Player"],
    };
    const abort = new AbortController();
    const pending = mountYouTube(document.createElement("div"), "abandoned", namespace, undefined, abort.signal);
    const rejected = expect(pending).rejects.toMatchObject({ name: "AbortError" });
    abort.abort();
    await rejected;
    options.events.onReady({ target: iframe });
    expect(iframe.destroy).toHaveBeenCalledOnce();
  });
});

describe("loadYouTubeIframeApi", () => {
  it("uses an already loaded API without injecting a script", async () => {
    const namespace = { Player: class {} };
    const win = { YT: namespace } as unknown as Window;
    const doc = document.implementation.createHTMLDocument();
    await expect(loadYouTubeIframeApi(win, doc)).resolves.toBe(namespace);
    expect(doc.querySelectorAll("script")).toHaveLength(0);
  });

  it("injects the iframe API once and resolves on its ready callback, keeping a previous callback", async () => {
    const previous = vi.fn();
    const win = { onYouTubeIframeAPIReady: previous } as unknown as Window & { YT?: unknown; onYouTubeIframeAPIReady?: () => void };
    const doc = document.implementation.createHTMLDocument();
    const first = loadYouTubeIframeApi(win, doc);
    const second = loadYouTubeIframeApi(win, doc);
    expect(second).toBe(first);
    const scripts = doc.querySelectorAll("script");
    expect([...scripts].map((s) => s.src)).toEqual([IFRAME_API_URL]);
    const namespace = { Player: class {} };
    win.YT = namespace;
    win.onYouTubeIframeAPIReady?.();
    await expect(first).resolves.toBe(namespace);
    expect(previous).toHaveBeenCalledOnce();
  });
});
