// An HTMLAudioElement player, as the source ClassicAudioMediaPlayer: state
// follows real media events only.

import { knownDuration, PlayerState, type MountedPlayer, type PlayerSnapshot } from "./player";

export class AudioElementPlayer implements MountedPlayer {
  readonly kind = "audio" as const;
  readonly #state: PlayerState;
  readonly #unbind: Array<() => void> = [];

  /**
   * `onLoadError` lets the view report a failed source load, e.g. an expired
   * signed streaming URL, as an intent; this adapter never refreshes it.
   */
  constructor(
    private readonly element: HTMLAudioElement,
    onLoadError?: () => void,
  ) {
    this.#state = new PlayerState({
      playing: !element.paused && !element.ended,
      time: element.currentTime,
      duration: knownDuration(element.duration),
    });
    this.#on("play", () => this.#state.set({ playing: true }));
    this.#on("pause", () => this.#state.set({ playing: false }));
    this.#on("ended", () => this.#state.set({ playing: false }));
    this.#on("timeupdate", () => this.#state.set({ time: element.currentTime }));
    const duration = () => {
      if (Number.isFinite(element.duration)) this.#state.set({ duration: knownDuration(element.duration) });
    };
    this.#on("loadedmetadata", duration);
    this.#on("durationchange", duration);
    if (onLoadError) this.#on("error", onLoadError);
  }

  #on(event: keyof HTMLMediaElementEventMap, listener: () => void): void {
    this.element.addEventListener(event, listener);
    this.#unbind.push(() => this.element.removeEventListener(event, listener));
  }

  async play(): Promise<void> {
    try {
      await this.element.play();
    } catch {
      // As in the source: a refused play (autoplay policy, no source) leaves
      // the state to the media events, which never report playing.
    }
  }

  pause(): void {
    this.element.pause();
  }

  seek(seconds: number): void {
    this.element.currentTime = seconds;
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

  destroy(): void {
    for (const unbind of this.#unbind.splice(0)) unbind();
    this.#state.clear();
  }
}
