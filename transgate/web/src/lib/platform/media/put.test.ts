import { describe, expect, it } from "vitest";
import { putFile } from "./put";

class FakeRequest {
  status = 0;
  method = "";
  url = "";
  headers: Record<string, string> = {};
  body: unknown = null;
  upload: { onprogress: ((event: ProgressEvent) => void) | null } = { onprogress: null };
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;
  ontimeout: (() => void) | null = null;
  open(method: string, url: string) {
    this.method = method;
    this.url = url;
  }
  setRequestHeader(name: string, value: string) {
    this.headers[name] = value;
  }
  send(body: unknown) {
    this.body = body;
  }
  progress(loaded: number, total: number) {
    this.upload.onprogress?.({ loaded, total, lengthComputable: total > 0 } as ProgressEvent);
  }
  finish(status: number) {
    this.status = status;
    this.onload?.();
  }
}

function start(file: File, url = "https://storage.test/signed?x=1") {
  const request = new FakeRequest();
  const progress: number[] = [];
  const outcome = putFile(file, url, (p) => progress.push(p), () => request as unknown as XMLHttpRequest);
  return { request, progress, outcome };
}

describe("putFile", () => {
  it("PUTs the File object itself with its own type and reports whole-percent progress", async () => {
    const file = new File(["abc"], "talk.m4a", { type: "audio/mp4" });
    const { request, progress, outcome } = start(file);
    expect([request.method, request.url]).toEqual(["PUT", "https://storage.test/signed?x=1"]);
    expect(request.headers).toEqual({ "Content-Type": "audio/mp4" });
    expect(request.body).toBe(file);
    request.progress(1, 3);
    request.progress(0, 0);
    request.progress(3, 3);
    request.finish(200);
    await expect(outcome).resolves.toEqual({ ok: true, status: 200 });
    expect(progress).toEqual([33, 100]);
  });

  it("falls back to application/octet-stream for a file without a type", () => {
    const { request } = start(new File(["x"], "noext"));
    expect(request.headers["Content-Type"]).toBe("application/octet-stream");
  });

  it("counts only HTTP 200 as uploaded", async () => {
    for (const status of [201, 204, 403, 500]) {
      const { request, outcome } = start(new File(["x"], "a.mp3", { type: "audio/mpeg" }));
      request.finish(status);
      await expect(outcome).resolves.toEqual({ ok: false, status });
    }
  });

  it("reports network failures, aborts and timeouts as not uploaded, without retrying", async () => {
    for (const event of ["onerror", "onabort", "ontimeout"] as const) {
      const { request, outcome } = start(new File(["x"], "a.mp3"));
      request[event]?.();
      await expect(outcome).resolves.toEqual({ ok: false, status: 0 });
    }
  });

  it("does not send when the URL cannot be opened", async () => {
    const request = new FakeRequest();
    request.open = () => {
      throw new SyntaxError("invalid URL");
    };
    const outcome = putFile(new File(["x"], "a.mp3"), "::", () => {}, () => request as unknown as XMLHttpRequest);
    await expect(outcome).resolves.toEqual({ ok: false, status: 0 });
    expect(request.body).toBeNull();
  });
});
