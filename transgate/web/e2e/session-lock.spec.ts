import { expect, test, type BrowserContext } from "@playwright/test";
import { CONFIG, EMAIL, SESSION, USER, shell } from "./support/shell";

const UNAVAILABLE = "Sign-in is temporarily unavailable. Please try again later.";
const SESSION_KEY = "transgate.session";

type LockMode = "missing" | "rejected" | "aborted" | "throws" | "native";
type LockState = { requested: string[]; active: number; released: number; blockerHeld: boolean };

declare global {
  interface Window {
    sessionLockTest: LockState;
  }
}

async function host(context: BrowserContext, mode: LockMode) {
  const refreshes: { token: unknown; active: number; held: string[] }[] = [];
  const profiles: string[] = [];
  await context.addInitScript(({ mode, user, email }) => {
    const native = navigator.locks;
    const state: LockState = window.sessionLockTest = { requested: [], active: 0, released: 0, blockerHeld: false };
    sessionStorage.setItem("session-lock-mode", sessionStorage.getItem("session-lock-mode") ?? mode);
    if (!localStorage.getItem("transgate.session")) {
      localStorage.setItem("transgate.session", JSON.stringify({
        access_token: "expired-access", refresh_token: "original-refresh", expires_at: 1, user_id: user, email,
      }));
    }
    if (window.name === "lock-holder") {
      void native.request("transgate.session", () => {
        state.blockerHeld = true;
        return new Promise<void>(() => {});
      });
    }
    const locks = {
      query: () => native.query(),
      request(name: string, callback: LockGrantedCallback) {
        state.requested.push(name);
        const current = sessionStorage.getItem("session-lock-mode");
        if (current === "throws") throw new Error("Lock request threw");
        if (current === "rejected") return Promise.reject(new Error("Lock request rejected"));
        if (current === "aborted") return Promise.reject(new DOMException("Lock acquisition cancelled", "AbortError"));
        return native.request(name, async lock => {
          state.active += 1;
          try {
            return await callback(lock);
          } finally {
            state.active -= 1;
            state.released += 1;
          }
        });
      },
    };
    Object.defineProperty(navigator, "locks", { get: () => sessionStorage.getItem("session-lock-mode") === "missing" ? undefined : locks });
  }, { mode, user: USER, email: EMAIL });
  await shell(context, {
    consent: "rejected",
    handlers: [(request, url, route) => {
      if (url.pathname === "/api/session") {
        profiles.push(request.headers().authorization);
        return route.fulfill({ json: SESSION });
      }
      return undefined;
    }],
    external: (request, url, route) => {
      if (url.origin !== new URL(CONFIG.authUrl).origin || url.pathname !== "/auth/v1/token") return undefined;
      return (async () => {
        expect(url.searchParams.get("grant_type")).toBe("refresh_token");
        const lock = await request.frame().page().evaluate(async () => ({
          active: window.sessionLockTest.active,
          held: (await navigator.locks.query()).held?.map(lock => lock.name ?? "") ?? [],
        }));
        refreshes.push({ token: request.postDataJSON().refresh_token, ...lock });
        await route.fulfill({ json: {
          access_token: "rotated-access", refresh_token: "rotated-refresh", expires_in: 3600,
          user: { id: USER, email: EMAIL },
        } });
      })();
    },
  });
  return { refreshes, profiles };
}

for (const mode of ["missing", "rejected", "aborted", "throws"] as const) {
  test.describe(`session lock ${mode}`, () => {
    test("shows a recoverable failure without refreshing or clearing the session, then retries under a lock", async ({ context, page }) => {
      const seen = await host(context, mode);
      await page.goto("/home");
      await expect(page.getByRole("alert")).toHaveText(UNAVAILABLE);
      await expect(page.getByRole("textbox", { name: "Email address" })).toBeVisible();
      expect(seen.refreshes).toEqual([]);
      expect(seen.profiles).toEqual([]);
      expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)!), SESSION_KEY)).toMatchObject({
        access_token: "expired-access", refresh_token: "original-refresh", expires_at: 1,
      });
      expect(await page.evaluate(() => window.sessionLockTest.requested)).toEqual(mode === "missing" ? [] : [SESSION_KEY]);
      await page.evaluate(() => sessionStorage.setItem("session-lock-mode", "native"));
      await page.reload();
      await expect.poll(() => seen.profiles).toEqual(["Bearer rotated-access"]);
      await expect(page.getByRole("alert").filter({ hasText: UNAVAILABLE })).toHaveCount(0);
      await expect(page.getByRole("textbox", { name: "Email address" })).toHaveCount(0);
      expect(seen.refreshes).toEqual([{ token: "original-refresh", active: 1, held: [SESSION_KEY] }]);
      await expect.poll(() => page.evaluate(() => window.sessionLockTest.released)).toBe(1);
      expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)!), SESSION_KEY)).toMatchObject({
        access_token: "rotated-access", refresh_token: "rotated-refresh",
      });
    });
  });
}

test("closing a tab while acquisition is queued never refreshes unlocked or blocks a retrying tab", async ({ context, page }) => {
  const seen = await host(context, "native");
  const blocker = await context.newPage();
  await blocker.evaluate(() => { window.name = "lock-holder"; });
  await blocker.goto("/home");
  await expect.poll(() => blocker.evaluate(() => window.sessionLockTest.blockerHeld)).toBe(true);
  await page.goto("/home");
  await expect.poll(() => page.evaluate(() => window.sessionLockTest.requested)).toEqual([SESSION_KEY]);
  expect(seen.refreshes).toEqual([]);
  expect(seen.profiles).toEqual([]);
  await page.close();
  const retry = await context.newPage();
  await retry.goto("/home");
  await expect.poll(() => retry.evaluate(() => window.sessionLockTest.requested)).toEqual([SESSION_KEY]);
  expect(seen.refreshes).toEqual([]);
  await blocker.close();
  await expect.poll(() => seen.profiles).toEqual(["Bearer rotated-access"]);
  expect(seen.refreshes).toEqual([{ token: "original-refresh", active: 1, held: [SESSION_KEY] }]);
  await expect.poll(() => retry.evaluate(() => window.sessionLockTest.released)).toBe(1);
});
