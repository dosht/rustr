// The browser capabilities the Rust presenter cannot own: objects that only
// exist in the page (File handles, media elements, clipboard, save/share).
// Rust (transgate-wasm) calls these from its effect driver and turns every
// outcome into a core event; nothing here decides a business rule. Navigation,
// storage, randomness, cross-tab locks, HTTP and the checkout form POST are done
// inside Rust and are deliberately absent. Theme and device class are
// shell-local UI mechanics and also absent.

/** Opaque id the shell assigned to a picked/dropped File. Bytes never cross into Rust. */
export type FileHandle = number;

export type PlayerSource =
  | { kind: "audio"; url: string }
  | { kind: "youtube"; videoId: string };

/** The one mounted player (D9: one layout, one player). */
export interface PlayerPort {
  play(): Promise<void>;
  pause(): void;
  /** Seconds from the start. */
  seek(seconds: number): void;
  isPlaying(): boolean;
  currentTime(): number;
  duration(): number | null;
}

/** Outcome of a share sheet; "dismissed" must not fall through to another action. */
export type ShareOutcome = "shared" | "dismissed" | "unsupported";

export interface Platform {
  /**
   * Duration in seconds of a held File, or null when it cannot be measured.
   * HTML5 metadata with a 5 s timeout (finite > 0), else Web Audio decode only
   * when size <= 100 MiB with a 10 s timeout, else null. Core rounds.
   */
  measure(handle: FileHandle): Promise<number | null>;
  /**
   * One XHR PUT of the File itself (not a copied buffer) to a signed URL.
   * Content-Type = file.type || "application/octet-stream". Reports 0..100.
   * `ok` only for HTTP 200. Exactly one attempt; never retried here.
   */
  put(handle: FileHandle, url: string, onProgress: (percent: number) => void): Promise<{ ok: boolean; status: number }>;
  /** Releases a File the presenter no longer references (dispose/session loss). */
  forget(handle: FileHandle): void;
  /** The mounted player, or null when none is attached. The view attaches it. */
  player(): PlayerPort | null;
  /** Clipboard; html is optional rich content alongside the plain text. */
  copy(text: string, html: string | null): Promise<boolean>;
  /** Save bytes as a download with the core-chosen filename. */
  save(bytes: Uint8Array, filename: string, mime: string): void;
  /**
   * Present PDF bytes in the invoice modal; never download or open a window.
   * Blob/object-URL errors throw like save. The shell revokes the URL on
   * replacement, close, route exit, session loss and root disposal.
   * Rust fences the session, invoice context and latest still-open preview
   * request immediately before calling this method.
   */
  preview(bytes: Uint8Array, filename: string, mime: string): void;
  /** Phone share sheet for exported bytes. */
  share(bytes: Uint8Array, filename: string, mime: string): Promise<ShareOutcome>;
  /**
   * Browser analytics event (D12). The implementation delivers only after the
   * visitor accepted cookies and the optional PostHog client was initialized;
   * before consent or after refusal it drops the event. Server transactional
   * analytics is separate and unaffected.
   */
  track(event: string, properties: Record<string, string | number | boolean | null>): void;
  /**
   * PostHog `reset()` after sign-out (IDN-024): only if the optional client was
   * started; never loads the SDK.
   */
  resetTelemetry(): void;
}
