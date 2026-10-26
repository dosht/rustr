// The desktop waveform player, as the source AudioPlayer/WaveSurferMediaPlayer:
// wavesurfer.js over a media element, state only from its events. The waveform
// itself is decorative: seeded peaks per job, never decoded from the audio.

import WaveSurfer from "wavesurfer.js";
import { knownDuration, PlayerState, type MountedPlayer, type PlayerSnapshot } from "./player";

export const WAVEFORM_HEIGHT = 40;
/** The source never draws a waveform shorter than 20 (`Math.max(duration, 20)`). */
export const MIN_WAVEFORM_DURATION = 20;

/**
 * The source's decorative peaks (`generateWaveformPeaks`): a Park-Miller LCG
 * seeded with the job id, `length * samplesPerSecond` values in [-1, 1],
 * smoothed by a centered moving average. Same job, same picture.
 */
export function decorativePeaks(jobId: number, length: number, samplesPerSecond = 60, smoothingWindow = 10): number[] {
  let value = jobId;
  const raw: number[] = [];
  for (let i = 0; i < length * samplesPerSecond; i++) {
    value = (value * 16807) % 2147483647;
    raw.push(((value - 1) / 2147483646) * 2 - 1);
  }
  const half = Math.floor(smoothingWindow / 2);
  return raw.map((_, i) => {
    let sum = 0;
    let count = 0;
    for (let j = i - half; j <= i + half; j++) {
      if (j >= 0 && j < raw.length) {
        sum += raw[j];
        count++;
      }
    }
    return sum / count;
  });
}

/** The subset of a wavesurfer.js instance used here. */
export interface WaveSurferLike {
  on(event: "play" | "pause" | "finish" | "ready", listener: () => void): () => void;
  on(event: "timeupdate", listener: (time: number) => void): () => void;
  play(): Promise<void>;
  pause(): void;
  setTime(seconds: number): void;
  getDuration(): number;
  getMediaElement(): HTMLMediaElement;
  destroy(): void;
}

export interface WaveformOptions {
  container: HTMLElement;
  url: string;
  peaks: number[];
  duration: number;
  /** Resolved colors: canvas drawing cannot read CSS custom properties. */
  waveColor: string;
  progressColor: string;
}

export type CreateWaveSurfer = (options: {
  container: HTMLElement;
  url: string;
  height: number;
  waveColor: string;
  progressColor: string;
  dragToSeek: boolean;
  backend: "MediaElement";
  peaks: number[][];
  duration: number;
}) => WaveSurferLike;

export class WaveformPlayer implements MountedPlayer {
  readonly kind = "waveform" as const;
  readonly #state = new PlayerState();
  readonly #unbind: Array<() => void> = [];

  constructor(
    private readonly surfer: WaveSurferLike,
    onLoadError?: () => void,
  ) {
    this.#unbind.push(surfer.on("play", () => this.#state.set({ playing: true })));
    this.#unbind.push(surfer.on("pause", () => this.#state.set({ playing: false })));
    this.#unbind.push(surfer.on("finish", () => this.#state.set({ playing: false })));
    this.#unbind.push(surfer.on("timeupdate", (time) => this.#state.set({ time })));
    this.#unbind.push(surfer.on("ready", () => this.#state.set({ duration: knownDuration(surfer.getDuration()) })));
    const media = surfer.getMediaElement();
    if (onLoadError) {
      media.addEventListener("error", onLoadError);
      this.#unbind.push(() => media.removeEventListener("error", onLoadError));
    }
  }

  async play(): Promise<void> {
    try {
      await this.surfer.play();
    } catch {
      // A refused play leaves the state to the engine events.
    }
  }

  pause(): void {
    this.surfer.pause();
  }

  seek(seconds: number): void {
    this.surfer.setTime(seconds);
  }

  /** Browser playback rate, as in the source phone audio controls. */
  setPlaybackRate(rate: number): void {
    this.surfer.getMediaElement().playbackRate = rate;
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
    for (const unbind of this.#unbind.splice(0)) {
      try {
        unbind();
      } catch {
        // Teardown never throws.
      }
    }
    this.#state.clear();
  }
}

export interface WaveformMount {
  player: WaveformPlayer;
  /** Destroys the adapter, then the wavesurfer instance and its element. */
  dispose(): void;
}

/**
 * Creates the waveform for one streaming URL (source options: height 40,
 * drag to seek, MediaElement backend, peaks and a duration of at least 20).
 */
export function mountWaveform(
  options: WaveformOptions,
  onLoadError?: () => void,
  create?: CreateWaveSurfer,
): WaveformMount {
  const factory: CreateWaveSurfer = create ?? ((settings) => WaveSurfer.create(settings));
  const surfer = factory({
    container: options.container,
    url: options.url,
    height: WAVEFORM_HEIGHT,
    waveColor: options.waveColor,
    progressColor: options.progressColor,
    dragToSeek: true,
    backend: "MediaElement",
    peaks: [options.peaks.length > 0 ? options.peaks : [0.5]],
    duration: Math.max(options.duration, MIN_WAVEFORM_DURATION),
  });
  const player = new WaveformPlayer(surfer, onLoadError);
  return {
    player,
    dispose: () => {
      player.destroy();
      surfer.destroy();
    },
  };
}
