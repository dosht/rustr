// PRV-046..048 (transgate/docs/rules.md): cookie consent and the PostHog gate.
// Real built shell and real posthog-js / @sentry/browser SDKs; the telemetry
// origins below are answered here, never contacted. Recorded P5 D12
// correction: no SDK chunk and no telemetry request exist before consent (the
// source loaded PostHog opted out), and GTM/gtag are not carried over.
import { gunzipSync } from "node:zlib";
import { expect, test, type BrowserContext, type Page, type Request } from "@playwright/test";

const POSTHOG = "https://ph.example.test";
const SENTRY = "https://sentry.example.test";
const DAY = 24 * 60 * 60;

const CONFIG = {
  apiUrl: "/api",
  authUrl: "https://auth.example.test/auth/v1",
  authKey: null,
  siteUrl: "https://app.example.test",
  captcha: null,
  telemetry: {
    posthog: { key: "phc_e2e", host: POSTHOG },
    sentry: { dsn: "https://e2ekey@sentry.example.test/1", environment: "e2e" },
  },
};

interface Observed {
  telemetry: string[];
  events: string[];
  sdkChunks: string[];
  personProfiles: unknown[];
}

function body(request: Request): unknown[] {
  const raw = request.postDataBuffer();
  if (!raw) return [];
  const text = (raw[0] === 0x1f && raw[1] === 0x8b ? gunzipSync(raw) : raw).toString("utf8");
  try {
    const json = JSON.parse(text);
    return Array.isArray(json) ? json : Array.isArray(json.batch) ? json.batch : [json];
  } catch {
    return [];
  }
}

async function open(context: BrowserContext, page: Page, path: string): Promise<Observed> {
  const seen: Observed = { telemetry: [], events: [], sdkChunks: [], personProfiles: [] };
  // PostHog drops events from automated browsers (navigator.webdriver, and the
  // HeadlessChrome client-hint brand); present as an ordinary visitor.
  await context.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });
    Object.defineProperty(Navigator.prototype, "userAgentData", { get: () => undefined });
  });
  await context.route("**/*", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.hostname === "127.0.0.1") {
      if (request.resourceType() === "document") {
        const response = await route.fetch();
        const island = `<script type="application/json" id="tg-config">${JSON.stringify(CONFIG).replace(/</g, "\\u003c")}</script>`;
        return route.fulfill({ response, body: (await response.text()).replace("<!--tg-inject-->", island) });
      }
      if (/\/_app\/(module|prod)-/.test(url.pathname)) seen.sdkChunks.push(url.pathname);
      return route.continue();
    }
    if (url.origin === POSTHOG || url.origin === SENTRY) {
      seen.telemetry.push(`${request.method()} ${url.origin}${url.pathname}`);
      for (const event of body(request) as { event?: string; properties?: Record<string, unknown> }[]) {
        if (event.event) seen.events.push(event.event);
        if (event.properties && "$process_person_profile" in event.properties) seen.personProfiles.push(event.properties.$process_person_profile);
      }
      if (url.pathname.endsWith("/config.js")) return route.fulfill({ status: 200, contentType: "application/javascript", body: "" });
      if (url.pathname.includes("/flags")) return route.fulfill({ status: 200, json: { featureFlags: {}, featureFlagPayloads: {}, errorsWhileComputingFlags: false } });
      return route.fulfill({ status: 200, json: { status: 1 } });
    }
    return route.abort();
  });
  await page.goto(path);
  await expect(page.locator('input[type="email"]')).toBeVisible();
  return seen;
}

async function consentCookie(context: BrowserContext) {
  return (await context.cookies()).find((cookie) => cookie.name === "cookieConsent");
}

function expectYear(expires: number) {
  const days = (expires - Date.now() / 1000) / DAY;
  expect(days).toBeGreaterThan(364);
  expect(days).toBeLessThanOrEqual(365.01);
}

test.describe("cookie consent", () => {
  test("shows the banner without a stored choice and loads no telemetry before consent", async ({ context, page }) => {
    const seen = await open(context, page, "/signin");
    await expect(page.getByRole("button", { name: "Accept", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Reject", exact: true })).toBeVisible();
    await page.waitForTimeout(1500);
    expect(seen.telemetry).toEqual([]);
    expect(seen.sdkChunks).toEqual([]);
    expect(await consentCookie(context)).toBeUndefined();
  });

  test("rejecting stores cookieConsent=rejected for 365 days, hides the banner and sends nothing", async ({ context, page }) => {
    const seen = await open(context, page, "/signin");
    await page.getByRole("button", { name: "Reject", exact: true }).click();
    await expect(page.getByRole("button", { name: "Reject", exact: true })).toBeHidden();
    const cookie = await consentCookie(context);
    expect(cookie?.value).toBe("rejected");
    expectYear(cookie!.expires);
    await page.reload();
    await expect(page.getByRole("textbox", { name: "Email address" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Accept", exact: true })).toBeHidden();
    await page.waitForTimeout(1500);
    expect(seen.telemetry).toEqual([]);
    expect(seen.sdkChunks).toEqual([]);
  });

  test("accepting stores cookieConsent=accepted for 365 days, hides the banner and captures cookie_consent_accepted and $pageview", async ({ context, page }) => {
    const seen = await open(context, page, "/signin");
    await page.getByRole("button", { name: "Accept", exact: true }).click();
    await expect(page.getByRole("button", { name: "Accept", exact: true })).toBeHidden();
    const cookie = await consentCookie(context);
    expect(cookie?.value).toBe("accepted");
    expectYear(cookie!.expires);
    await expect.poll(() => seen.events, { timeout: 15_000 }).toEqual(expect.arrayContaining(["cookie_consent_accepted", "$pageview"]));
    // Both consent-gated SDK chunks (PostHog and Sentry) load once each.
    expect(new Set(seen.sdkChunks).size).toBe(2);
    expect(seen.sdkChunks.length).toBe(2);
  });

  test("a stored accepted choice opts PostHog in on load with identified-only person profiles", async ({ context, page }) => {
    await context.addCookies([{ name: "cookieConsent", value: "accepted", url: "http://127.0.0.1:4319" }]);
    const seen = await open(context, page, "/signin");
    await expect(page.getByRole("button", { name: "Accept", exact: true })).toBeHidden();
    await expect.poll(() => seen.events, { timeout: 15_000 }).toContain("$pageview");
    expect(seen.events).not.toContain("cookie_consent_accepted");
    // `person_profiles: "identified_only"`: an anonymous visitor's events carry no person profile.
    expect(seen.personProfiles.length).toBeGreaterThan(0);
    expect(seen.personProfiles.every((value) => value === false)).toBe(true);
  });

  test("PostHog does not start for the id locale", async ({ context, page }) => {
    const seen = await open(context, page, "/id/signin");
    // `common` "Accept" in id is "Terima" (locales/id/common.json).
    await page.getByRole("button", { name: "Terima", exact: true }).click();
    // Consent is honoured (the Sentry chunk loads) but PostHog is never imported or contacted.
    await expect.poll(() => seen.sdkChunks.length, { timeout: 15_000 }).toBe(1);
    await page.waitForTimeout(3000);
    expect(seen.sdkChunks.length).toBe(1);
    expect(seen.telemetry.filter((entry) => entry.includes(POSTHOG))).toEqual([]);
    expect((await consentCookie(context))?.value).toBe("accepted");
  });
});
