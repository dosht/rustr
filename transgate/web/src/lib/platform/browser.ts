// The page-only capabilities of p22's `Platform` that this shell owns: clipboard,
// save, share sheet and consent-gated analytics.
// Each reports what happened; core decides what it means. Media capabilities
// (files, measure, put, player) come from p1Z's createMediaPlatform().
import type { Platform, ShareOutcome } from "$lib/core/platform";
import type { Telemetry } from "./telemetry.svelte";

export type BrowserPlatform = Pick<Platform, "copy" | "save" | "share" | "track" | "resetTelemetry">;

interface Options {
  telemetry: Pick<Telemetry, "track" | "reset">;
  win?: Window & Pick<typeof globalThis, "ClipboardItem" | "URL">;
}

export function createBrowserPlatform({ telemetry, win = window }: Options): BrowserPlatform {
  const doc = win.document;
  const nav = win.navigator;

  async function copy(text: string, html: string | null): Promise<boolean> {
    try {
      if (html !== null && typeof win.ClipboardItem === "function" && nav.clipboard?.write) {
        await nav.clipboard.write([
          new win.ClipboardItem({
            "text/plain": new Blob([text], { type: "text/plain" }),
            "text/html": new Blob([html], { type: "text/html" }),
          }),
        ]);
        return true;
      }
      if (!nav.clipboard?.writeText) return false;
      await nav.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }

  function blob(bytes: Uint8Array, mime: string): Blob {
    return new Blob([bytes as Uint8Array<ArrayBuffer>], { type: mime });
  }

  function save(bytes: Uint8Array, filename: string, mime: string): void {
    const url = win.URL.createObjectURL(blob(bytes, mime));
    const link = doc.createElement("a");
    link.href = url;
    link.download = filename;
    link.rel = "noopener";
    link.hidden = true;
    doc.body.append(link);
    link.click();
    link.remove();
    // Some browsers start the download asynchronously; keep the URL alive briefly.
    win.setTimeout(() => win.URL.revokeObjectURL(url), 1000);
  }

  async function share(bytes: Uint8Array, filename: string, mime: string): Promise<ShareOutcome> {
    if (typeof nav.share !== "function" || typeof nav.canShare !== "function") return "unsupported";
    const data = { files: [new File([blob(bytes, mime)], filename, { type: mime })] };
    if (!nav.canShare(data)) return "unsupported";
    try {
      await nav.share(data);
      return "shared";
    } catch (error) {
      // The user closed the sheet: never fall through to a download.
      if (error instanceof DOMException && error.name === "AbortError") return "dismissed";
      return "unsupported";
    }
  }

  function track(event: string, properties: Record<string, string | number | boolean | null>): void {
    telemetry.track(event, properties);
  }

  function resetTelemetry(): void {
    telemetry.reset();
  }

  return { copy, save, share, track, resetTelemetry };
}
