/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";

// In dev, /api goes to taskr-server and /auth to a GoTrue container, so the
// browser sees one origin and needs no CORS.
const api = process.env.TASKR_API_PROXY ?? "http://127.0.0.1:8787";
const auth = process.env.TASKR_AUTH_PROXY ?? "http://127.0.0.1:9999";

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  server: {
    proxy: {
      "/api": api,
      "/auth": { target: auth, rewrite: (path) => path.replace(/^\/auth/, "") },
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    setupFiles: ["src/test/setup.ts"],
  },
});
