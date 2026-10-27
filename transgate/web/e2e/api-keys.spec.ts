// WEB-045/046 and API-F08/F09: the key screen at /dashboard/api-keys on the real
// built shell. The session routes (/api/keys, /api/keys/signing-secret,
// /api/session) are answered here with the wire shapes of transgate_api::public
// and core's SessionProfile; nothing reaches a host, GoTrue or a provider.
import { expect, test, type BrowserContext, type Page } from "@playwright/test";

const USER = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b";
const EMAIL = "key-owner@example.test";
const SECRET = "dGVzdC1zaWduaW5nLXNlY3JldC1ub3QtYS1jcmVkZW50aWFs";

const CONFIG = {
  apiUrl: "/api",
  authUrl: "https://auth.example.test/auth/v1",
  authKey: null,
  siteUrl: "https://app.example.test",
  captcha: null,
  telemetry: { posthog: null, sentry: null },
};

const SCOPES = {
  read: ["transcriptions:read"],
  create: ["transcriptions:create"],
  read_and_create: ["transcriptions:read", "transcriptions:create"],
} as const;

interface Key {
  id: string;
  name: string;
  createdAt: string;
  expiresAt: string | null;
  isActive: boolean;
  scopes: string[];
  expirationPolicy: string;
  hint: string | null;
}

interface Host {
  keys: Key[];
  /** Plaintext keys handed out by POST /api/keys, in order. */
  issued: string[];
  revoked: string[];
  /** GET /api/keys answers: "ok", "fail" (500) or a pending promise holding the answer. */
  list: "ok" | "fail" | Promise<void>;
}

async function signedIn(context: BrowserContext, host: Host): Promise<void> {
  await context.addInitScript(
    ([user, email]) => {
      const session = { access_token: "e2e-access", refresh_token: "e2e-refresh", expires_at: Math.floor(Date.now() / 1000) + 3600, user_id: user, email };
      localStorage.setItem("transgate.session", JSON.stringify(session));
      document.cookie = "cookieConsent=rejected; Path=/";
    },
    [USER, EMAIL],
  );
  await context.route("**/*", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.hostname === "127.0.0.1" && !url.pathname.startsWith("/api/")) {
      if (request.resourceType() === "document") {
        const response = await route.fetch();
        const island = `<script type="application/json" id="tg-config">${JSON.stringify(CONFIG).replace(/</g, "\\u003c")}</script>`;
        return route.fulfill({ response, body: (await response.text()).replace("<!--tg-inject-->", island) });
      }
      return route.continue();
    }
    if (url.hostname !== "127.0.0.1") return route.abort();
    const method = request.method();
    const path = url.pathname;
    if (path === "/api/session") {
      return route.fulfill({
        json: {
          account: USER, email: EMAIL, full_name: null, display_name: "key-owner", joined_on: null,
          capabilities: {
            ai: false, ai_chat: false, youtube: false, microphone: false, google_drive: false, onedrive: false, show_original: false,
            mobile_ux_jobs: true, mobile_expanded_player: false, mobile_ux_ai: true, billing_history: true, customer_lookup: false,
          },
        },
      });
    }
    if (path === "/api/keys/signing-secret" && method === "GET") return route.fulfill({ json: { secret: SECRET } });
    if (path === "/api/keys" && method === "GET") {
      if (host.list instanceof Promise) await host.list;
      if (host.list === "fail") return route.fulfill({ status: 500, json: { code: "internal", error: "Internal error" } });
      return route.fulfill({ json: { keys: host.keys } });
    }
    if (path === "/api/keys" && method === "POST") {
      const body = request.postDataJSON() as { name: string; scopes: string[]; expirationPolicy: string };
      const n = host.issued.length + 1;
      const apiKey = `sk_e2e${String(n).padStart(2, "0")}${"Q".repeat(39)}=`;
      const key: Key = {
        id: `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`, name: body.name, createdAt: new Date().toISOString(), expiresAt: null,
        isActive: true, scopes: body.scopes, expirationPolicy: body.expirationPolicy, hint: apiKey.slice(-8),
      };
      host.keys.unshift(key);
      host.issued.push(apiKey);
      return route.fulfill({ status: 201, json: { key, apiKey } });
    }
    const revoke = path.match(/^\/api\/keys\/([0-9a-f-]+)$/);
    if (revoke && method === "DELETE") {
      const key = host.keys.find((k) => k.id === revoke[1])!;
      key.isActive = false;
      host.revoked.push(key.id);
      return route.fulfill({ json: { key } });
    }
    // Everything else the signed-in frame reads is outside the key screen.
    return route.fulfill({ status: 404, json: { code: "not_found", error: "Not found" } });
  });
}

function newHost(list: Host["list"] = "ok"): Host {
  return { keys: [], issued: [], revoked: [], list };
}

const isPhone = (page: Page) => (page.viewportSize()?.width ?? 0) < 768;

async function openKeys(page: Page): Promise<void> {
  await page.goto("/dashboard/api-keys");
  await expect(page.getByRole("heading", { name: "Your API Keys" })).toBeVisible();
}

async function create(page: Page, name: string, permission: keyof typeof SCOPES): Promise<string> {
  await page.getByRole("button", { name: "Generate New Key", exact: true }).first().click();
  await page.getByLabel("Key Name").fill(name);
  await page.getByLabel("Permissions").selectOption(permission);
  await page.getByRole("button", { name: "Generate Key", exact: true }).click();
  await expect(page.getByText("Your new API key")).toBeVisible();
  return (await page.locator("main code").filter({ hasText: /^sk_/ }).textContent())!.trim();
}

test.describe("api keys", () => {
  test.beforeEach(async ({ context }) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  });

  test("the desktop account menu opens the key screen; on phone it is reached by URL", async ({ context, page }) => {
    await signedIn(context, newHost());
    await page.goto("/home");
    if (isPhone(page)) {
      await page.getByRole("button", { name: "Main navigation menu", exact: true }).click();
      await page.locator("dialog[open]").getByRole("button").last().click();
      await expect(page.getByRole("button", { name: "Settings", exact: true })).toBeVisible();
      await expect(page.getByRole("button", { name: "API Keys", exact: true })).toHaveCount(0);
      await openKeys(page);
    } else {
      await page.getByRole("button", { name: "key-owner", exact: true }).click();
      await page.getByRole("button", { name: "API Keys", exact: true }).click();
      await expect(page).toHaveURL(/\/dashboard\/api-keys$/);
      await expect(page.getByRole("heading", { name: "Your API Keys" })).toBeVisible();
    }
  });

  test("an empty account shows no keys and the owner's signing secret, which copies", async ({ context, page }) => {
    await signedIn(context, newHost());
    await openKeys(page);
    await expect(page.getByText("No API keys found.")).toBeVisible();
    await expect(page.getByText("Callback signing secret")).toBeVisible();
    await expect(page.locator("main code").filter({ hasText: SECRET })).toBeVisible();
    await page.getByRole("button", { name: "Copy secret" }).click();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(SECRET);
  });

  test("each scope creates a key that is shown once, copies, and is hidden after reload", async ({ context, page }) => {
    const host = newHost();
    await signedIn(context, host);
    await openKeys(page);
    for (const [permission, name] of [["read_and_create", "both"], ["read", "reader"], ["create", "creator"]] as const) {
      const key = await create(page, name, permission);
      expect(key).toBe(host.issued.at(-1));
      expect(host.keys[0].scopes).toEqual(SCOPES[permission]);
      await page.locator("main").getByRole("button", { name: "Copy", exact: true }).click();
      await expect(page.locator("main").getByRole("button", { name: "Copied!", exact: true })).toBeVisible();
      expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(key);
    }
    await page.reload();
    await expect(page.getByRole("heading", { name: "Your API Keys" })).toBeVisible();
    for (const name of ["both", "reader", "creator"]) await expect(page.locator("main").getByText(name, { exact: true })).toBeVisible();
    const text = await page.locator("main").innerText();
    for (const key of host.issued) expect(text).not.toContain(key);
    await expect(page.getByText("Your new API key")).toHaveCount(0);
  });

  test("revoking asks for confirmation (dialog on desktop, sheet on phone) and marks the key revoked", async ({ context, page }) => {
    const host = newHost();
    await signedIn(context, host);
    await openKeys(page);
    await create(page, "to-revoke", "read_and_create");
    await page.reload();
    await expect(page.locator("main").getByText("to-revoke", { exact: true })).toBeVisible();
    await page.locator("main").getByRole("button", { name: "Revoke", exact: true }).click();
    const confirm = page.getByRole("dialog", { name: "Revoke API key" });
    await expect(confirm).toBeVisible();
    await expect(confirm.getByText("Are you sure you want to revoke this API key?")).toBeVisible();
    const box = (await confirm.boundingBox())!;
    const viewport = page.viewportSize()!;
    if (isPhone(page)) expect(box.y + box.height).toBeGreaterThan(viewport.height - 2);
    else expect(box.y + box.height).toBeLessThan(viewport.height - 2);
    await confirm.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(confirm).toBeHidden();
    expect(host.revoked).toEqual([]);
    await page.locator("main").getByRole("button", { name: "Revoke", exact: true }).click();
    await confirm.getByRole("button", { name: "Revoke", exact: true }).click();
    await expect(confirm).toBeHidden();
    expect(host.revoked).toEqual([host.keys[0].id]);
    await expect(page.locator("main").getByText("Revoked", { exact: true })).toBeVisible();
    await expect(page.locator("main").getByRole("button", { name: "Revoke", exact: true })).toHaveCount(0);
  });

  test("the list shows a loading state until the keys arrive", async ({ context, page }) => {
    const { promise, resolve: release } = Promise.withResolvers<void>();
    const host = newHost(promise);
    await signedIn(context, host);
    await openKeys(page);
    await expect(page.getByText("Loading...")).toBeVisible();
    host.list = "ok";
    release();
    await expect(page.getByText("No API keys found.")).toBeVisible();
    await expect(page.getByText("Loading...")).toHaveCount(0);
  });

  test("a failed load shows the error and the empty state without a table", async ({ context, page }) => {
    await signedIn(context, newHost("fail"));
    await openKeys(page);
    await expect(page.getByText("Failed to load API keys.")).toBeVisible();
    await expect(page.getByText("No API keys found.")).toBeVisible();
    await expect(page.locator("main table")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Generate New Key", exact: true }).first()).toBeVisible();
  });
});
