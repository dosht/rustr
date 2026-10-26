// UPL-006 browser measurement, as in the source AudioDurationCalculator:
// HTML5 metadata first, then a Web Audio decode for files up to 100 MiB.
// Returns seconds or null; core rounds to minutes and decides what follows.

export const HTML5_TIMEOUT_MS = 5_000;
export const WEB_AUDIO_TIMEOUT_MS = 10_000;
export const MAX_WEB_AUDIO_BYTES = 100 * 1024 * 1024;

interface DecodingContext {
  decodeAudioData(data: ArrayBuffer): Promise<AudioBuffer>;
  close(): Promise<void>;
}

/** The browser objects measurement needs; injected so jsdom tests can drive them. */
export interface MeasureEnv {
  createAudio(): HTMLAudioElement;
  createObjectURL(file: Blob): string;
  revokeObjectURL(url: string): void;
  /** null when this browser has no Web Audio. */
  createContext(): DecodingContext | null;
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
}

export function browserMeasureEnv(): MeasureEnv {
  return {
    createAudio: () => new Audio(),
    createObjectURL: (file) => URL.createObjectURL(file),
    revokeObjectURL: (url) => URL.revokeObjectURL(url),
    createContext: () => {
      // Older Safari only exposes the prefixed constructor.
      const prefixed = "webkitAudioContext" in window ? (window.webkitAudioContext as typeof AudioContext) : undefined;
      const Context = window.AudioContext ?? prefixed;
      return Context ? new Context() : null;
    },
    setTimeout: (callback, ms) => window.setTimeout(callback, ms),
    clearTimeout: (id) => window.clearTimeout(id as number),
  };
}

function usable(seconds: number): boolean {
  return Number.isFinite(seconds) && seconds > 0;
}

function fromMetadata(file: File, env: MeasureEnv): Promise<number | null> {
  return new Promise((resolve) => {
    const audio = env.createAudio();
    const url = env.createObjectURL(file);
    let settled = false;
    const finish = (seconds: number | null) => {
      if (settled) return;
      settled = true;
      env.clearTimeout(timer);
      env.revokeObjectURL(url);
      audio.src = "";
      resolve(seconds);
    };
    const timer = env.setTimeout(() => finish(null), HTML5_TIMEOUT_MS);
    audio.preload = "metadata";
    audio.addEventListener("loadedmetadata", () => finish(usable(audio.duration) ? audio.duration : null));
    audio.addEventListener("error", () => finish(null));
    audio.src = url;
  });
}

async function fromDecode(file: File, env: MeasureEnv): Promise<number | null> {
  let data: ArrayBuffer;
  let context: DecodingContext | null;
  try {
    // As in the source, reading the file is not part of the decode timeout.
    data = await file.arrayBuffer();
    context = env.createContext();
  } catch {
    return null;
  }
  if (!context) return null;
  const decoding = context;
  return new Promise((resolve) => {
    let settled = false;
    const finish = (seconds: number | null) => {
      if (settled) return;
      settled = true;
      env.clearTimeout(timer);
      decoding.close().catch(() => {});
      resolve(seconds);
    };
    const timer = env.setTimeout(() => finish(null), WEB_AUDIO_TIMEOUT_MS);
    decoding.decodeAudioData(data).then(
      (buffer) => finish(usable(buffer.duration) ? buffer.duration : null),
      () => finish(null),
    );
  });
}

/** Seconds of audio in `file`, or null when neither method can tell. */
export async function measureFile(file: File, env: MeasureEnv = browserMeasureEnv()): Promise<number | null> {
  const metadata = await fromMetadata(file, env);
  if (metadata !== null) return metadata;
  if (file.size > MAX_WEB_AUDIO_BYTES) return null;
  return fromDecode(file, env);
}
