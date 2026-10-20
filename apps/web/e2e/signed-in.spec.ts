import { expect, test } from "@playwright/test";
import {
  addTask,
  isPhone,
  openSection,
  signIn,
  signOut,
  task,
  uniqueEmail,
  workspaceNames,
  workspaceOf,
} from "./app";

test("an owner adds tasks and sees the core's rules", async ({ page }) => {
  await signIn(page, uniqueEmail("ada"));
  await addTask(page, "Ship M1");
  await expect(page.getByText('Added "Ship M1"')).toBeVisible();
  await expect(task(page, "Ship M1")).toBeVisible();

  await addTask(page, "ship m1");
  await expect(page.getByRole("alert")).toHaveText(/There is already an open task with that title./);

  if (isPhone(page)) await task(page, "Ship M1").click();
  else await page.getByLabel('Mark "Ship M1" done').check();
  await expect(page.getByText('Done: "Ship M1"')).toBeVisible();
  await expect(page.getByRole("tab", { name: "Done (1)" })).toBeVisible();
});

test("a viewer sees the shared list read-only and still has a workspace of their own", async ({ browser, page }) => {
  const ada = uniqueEmail("ada");
  const bob = uniqueEmail("bob");
  await signIn(page, ada);
  await addTask(page, "Shared plan");
  await expect(task(page, "Shared plan")).toBeVisible();
  await openSection(page, "Members");
  await page.getByLabel("Member email").fill(bob);
  await page.getByLabel("Role", { exact: true }).selectOption("viewer");
  await page.getByRole("button", { name: /^Add/ }).click();
  await expect(page.getByText(`${bob} is now Viewer`)).toBeVisible();

  const bobPage = await (await browser.newContext({ viewport: page.viewportSize() })).newPage();
  await signIn(bobPage, bob, workspaceOf(ada));
  await expect(task(bobPage, "Shared plan")).toBeVisible();
  await expect(bobPage.getByText(/You can view this list/)).toBeVisible();
  await expect(bobPage.getByLabel("New task title")).toHaveCount(0);
  expect(await workspaceNames(bobPage)).toEqual([workspaceOf(ada), workspaceOf(bob)]);
});

test("the session survives a reload and sign-out ends it", async ({ page }) => {
  await signIn(page, uniqueEmail("cy"));
  await page.reload();
  await expect(page.getByRole("navigation", { name: "Sections" })).toBeVisible();
  await signOut(page);
  await page.reload();
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
});

test("phones get the mobile views and desktops the desktop views", async ({ page }) => {
  await signIn(page, uniqueEmail("dee"));
  if (isPhone(page)) {
    await expect(page.getByRole("button", { name: "Open workspaces" })).toBeVisible();
    await expect(page.getByRole("table")).toHaveCount(0);
  } else {
    await expect(page.getByRole("navigation", { name: "Workspaces" })).toBeVisible();
    await expect(page.getByRole("table")).toHaveCount(0);
    await addTask(page, "a row");
    await expect(page.getByRole("table")).toBeVisible();
  }
});
