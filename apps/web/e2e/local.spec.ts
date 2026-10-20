import { expect, test } from "@playwright/test";
import { LOCAL_URL } from "../playwright.config";
import { addTask, task } from "./app";

test.use({ baseURL: LOCAL_URL });

test("signed out, tasks live in this browser and survive a reload", async ({ page }) => {
  await page.goto("/");
  await addTask(page, "Buy milk");
  await expect(task(page, "Buy milk")).toBeVisible();
  await page.reload();
  await expect(task(page, "Buy milk")).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign in" })).toHaveCount(0);
});
