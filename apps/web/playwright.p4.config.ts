import { defineConfig, devices } from "@playwright/test";

if (!process.env.TASKR_P4_WEB_URL) throw new Error("TASKR_P4_WEB_URL must name the actual wasm taskr web shell connected to the disposable host.");
export default defineConfig({
  testDir: "p4-e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 45_000,
  reporter: [["list"], ["html", { outputFolder: "p4-playwright-report", open: "never" }]],
  use: { baseURL: process.env.TASKR_P4_WEB_URL, trace: "retain-on-failure", screenshot: "only-on-failure" },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"] } },
    { name: "mobile", use: { ...devices["Pixel 7"] } },
  ],
});
