import { expect, test } from "@playwright/test";
import { EMAIL, SESSION, USER, shell } from "./support/shell";

function wav(): Buffer {
  const bytes = Buffer.alloc(32044);
  bytes.write("RIFF"); bytes.writeUInt32LE(32036, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(16000, 24); bytes.writeUInt32LE(32000, 28);
  bytes.writeUInt16LE(2, 32); bytes.writeUInt16LE(16, 34); bytes.write("data", 36);
  bytes.writeUInt32LE(32000, 40);
  return bytes;
}

for (const boundary of ["profile", "drafts"] as const) {
  test(`first callback file selection survives pending ${boundary}`, async ({ context, page }) => {
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const creates: unknown[] = [];
    let puts = 0;
    let exchanged = false;
    await context.addInitScript(() => localStorage.setItem("transgate.session.code-verifier", "test-verifier"));
    await shell(context, {
      consent: "rejected",
      external: (request, url, route) => {
        if (url.pathname !== "/auth/v1/token") return undefined;
        expect(url.searchParams.get("grant_type")).toBe("pkce");
        expect(request.postDataJSON().auth_code).toBe("first-signup");
        exchanged = true;
        return route.fulfill({ json: { access_token: "new-access", refresh_token: "new-refresh", expires_in: 3600, user: { id: USER, email: EMAIL } } });
      },
      handlers: [(request, url, route) => {
        if (url.pathname === "/api/session") return (async () => {
          if (boundary === "profile") await pending;
          await route.fulfill({ json: SESSION });
        })();
        if (url.pathname === "/api/jobs/drafts") return (async () => {
          if (boundary === "drafts") await pending;
          await route.fulfill({ json: [] });
        })();
        if (url.pathname === "/api/jobs" && request.method() === "POST") {
          creates.push(request.postDataJSON());
          return route.fulfill({ status: 201, json: {
            id: 1, account: USER, revision: 1, status: "draft", name: "first.wav",
            source: { kind: "upload", storage: "gcs", object: null }, language: null,
            translate_to: [], speakers: false, fix_grammer: null, duration_minutes: 1,
            size_bytes: 32044, provider: null, model: null, callback_url: null,
            whisper_params: null, error: null, retry_count: 0, recovery_attempts: 0,
            soniox_transcription_id: null, started_at: null, created_at: 0, updated_at: 0,
            current_attempt: null, attempts_made: 0, deferred_charge: "none", next_dispatch_at: null,
          } });
        }
        if (url.pathname === "/api/init-upload") return route.fulfill({ status: 201, json: `${url.origin}/upload/first` });
        if (url.pathname === "/upload/first") { puts++; return route.fulfill({ status: 200 }); }
        return undefined;
      }],
    });
    try {
      await page.goto("/home?code=first-signup");
      await page.waitForURL("**/home**");
      await page.getByTestId("upload-files").first().setInputFiles({ name: "first.wav", mimeType: "audio/wav", buffer: wav() });
      expect(exchanged).toBe(true);
      await expect.poll(() => creates.length).toBe(1);
      await expect.poll(() => puts).toBe(1);
      release();
      await expect(page.getByRole("group", { name: "first.wav", exact: true })).toContainText("Ready");
      expect(creates).toHaveLength(1);
    } finally { release(); }
  });
}
