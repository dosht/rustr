// Helpers that hide the layout difference: on a phone, workspaces are in a
// drawer and sections in a bottom tab bar; on a desktop both are in the sidebar.
import { expect, type Page } from "@playwright/test";

export const isPhone = (page: Page) => (page.viewportSize()?.width ?? 1280) <= 720;

export function uniqueEmail(name: string): string {
  return `${name}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}@example.com`;
}

export const workspaceOf = (email: string) => `${email.split("@")[0]}'s workspace`;

/** Signs in and waits for the first workspace (by name) to open. */
export async function signIn(page: Page, email: string, opens = workspaceOf(email)) {
  await page.goto("/");
  await page.getByLabel("Email").fill(email);
  await page.getByRole("button", { name: "Sign in" }).click();
  if (isPhone(page)) {
    await expect(page.getByText(`${opens} · Tasks`)).toBeVisible();
  } else {
    await expect(page.getByRole("heading", { name: opens })).toBeVisible();
  }
}

export async function workspaceNames(page: Page): Promise<string[]> {
  if (isPhone(page)) await page.getByRole("button", { name: "Open workspaces" }).click();
  const scope = isPhone(page)
    ? page.getByRole("complementary")
    : page.getByRole("navigation", { name: "Workspaces" });
  const names = await scope.locator(".name").allTextContents();
  if (isPhone(page)) await page.getByRole("button", { name: "Close workspaces" }).click();
  return names;
}

export async function openSection(page: Page, name: "Tasks" | "Members") {
  const nav = page.getByRole("navigation", { name: "Sections" });
  await nav.getByRole("button", { name }).click();
}

export async function openWorkspace(page: Page, name: string) {
  if (isPhone(page)) {
    await page.getByRole("button", { name: "Open workspaces" }).click();
    await page.getByRole("complementary").getByRole("button", { name: new RegExp(name) }).click();
  } else {
    await page.getByRole("navigation", { name: "Workspaces" }).getByRole("button", { name: new RegExp(name) }).click();
  }
}

export async function signOut(page: Page) {
  if (isPhone(page)) await page.getByRole("button", { name: "Open workspaces" }).click();
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
}

export async function addTask(page: Page, title: string) {
  await page.getByLabel("New task title").fill(title);
  await page.getByRole("button", { name: "Add", exact: true }).click();
}

export function task(page: Page, title: string) {
  return isPhone(page)
    ? page.getByRole("button", { name: new RegExp(`^${title}, (open|done)$`) })
    : page.getByRole("cell", { name: title, exact: true });
}
