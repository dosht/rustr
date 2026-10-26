// D9: one mounted player. Its state comes only from engine events, never from
// UI toggles, so every consumer (header controls, phone sticky player, the
// active-line highlight) reads the same truth.

import type { PlayerPort } from "$lib/core/platform";

export interface PlayerSnapshot {
  playing: boolean;
  /** Seconds from the start. */
  time: number;
  /** Seconds, or null until the engine reports it. */
  duration: number | null;
}

/** A player the view mounts. The presenter only ever sees it as a `PlayerPort`. */
export interface MountedPlayer extends PlayerPort {
  readonly kind: "audio" | "waveform" | "youtube";
  snapshot(): PlayerSnapshot;
  subscribe(listener: (snapshot: PlayerSnapshot) => void): () => void;
  /** Detaches engine listeners and timers. The owner of the engine element tears it down. */
  destroy(): void;
}

export class PlayerState {
  #snapshot: PlayerSnapshot;
  readonly #listeners = new Set<(snapshot: PlayerSnapshot) => void>();

  constructor(initial: PlayerSnapshot = { playing: false, time: 0, duration: null }) {
    this.#snapshot = initial;
  }

  get(): PlayerSnapshot {
    return this.#snapshot;
  }

  set(patch: Partial<PlayerSnapshot>): void {
    const next = { ...this.#snapshot, ...patch };
    const now = this.#snapshot;
    if (next.playing === now.playing && next.time === now.time && next.duration === now.duration) return;
    this.#snapshot = next;
    for (const listener of [...this.#listeners]) listener(next);
  }

  subscribe(listener: (snapshot: PlayerSnapshot) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  clear(): void {
    this.#listeners.clear();
  }
}

/** Seconds when finite and positive, else null. */
export function knownDuration(seconds: number): number | null {
  return Number.isFinite(seconds) && seconds > 0 ? seconds : null;
}

/** Holds the single mounted player for `Platform.player()`. */
export class PlayerSlot {
  #current: MountedPlayer | null = null;

  /**
   * Makes `player` the mounted one, destroying any previous one (never two
   * players, which produced dual audio in the source). The returned detach
   * destroys it only if it is still the mounted one.
   */
  attach(player: MountedPlayer): () => void {
    if (this.#current && this.#current !== player) this.#current.destroy();
    this.#current = player;
    return () => {
      if (this.#current !== player) return;
      player.destroy();
      this.#current = null;
    };
  }

  current(): MountedPlayer | null {
    return this.#current;
  }
}

/**
 * The highlighted line, as in the source `useActiveLineIndex`: the first line
 * whose end is at or after the current time; -1 before playback starts or
 * after the last line.
 */
export function activeLineIndex(endSeconds: readonly number[], time: number): number {
  if (time <= 0) return -1;
  for (let index = 0; index < endSeconds.length; index++) {
    if (time <= endSeconds[index]) return index;
  }
  return -1;
}

/** The player's `HH:MM:SS` readout, whole seconds rounded down (source AudioPlayer). */
export function clockTime(seconds: number): string {
  const whole = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const pad = (value: number) => value.toString().padStart(2, "0");
  return `${pad(Math.floor(whole / 3600))}:${pad(Math.floor((whole % 3600) / 60))}:${pad(whole % 60)}`;
}
