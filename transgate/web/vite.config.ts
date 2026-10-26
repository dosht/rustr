/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { svelteTesting } from "@testing-library/svelte/vite";

// In dev the browser sees one origin, as in the packaged image where Axum
// serves both the app and the API: /api and the provider browser return go to
// transgate-server, /auth/v1 to the local GoTrue.
const host = process.env.TRANSGATE_HOST_PROXY ?? "http://127.0.0.1:8788";
const auth = process.env.TRANSGATE_AUTH_PROXY ?? "http://127.0.0.1:9999";

export default defineConfig({
  plugins: [tailwindcss(), svelte(), svelteTesting()],
  resolve: {
    alias: { $lib: fileURLToPath(new URL("./src/lib", import.meta.url)) },
  },
  // P5 D20: every app-owned asset (JS, CSS, wasm, licensed fonts/notices,
  // invoice logo, rating art) is served under /_app/*; the source public
  // asset path stays owned by P6.
  build: {
    assetsDir: "_app",
    // Never inline imported assets as data: URLs: license notices must stay
    // linkable files, and Rust fetches fonts under connect-src 'self'.
    assetsInlineLimit: 0,
  },
  server: {
    // Document fonts, OFL notices and the invoice logo have one versioned copy
    // in transgate/export (p1W); src/lib/documents/assets.ts imports them ?url.
    fs: { allow: [searchForWorkspaceRoot(process.cwd()), fileURLToPath(new URL("../export", import.meta.url))] },
    proxy: {
      "/api": host,
      "/checkout/confirm": host,
      "/auth/v1": { target: auth, rewrite: (path) => path.replace(/^\/auth\/v1/, "") },
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    setupFiles: ["src/test/setup.ts"],
  },
});
