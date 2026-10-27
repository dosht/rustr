// UPL-003/004 (transgate/docs/rules.md): which dropped or picked files reach the
// upload queue, and the 10-file cap. The decisions are core's
// (presentation::upload_queue accept_dropped, phone_accepts, MAX_FILES); this
// spec proves the built shell forwards real browser drops (desktop) and picks
// (phone, which has no drop zone) to them.
import { expect, test, type Page } from "@playwright/test";
import { shell } from "./support/shell";

interface Picked { name: string; mimeType: string }

async function home(page: Page): Promise<void> {
  await shell(page.context(), {
    signedIn: true,
    consent: "rejected",
    handlers: [(request, url, route) => (url.pathname === "/api/jobs/drafts" && request.method() === "GET" ? route.fulfill({ json: [] }) : undefined)],
  });
  await page.goto("/home");
  await expect(page.locator('[data-testid="upload-files"]')).toBeAttached();
}

/** Desktop: a real drag-and-drop on the drop zone. Phone: the mounted file picker. */
async function add(page: Page, isMobile: boolean, files: Picked[]): Promise<void> {
  if (isMobile) {
    await page.locator('[data-testid="upload-files"]').setInputFiles(files.map((file) => ({ ...file, buffer: Buffer.alloc(64) })));
    return;
  }
  await page.getByRole("region", { name: "Upload files" }).evaluate((zone, list) => {
    const data = new DataTransfer();
    for (const file of list) data.items.add(new File([new Uint8Array(64)], file.name, { type: file.mimeType }));
    zone.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: data }));
    zone.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: data }));
  }, files);
}

/** One `UploadFileRow` per queued file: a group named by the file. */
const queued = (page: Page) => page.locator("main li > [role=group]");
const row = (page: Page, name: string) => page.getByRole("group", { name, exact: true });
const audio = (count: number, prefix: string): Picked[] =>
  Array.from({ length: count }, (_, i) => ({ name: `${prefix}-${String(i + 1).padStart(2, "0")}.wav`, mimeType: "audio/wav" }));

test.describe("upload queue", () => {
  test("dropped or picked files reach the queue only when core accepts them", async ({ page, isMobile }) => {
    await home(page);
    await add(page, isMobile, [
      { name: "meeting.wav", mimeType: "audio/wav" },
      { name: "clip.mov", mimeType: "video/quicktime" },
      { name: "voice.m4a", mimeType: "" },
      { name: "song.MP3", mimeType: "" },
      { name: "notes.txt", mimeType: "text/plain" },
    ]);
    for (const name of ["meeting.wav", "clip.mov", "voice.m4a"]) await expect(row(page, name)).toHaveCount(1);
    await expect(row(page, "notes.txt")).toHaveCount(0);
    // A drop needs audio/video MIME or a lower-case .m4a name; the phone picker
    // accepts a case-insensitive audio extension without a MIME type.
    await expect(row(page, "song.MP3")).toHaveCount(isMobile ? 1 : 0);
    await expect(queued(page)).toHaveCount(isMobile ? 4 : 3);
  });

  test("at ten queued files the add buttons disappear and further files are refused as core decides", async ({ page, isMobile }) => {
    await home(page);
    // The add control (desktop menu trigger, phone button); the hidden file input shares the name.
    const button = page.locator("button").and(page.getByRole("button", { name: "Upload files", exact: true }));
    await expect(button).toBeVisible();
    if (isMobile) {
      // A multi-file pick is not capped; the add buttons disappear once ten are queued.
      await add(page, isMobile, audio(11, "phone"));
      await expect(queued(page)).toHaveCount(11);
    } else {
      await add(page, isMobile, audio(9, "first"));
      await expect(queued(page)).toHaveCount(9);
      await expect(button).toBeVisible();
      // Below the cap a drop is not trimmed: drops are refused only once ten are queued.
      await add(page, isMobile, audio(2, "second"));
      await expect(queued(page)).toHaveCount(11);
      await add(page, isMobile, audio(1, "refused"));
      await page.waitForTimeout(500);
      await expect(row(page, "refused-01.wav")).toHaveCount(0);
      await expect(queued(page)).toHaveCount(11);
    }
    await expect(button).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Add from YouTube", exact: true })).toHaveCount(0);
  });
});
