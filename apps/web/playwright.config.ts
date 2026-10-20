import { defineConfig, devices } from "@playwright/test";

// End-to-end runs against real processes: a throwaway taskr-server (memory
// storage, dev sign-in) and Vite serving the web app twice, signed in and
// local. Every spec runs on a desktop browser and on a phone, so both sets of
// views are covered.
const API_PORT = 18790;
const SIGNED_IN_PORT = 5199;
export const LOCAL_URL = "http://127.0.0.1:5198";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: `http://127.0.0.1:${SIGNED_IN_PORT}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"] } },
    { name: "mobile", use: { ...devices["Pixel 7"] } },
  ],
  webServer: [
    {
      command: "cargo run -q -p taskr-server",
      cwd: "../..",
      url: `http://127.0.0.1:${API_PORT}/api/health`,
      env: { TASKR_STORAGE: "memory", TASKR_DEV_AUTH: "1", TASKR_BIND: `127.0.0.1:${API_PORT}` },
      timeout: 300_000,
    },
    {
      command: `npx vite --host 127.0.0.1 --port ${SIGNED_IN_PORT} --strictPort`,
      url: `http://127.0.0.1:${SIGNED_IN_PORT}`,
      env: { VITE_TASKR_API: "/api", TASKR_API_PROXY: `http://127.0.0.1:${API_PORT}` },
      timeout: 120_000,
    },
    {
      command: "npx vite --host 127.0.0.1 --port 5198 --strictPort",
      url: LOCAL_URL,
      env: { VITE_TASKR_API: "" },
      timeout: 120_000,
    },
  ],
});
