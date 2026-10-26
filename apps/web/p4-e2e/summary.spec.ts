import { expect, test, type Page } from "@playwright/test";
import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { isPhone, signIn, uniqueEmail } from "../e2e/app";

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required; use only the owned disposable P4 fixture host/database.`);
  return value;
}
const api = required("TASKR_P4_API").replace(/\/$/, "");
const control = required("TASKR_P4_PROVIDER_CONTROL").replace(/\/$/, "");
const database = required("TASKR_P4_FIXTURE_DATABASE_URL");
const fixture = fileURLToPath(new URL("../../../scripts/p4-fixtures/taskr-completed.sql", import.meta.url));
function accountFor(email: string): string {
  // Exact published taskr-billing SUBJECTS namespace + DevIdentity subject.
  const namespace = Buffer.from("3f0e5a9162c44d7b9e18a2c671b408d5", "hex");
  const digest = createHash("sha1").update(namespace).update(`dev-${email}`).digest().subarray(0, 16);
  digest[6] = (digest[6] & 0x0f) | 0x50;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = digest.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
async function prepare(page: Page) {
  const email = uniqueEmail("p4-wasm");
  await signIn(page, email);
  const headers = { Authorization: `Bearer dev:${email}` };
  const enrolled = await page.request.post(`${api}/audio-work/enroll`, { headers });
  expect(enrolled.ok(), await enrolled.text()).toBeTruthy();
  const work = randomUUID();
  execFileSync("psql", [database, "-v", "ON_ERROR_STOP=1", "-v", `account_id=${accountFor(email)}`, "-v", `work_id=${work}`, "-f", fixture], { stdio: "pipe" });
  const seeded = await page.request.get(`${api}/audio-work/${work}`, { headers });
  expect(seeded.ok(), "fixture SQL must create a completed work owned by this dev-auth account").toBeTruthy();
  expect((await seeded.json()).status).toBe("completed");
  await openWork(page, work);
  return { email, headers, work };
}
async function openWork(page: Page, work: string) {
  await page.getByRole("button", { name: "Audio summaries", exact: true }).click();
  if (isPhone(page)) await page.getByLabel("Audio work", { exact: true }).selectOption(work);
  else await page.getByRole("navigation", { name: "Audio work", exact: true }).getByRole("button", { name: /Planning meeting/ }).click();
}
async function start(page: Page) {
  await page.getByRole("checkbox", { name: "I agree to external AI processing for this summary" }).check();
  await page.getByRole("button", { name: "Generate summary" }).click();
  await expect(page.getByLabel("Summary text")).toHaveText("Hello");
  await expect(page.getByRole("status")).toHaveText("Receiving provisional summary…");
}
interface Run { id: string; finished: boolean; model: string }
async function runs(page: Page): Promise<Run[]> {
  const response = await page.request.get(`${control}/runs`);
  expect(response.ok()).toBeTruthy();
  const body: { runs: Run[] } = await response.json();
  return body.runs;
}
async function finish(page: Page, id: string) {
  expect((await page.request.post(`${control}/runs/${id}/finish`)).ok()).toBeTruthy();
}

test("real wasm progressive view, replacement, detach/reconnect and reload use one generation", async ({ page }, info) => {
  const before = await runs(page);
  const { work } = await prepare(page);
  await start(page);
  const generation = (await runs(page)).filter((run) => !before.some((old) => old.id === run.id));
  expect(generation).toHaveLength(1);
  expect(generation[0].finished).toBe(false);
  await page.screenshot({ path: info.outputPath("partial-before-upstream-final.png"), fullPage: true });
  await page.getByRole("button", { name: "Pause updates" }).click();
  await page.getByRole("button", { name: "Resume updates" }).click();
  await expect(page.getByLabel("Summary text")).toHaveText("Hello");
  await page.reload();
  await openWork(page, work);
  await expect(page.getByLabel("Summary text")).toHaveText("Hello");
  expect((await runs(page)).length).toBe(before.length + 1);
  await finish(page, generation[0].id);
  await expect(page.getByLabel("Summary text")).toHaveText("Hello world");
  await expect(page.getByRole("status")).toHaveText("Summary saved.");
  await page.screenshot({ path: info.outputPath("authoritative-saved-replacement.png"), fullPage: true });
  await page.reload();
  await openWork(page, work);
  await expect(page.getByLabel("Summary text")).toHaveText("Hello world");
  await expect(page.getByRole("status")).toHaveText("Summary saved.");
  expect((await runs(page)).length).toBe(before.length + 1);
});

test("cancellation cannot save a late response and session loss drops the old wasm surface", async ({ page }) => {
  const before = await runs(page);
  const { headers, work } = await prepare(page);
  await start(page);
  let dispatched = (await runs(page)).filter((run) => !before.some((old) => old.id === run.id));
  expect(dispatched).toHaveLength(1);
  await page.getByRole("button", { name: "Stop generation" }).click();
  await expect(page.getByRole("status")).toHaveText(/Stopped locally/);
  await finish(page, dispatched[0].id);
  const cancelled = await page.request.get(`${api}/audio-work/${work}/summary`, { headers });
  const cancelledBody = await cancelled.json();
  expect(cancelledBody.generation.status).toBe("cancelled");
  expect(cancelledBody.summary).toBeNull();
  await start(page); // explicit new user action, intentionally a second generation
  dispatched = (await runs(page)).filter((run) => !before.some((old) => old.id === run.id));
  expect(dispatched).toHaveLength(2);
  await page.getByRole("button", { name: "Sign out", exact: true }).click();
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
  await finish(page, dispatched[1].id);
  await expect(page.getByLabel("Summary text")).toHaveCount(0);
  await signIn(page, uniqueEmail("p4-other-owner"));
  await page.getByRole("button", { name: "Audio summaries", exact: true }).click();
  await expect(page.getByLabel("Summary text")).toHaveText("");
  expect((await runs(page)).length).toBe(before.length + 2);
});
