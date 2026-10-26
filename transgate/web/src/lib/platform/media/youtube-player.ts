// The YouTube iframe player, as the source YouTubeMediaPlayer/YouTubeMount:
// BUFFERING counts as playing, time is polled every 250 ms only while playing,
// and play/pause/seek reflect intent at once until the next state event.

import { knownDuration, PlayerState, type MountedPlayer, type PlayerSnapshot } from "./player";

export const YT_ENDED = 0;
export const YT_PLAYING = 1;
export const YT_PAUSED = 2;
export const YT_BUFFERING = 3;
export const POLL_MS = 250;
export const IFRAME_API_URL = "https://www.youtube.com/iframe_api";

/** The subset of `YT.Player` used here. */
export interface YouTubeApiPlayer {
  playVideo(): void;
  pauseVideo(): void;
  seekTo(seconds: number, allowSeekAhead: boolean): void;
  getCurrentTime(): number;
  getDuration(): number;
  destroy(): void;
}

export interface YouTubePlayerOptions {
  videoId: string;
  width: string;
  playerVars: Record<string, number>;
  events: {
    onReady(event: { target: YouTubeApiPlayer }): void;
    onStateChange(event: { data: number }): void;
  };
}

export interface YouTubeNamespace {
  Player: new (host: HTMLElement, options: YouTubePlayerOptions) => YouTubeApiPlayer;
}

export interface Timers {
  setInterval(callback: () => void, ms: number): unknown;
  clearInterval(id: unknown): void;
}

const browserTimers: Timers = {
  setInterval: (callback, ms) => globalThis.setInterval(callback, ms),
  clearInterval: (id) => globalThis.clearInterval(id as number),
};

export class YouTubePlayer implements MountedPlayer {
  readonly kind = "youtube" as const;
  readonly #state = new PlayerState();
  #poll: unknown = null;

  constructor(
    private readonly api: YouTubeApiPlayer,
    private readonly timers: Timers = browserTimers,
  ) {
    this.#refreshDuration();
  }

  #refreshDuration(): void {
    try {
      const duration = knownDuration(this.api.getDuration());
      if (duration !== null) this.#state.set({ duration });
    } catch {
      // Not ready yet; a later state change surfaces it.
    }
  }

  #sample(): void {
    try {
      const time = this.api.getCurrentTime();
      if (typeof time === "number" && !Number.isNaN(time)) this.#state.set({ time });
    } catch {
      // Not ready or disposed.
    }
  }

  #stopPolling(): void {
    if (this.#poll === null) return;
    this.timers.clearInterval(this.#poll);
    this.#poll = null;
  }

  /** Called by the mount on every iframe state change. */
  handleStateChange(state: number): void {
    if (state === YT_PLAYING || state === YT_BUFFERING) {
      this.#state.set({ playing: true });
      if (this.#state.get().duration === null) this.#refreshDuration();
      if (this.#poll === null) this.#poll = this.timers.setInterval(() => this.#sample(), POLL_MS);
    } else if (state === YT_PAUSED || state === YT_ENDED) {
      this.#state.set({ playing: false });
      this.#stopPolling();
      this.#sample();
    }
  }

  async play(): Promise<void> {
    try {
      this.api.playVideo();
      this.#state.set({ playing: true });
    } catch {
      // Not ready.
    }
  }

  pause(): void {
    try {
      this.api.pauseVideo();
      this.#state.set({ playing: false });
    } catch {
      // Not ready.
    }
  }

  seek(seconds: number): void {
    try {
      this.api.seekTo(seconds, true);
      this.#state.set({ time: seconds });
    } catch {
      // Not ready.
    }
  }

  isPlaying(): boolean {
    return this.#state.get().playing;
  }

  currentTime(): number {
    return this.#state.get().time;
  }

  duration(): number | null {
    return this.#state.get().duration;
  }

  snapshot(): PlayerSnapshot {
    return this.#state.get();
  }

  subscribe(listener: (snapshot: PlayerSnapshot) => void): () => void {
    return this.#state.subscribe(listener);
  }

  /** Stops polling; the iframe belongs to its mount. */
  destroy(): void {
    this.#stopPolling();
    this.#state.clear();
  }
}

interface YouTubeWindow {
  YT?: Partial<YouTubeNamespace>;
  onYouTubeIframeAPIReady?: () => void;
}

let loading: Promise<YouTubeNamespace> | null = null;

/** Loads the iframe API script once per page. */
export function loadYouTubeIframeApi(win: Window = window, doc: Document = document): Promise<YouTubeNamespace> {
  // The iframe API is a page global the DOM typings do not declare.
  const target = win as unknown as YouTubeWindow;
  if (target.YT?.Player) return Promise.resolve(target.YT as YouTubeNamespace);
  if (loading) return loading;
  loading = new Promise<YouTubeNamespace>((resolve, reject) => {
    const previous = target.onYouTubeIframeAPIReady;
    target.onYouTubeIframeAPIReady = () => {
      previous?.();
      resolve(target.YT as YouTubeNamespace);
    };
    const script = doc.createElement("script");
    script.src = IFRAME_API_URL;
    script.async = true;
    script.onerror = () => {
      loading = null;
      reject(new Error("The YouTube player could not be loaded."));
    };
    doc.head.appendChild(script);
  });
  return loading;
}

export interface YouTubeMount {
  player: YouTubePlayer;
  /** Destroys the adapter and the iframe it created. */
  dispose(): void;
}

/**
 * Creates the one iframe for `videoId` in `host` and resolves when it is
 * ready. Options match the source mount and never change after creation.
 */
export function mountYouTube(host: HTMLElement, videoId: string, api: YouTubeNamespace, timers?: Timers, signal?: AbortSignal): Promise<YouTubeMount> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) { reject(signal.reason); return; }
    let adapter: YouTubePlayer | null = null;
    let disposed = false;
    const dispose = () => {
      if (disposed) return;
      disposed = true;
      signal?.removeEventListener("abort", abort);
      adapter?.destroy();
      iframe.destroy();
    };
    const abort = () => { dispose(); reject(signal?.reason); };
    const iframe = new api.Player(host, {
      videoId,
      width: "100%",
      playerVars: { autoplay: 0, rel: 0, modestbranding: 1 },
      events: {
        onReady: (event) => {
          if (disposed) return;
          adapter = new YouTubePlayer(event.target, timers);
          const ready = adapter;
          resolve({
            player: ready,
            dispose,
          });
        },
        onStateChange: (event) => adapter?.handleStateChange(event.data),
      },
    });
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) abort();
  });
}
