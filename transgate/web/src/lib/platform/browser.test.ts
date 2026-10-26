import { afterEach, describe, expect, it, vi } from "vitest";
import { createBrowserPlatform } from "./browser";

const telemetry = { track: vi.fn(), reset: vi.fn() };

function platform() {
  return createBrowserPlatform({ telemetry });
}

function stub<K extends keyof Navigator>(key: K, value: unknown) {
  Object.defineProperty(navigator, key, { configurable: true, value });
}

afterEach(() => {
  document.body.replaceChildren();
  for (const key of ["clipboard", "share", "canShare"] as const) stub(key, undefined);
  vi.restoreAllMocks();
});

describe("copy", () => {
  it("writes plain text and reports success", async () => {
    const writeText = vi.fn(async () => {});
    stub("clipboard", { writeText });
    await expect(platform().copy("hello", null)).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("reports a refused or missing clipboard as false, never throws", async () => {
    stub("clipboard", { writeText: vi.fn(async () => Promise.reject(new DOMException("no", "NotAllowedError"))) });
    await expect(platform().copy("x", null)).resolves.toBe(false);
    stub("clipboard", undefined);
    await expect(platform().copy("x", null)).resolves.toBe(false);
  });
});

describe("share", () => {
  const bytes = new TextEncoder().encode("transcript");

  it("is unsupported without a file-capable share sheet", async () => {
    await expect(platform().share(bytes, "a.txt", "text/plain")).resolves.toBe("unsupported");
    stub("share", vi.fn());
    stub("canShare", () => false);
    await expect(platform().share(bytes, "a.txt", "text/plain")).resolves.toBe("unsupported");
  });

  it("reports a closed sheet as dismissed, not as a failure to fall back from", async () => {
    stub("canShare", () => true);
    stub("share", vi.fn(async () => Promise.reject(new DOMException("closed", "AbortError"))));
    await expect(platform().share(bytes, "a.txt", "text/plain")).resolves.toBe("dismissed");
  });

  it("shares the bytes as one named file", async () => {
    const share = vi.fn(async () => {});
    stub("canShare", () => true);
    stub("share", share);
    await expect(platform().share(bytes, "talk.srt", "application/x-subrip")).resolves.toBe("shared");
    const [{ files }] = share.mock.calls[0] as unknown as [{ files: File[] }];
    expect(files.map((f) => [f.name, f.type, f.size])).toEqual([["talk.srt", "application/x-subrip", bytes.length]]);
  });
});

describe("save", () => {
  it("downloads through a temporary link with the core-chosen name", () => {
    vi.useFakeTimers();
    const created = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:x");
    const revoked = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const clicked: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push(this.download);
    });
    platform().save(new Uint8Array([1, 2]), "invoice-TRAN00000007.pdf", "application/pdf");
    expect(created).toHaveBeenCalledOnce();
    expect(clicked).toEqual(["invoice-TRAN00000007.pdf"]);
    expect(document.querySelector("a")).toBeNull();
    vi.runAllTimers();
    expect(revoked).toHaveBeenCalledWith("blob:x");
    vi.useRealTimers();
  });
});
