import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import analyses from "../../core/src/documents/fixtures/analysis.json" with { type: "json" };
import { SESSION, USER, shell } from "./support/shell";

const job = {
  id: 3, account: USER, revision: 1, status: "completed", name: "analysis.wav",
  source: { kind: "upload", storage: "gcs", object: null }, language: null,
  translate_to: [], speakers: false, fix_grammer: null, duration_minutes: 12,
  size_bytes: 4200000, provider: null, model: null, callback_url: null,
  whisper_params: null, error: null, retry_count: 0, recovery_attempts: 0,
  soniox_transcription_id: null, started_at: null, created_at: 1790000000000000,
  updated_at: 1790000000000000, current_attempt: null, attempts_made: 0,
  deferred_charge: "none", next_dispatch_at: null,
};

const lines = [{
  id: 1, job: 3, account: USER, start_time: "0", end_time: "120",
  line: "A completed transcript for the saved analysis. ".repeat(40),
  speaker: null, corrected_text: null, split_on_speaker_change: true,
  language: null, translation_status: null, created_at: 0,
}];

for (const name of ["meeting", "support"]) {
  test(`${name} analysis downloads a PDF with its saved content and bundled fonts`, async ({ page }, testInfo) => {
    const progress = analyses.find((entry) => entry.name === name)!.progress;
    const fontRequests = new Set<string>();
    await shell(page.context(), {
      signedIn: true,
      consent: "rejected",
      handlers: [(request, url, route) => {
        if (/\/NotoSans[^/]*\.(?:ttf|otf)$/.test(url.pathname)) fontRequests.add(url.pathname);
        if (request.method() !== "GET") return undefined;
        if (url.pathname === "/api/session") {
          return route.fulfill({ json: { ...SESSION, capabilities: { ...SESSION.capabilities, ai: true } } });
        }
        if (url.pathname === "/api/billing/overview") return route.fulfill({ json: { balance: 1200, is_freemium: false, special_client_alert: null } });
        if (url.pathname === "/api/jobs/3") return route.fulfill({ json: { job, lines } });
        if (url.pathname === "/api/jobs") return route.fulfill({ json: { jobs: [job], next_cursor: null } });
        if (url.pathname === "/api/jobs/drafts") return route.fulfill({ json: [] });
        if (url.pathname === "/api/ai/consent") {
          return route.fulfill({ json: {
            hasConsented: true, consentVersion: "1", consentDate: "2026-01-01T00:00:00Z", revokedDate: null,
            privacyPolicyUrl: "https://example.test/privacy", providerName: "Test provider", canRevoke: true, cleanupPending: false,
          } });
        }
        if (url.pathname === "/api/ai/jobs/3/analysis") {
          return route.fulfill({ json: { analysis: {
            id: "00000000-0000-4000-8000-000000000003", job: 3, kind: "analysis", status: "completed",
            sequence: 7, source_revision: 1, consent_epoch: 1, draft: { kind: "empty" },
            final_content: { kind: "analysis", result: {
              detected: progress.detected, summary: progress.summary, analysis: progress.analysis, warnings: progress.warnings,
            } },
            durable: true, remote_outcome_unknown: false, execution: null, tokens_used: 100,
            interruption: null, created_at: 1790000000000000, finished_at: 1790000001000000,
          } } });
        }
        return undefined;
      }],
    });

    await page.goto("/home/3");
    await page.getByRole("tab", { name: "AI Analysis", exact: true }).click();
    await expect(page.getByText(progress.summary, { exact: true })).toBeVisible();
    expect(fontRequests.size).toBe(0);

    await page.getByRole("button", { name: "Export", exact: true }).click();
    const downloadPromise = page.waitForEvent("download");
    await page.getByRole("button", { name: /^Export as PDF/ }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toMatch(/\.pdf$/);
    const path = testInfo.outputPath(`${name}.pdf`);
    await download.saveAs(path);
    expect(readFileSync(path).subarray(0, 5).toString()).toBe("%PDF-");
    const text = execFileSync("pdftotext", ["-layout", path, "-"], { encoding: "utf8" }).replace(/\s+/g, " ");
    expect(text).toContain(progress.summary);
    const expected = name === "meeting"
      ? ["Ship the reviewed release", "Ana & Bob", "Send the review"]
      : ["Cannot export", "frustrated", "relieved", "\u2192", "Check the next export"];
    for (const content of expected) expect(text).toContain(content);
    for (const font of ["NotoSans", "NotoSans-Italic", "NotoSansArabic", "NotoSansSymbols2-Regular", "NotoSansSymbols"]) {
      expect([...fontRequests].some((url) => new RegExp("/" + font + "-[^/]+\\.ttf$").test(url)), `${font} is fetched from the built app`).toBe(true);
    }
    await expect(page.getByText("Failed to export as PDF", { exact: true })).toHaveCount(0);
    await testInfo.attach(`${name}-analysis`, { path, contentType: "application/pdf" });
  });
}
