import { defineConfig, devices } from "@playwright/test";

// Browser-only parity rules (transgate/docs/rules.md `playwright:` references) run on
// both projects; scripts/check-prd.py requires a pass on `desktop` and `mobile`.
// The built shell is served by `vite preview`; each spec injects the host's
// `tg-config` island and answers every non-app origin itself, so no live
// provider, GoTrue or Axum host is contacted.
const port = 4319;

export default defineConfig({
  // The JSON report keys specs relative to testDir; check-prd.py resolves them under
  // transgate/web, so references read `playwright:e2e/<file>.spec.ts#…`.
  testDir: ".",
  testMatch: "e2e/**/*.spec.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? "line" : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"] } },
    { name: "mobile", use: { ...devices["Pixel 7"] } },
  ],
  webServer: {
    command: `npm run build && npx vite preview --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/signin`,
    reuseExistingServer: !process.env.CI,
    timeout: 300_000,
  },
});
