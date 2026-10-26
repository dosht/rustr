// The web shell's only door into the Rust presenter (transgate-wasm). Containers
// depend on `TransgateCore`, so stories and component tests can pass generated
// fixtures instead of wasm. All business state lives in Rust; this file only
// loads the module and forwards calls.
import init, { TransgatePresenter } from "./pkg/transgate_wasm.js";
import type { Platform } from "./platform";
import type { AppIntent } from "./types/AppIntent";
import type { AppVm } from "./types/AppVm";

export type { Platform, PlayerPort, PlayerSource, ShareOutcome, FileHandle } from "./platform";
export type { AppIntent } from "./types/AppIntent";
export type { AppVm } from "./types/AppVm";

/** The public subset of the host-injected `tg-config` (web-shell.md section 7) the presenter needs. */
export interface TransgateConfig {
  /** API root; relative values such as "/api" resolve against the page URL. */
  apiUrl: string;
  /** GoTrue root ("/auth/v1" or absolute); relative values resolve against the page URL. */
  authUrl: string;
  /** Public anon key when the deployment requires it; never a service key. */
  authKey?: string | null;
  /** Origin auth redirects return to (the source's NEXT_PUBLIC_SITE_URL). */
  siteUrl: string;
  /** True when tg-config.captcha is configured: sign-in forms require a Turnstile token (D12). */
  captcha: boolean;
  /** Same-origin URLs of the bundled licensed PDF assets (fetched lazily, PDF only). */
  documentAssets: DocumentAssetUrls;
}

/** From the web package's `?url` imports of `transgate/export/{fonts,assets}`. */
export interface DocumentAssetUrls {
  base: { latin: string; latinItalic: string; arabic: string; symbols: string; invoiceLogo: string };
  /** Keyed by FallbackFont wire value: hebrew, armenian, georgian, devanagari, kannada, tamil, thai, cjk. */
  fallbacks: Record<"hebrew" | "armenian" | "georgian" | "devanagari" | "kannada" | "tamil" | "thai" | "cjk", string>;
}

export interface TransgateCore {
  /**
   * Applies a user intent in Rust, starts the effects it asks for and returns
   * the resulting view. Views produced later (requests, polls, AI watch, other
   * tabs) go to subscribers.
   */
  dispatch(intent: AppIntent): AppVm;
  /** Every new view, called outside the presenter's state; returns unsubscribe. */
  subscribe(listener: (view: AppVm) => void): () => void;
  current(): AppVm;
  /** Stops watches, polls and timers, forgets file handles and drops subscribers. */
  dispose(): void;
}

let ready: Promise<unknown> | undefined;

export async function loadTransgate(config: TransgateConfig, platform: Platform): Promise<TransgateCore> {
  ready ??= init();
  await ready;
  const presenter = new TransgatePresenter(config, platform);
  const listeners = new Set<(view: AppVm) => void>();
  presenter.listen((view: AppVm) => {
    for (const listener of listeners) listener(view);
  });
  // Restores the stored session and reads the current location; may start effects.
  presenter.start();
  return {
    dispatch: (intent) => presenter.dispatch(intent) as AppVm,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    current: () => presenter.current() as AppVm,
    dispose() {
      listeners.clear();
      presenter.dispose();
    },
  };
}
