// p1Z's part of the p22 `Platform`: File handles, measurement, the signed-URL
// PUT and the one mounted player. p1V assembles the single Platform object
// with `{ ...createMediaPlatform(), ...createBrowserPlatform({ telemetry }) }`.

import type { FileHandle, Platform } from "$lib/core/platform";
import { FileStore } from "./files";
import { browserMeasureEnv, measureFile, type MeasureEnv } from "./measure";
import { PlayerSlot } from "./player";
import { putFile } from "./put";

export { FileStore, type HeldFile } from "./files";
export { activeLineIndex, clockTime, PlayerSlot, type MountedPlayer, type PlayerSnapshot } from "./player";
export { AudioElementPlayer } from "./audio-player";
export { decorativePeaks, mountWaveform, WaveformPlayer, type WaveformMount, type WaveformOptions } from "./waveform-player";
export { loadYouTubeIframeApi, mountYouTube, YouTubePlayer, type YouTubeMount } from "./youtube-player";

export interface MediaPlatform extends Pick<Platform, "measure" | "put" | "forget" | "player"> {
  /** Views hold picked/dropped Files here and dispatch the returned handles. */
  readonly files: FileStore;
  /** Views attach the one mounted player here. */
  readonly players: PlayerSlot;
}

export interface MediaPlatformDeps {
  measureEnv?: MeasureEnv;
  createRequest?: () => XMLHttpRequest;
}

export function createMediaPlatform(deps: MediaPlatformDeps = {}): MediaPlatform {
  const files = new FileStore();
  const players = new PlayerSlot();
  const measureEnv = deps.measureEnv ?? browserMeasureEnv();
  return {
    files,
    players,
    async measure(handle: FileHandle) {
      const file = files.get(handle);
      return file ? measureFile(file, measureEnv) : null;
    },
    async put(handle: FileHandle, url: string, onProgress: (percent: number) => void) {
      const file = files.get(handle);
      if (!file) return { ok: false, status: 0 };
      return putFile(file, url, onProgress, deps.createRequest);
    },
    forget(handle: FileHandle) {
      files.forget(handle);
    },
    player() {
      return players.current();
    },
  };
}
