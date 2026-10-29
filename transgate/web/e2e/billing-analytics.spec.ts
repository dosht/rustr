import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { gunzipSync } from "node:zlib";
import { expect, test, type Page } from "@playwright/test";
import { CONFIG, DISPLAY_NAME, SESSION, USER, EMAIL } from "./support/shell";

const DAY = 86_400_000_000;

async function accountMenu(page: Page) {
  if ((page.viewportSize()?.width ?? 0) < 768) {
    await page.getByRole("button", { name: "Main navigation menu", exact: true }).click();
    await page.locator("dialog[open]").getByRole("button").last().click();
  } else {
    await page.getByRole("button", { name: DISPLAY_NAME, exact: true }).click();
  }
}

for (const consent of ["Accept", "Reject"] as const) {
  test.describe(`BIL-072 ${consent}`, () => {
    test("billing intents reach PostHog once only with consent", async ({ context, page }, testInfo) => {
      const events: { event: string; properties: Record<string, unknown> }[] = [];
      const telemetry: string[] = [];
      const checkout: Record<string, unknown>[] = [];
      const cancellations: unknown[] = [];
      const now = Date.now() * 1000;
      let canceledAt: number | null = null;
      const cancellation = Promise.withResolvers<void>();
      await context.addInitScript(([user, email]) => {
        Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });
        Object.defineProperty(Navigator.prototype, "userAgentData", { get: () => undefined });
        localStorage.setItem("transgate.session", JSON.stringify({ access_token: "e2e-access", refresh_token: "e2e-refresh", expires_at: Math.floor(Date.now() / 1000) + 3600, user_id: user, email }));
      }, [USER, EMAIL]);
      let origin = "";
      const failures: unknown[] = [];
      // Browser routing can cancel unload beacons, so checkout and telemetry use a real loopback host.
      const collector = createServer(async (request, response) => {
        const json = (value: unknown) => { response.setHeader("Content-Type", "application/json"); response.end(JSON.stringify(value)); };
        const html = (value: string) => { response.setHeader("Content-Type", "text/html"); response.end(value); };
        try {
          const chunks: Buffer[] = [];
          for await (const chunk of request) chunks.push(Buffer.from(chunk));
          const raw = Buffer.concat(chunks);
          const path = new URL(request.url ?? "/", origin).pathname;
          if (path.startsWith("/telemetry/")) {
            telemetry.push(path);
            if (raw.length) {
              const text = (raw[0] === 0x1f && raw[1] === 0x8b ? gunzipSync(raw) : raw).toString();
              const data = new URLSearchParams(text).get("data");
              const payload = JSON.parse(data ? Buffer.from(data, "base64").toString() : text);
              events.push(...(Array.isArray(payload) ? payload : payload.batch ?? [payload]));
            }
            if (path.endsWith("/config.js")) { response.setHeader("Content-Type", "application/javascript"); response.end(""); return; }
            return json(path.includes("/flags") ? { featureFlags: {}, featureFlagPayloads: {} } : { status: 1 });
          }
          if (path === "/api/session") return json(SESSION);
          if (path === "/api/billing/overview") return json({ balance: 1200, is_freemium: false, special_client_alert: null });
          if (path === "/api/billing/subscription") return json({
            package_name: "PREMIUM", minutes_included: 1200, valid_until: now + 20 * DAY,
            payment: { payment_id: 9, created: now - 10 * DAY, canceled_at: canceledAt, next_payment_at: canceledAt ? null : now + 20 * DAY },
          });
          if (path === "/api/billing/cancel-subscription") {
            cancellations.push(JSON.parse(raw.toString()));
            await cancellation.promise;
            canceledAt = now;
            return json({ payment_id: 9, canceled_at: canceledAt });
          }
          if (path === "/api/billing/catalog") return json({ plans: [], pay_as_you_go: { price_text: "$1.49/hour", struck_per_hour: "2.00" } });
          if (path === "/api/billing/quote") return json({
            product_name: "Pay-as-you-Go", amount: "14.90", display_amount: "14.80", currency: "USD",
            coupon_id: null, discount_applied: false, pricing_plan_id: null, hours: 10, usd_amount: "14.90", rate: "44",
            spec: { name: "Pay-as-you-Go", minutes: 600, count: 1, interval: "month", frequency: 1, expiry: 12 },
          });
          if (path === "/checkout" && request.method === "POST") {
            const form = new URLSearchParams(raw.toString());
            checkout.push(JSON.parse(Buffer.from(form.get("order")!, "base64url").toString()));
            return html("<!doctype html><title>Checkout</title><h1>Checkout received</h1>");
          }
          if (path.startsWith("/api/")) { response.statusCode = 404; return json({ code: "not_found", error: "e2e: not answered" }); }
          if (path.startsWith("/_app/")) {
            const types: Record<string, string> = { js: "application/javascript", css: "text/css", wasm: "application/wasm", json: "application/json" };
            response.setHeader("Content-Type", types[path.split(".").at(-1)!] ?? "application/octet-stream");
            response.end(await readFile(resolve(import.meta.dirname, "../dist", `.${path}`)));
            return;
          }
          const config = { ...CONFIG, authUrl: `${origin}/auth/v1`, siteUrl: origin, telemetry: { posthog: { key: "phc_e2e", host: `${origin}/telemetry` }, sentry: null } };
          const document = await readFile(resolve(import.meta.dirname, "../dist/index.html"), "utf8");
          return html(document.replace("<!--tg-inject-->", `<script type="application/json" id="tg-config">${JSON.stringify(config)}</script>`));
        } catch (error) {
          failures.push(String(error));
          response.statusCode = 500;
          response.end("Fixture failed");
        }
      });
      await new Promise<void>(resolve => collector.listen(0, "127.0.0.1", resolve));
      const address = collector.address();
      if (!address || typeof address === "string") throw new Error("No collector port");
      origin = `http://127.0.0.1:${address.port}`;
      try {
        await page.goto(`${origin}/home?utm_source=billing-test&utm_medium=e2e&utm_campaign=retention`);
        await page.getByRole("button", { name: consent, exact: true }).click();
        if (consent === "Accept") await expect.poll(() => events.map(e => e.event), { timeout: 15_000 }).toContain("cookie_consent_accepted");
        await accountMenu(page);
        await page.getByRole("button", { name: "Settings", exact: true }).click();
        await page.getByRole("tab", { name: "Subscription", exact: true }).click();
        await page.getByRole("button", { name: "Cancel Subscription", exact: true }).click();
        const confirm = page.getByRole("dialog", { name: "Are you sure you want to cancel your subscription?" });
        await confirm.getByRole("button", { name: "No", exact: true }).click();
        expect(cancellations).toEqual([]);
        expect(events.filter(e => e.event === "subscription_cancelled")).toEqual([]);
        await page.getByRole("button", { name: "Cancel Subscription", exact: true }).click();
        await confirm.getByRole("button", { name: "Yes", exact: true }).click();
        await expect.poll(() => cancellations.length).toBe(1);
        await expect(confirm.getByRole("button", { name: "Yes", exact: true })).toBeDisabled();
        expect(events.filter(e => e.event === "subscription_cancelled")).toEqual([]);
        cancellation.resolve();
        await expect(page.getByText("Your subscription is canceled, but you can still use your minutes.")).toBeVisible();
        expect(cancellations).toEqual([{ payment_id: 9 }]);
        await page.getByRole("dialog").getByRole("button", { name: "Close", exact: true }).click();
        await accountMenu(page);
        await page.getByRole("button", { name: testInfo.project.name === "mobile" ? "Upgrade Now" : "Change Plan", exact: true }).click();
        await page.getByRole("checkbox").check();
        const pay = page.getByRole("button", { name: "Pay with Card", exact: true });
        await expect(pay).toBeEnabled();
        await pay.click({ noWaitAfter: true });
        await expect.poll(() => checkout.length).toBe(1);
        await expect(page.getByRole("heading", { name: "Checkout received" })).toBeVisible();
        await expect(page).toHaveURL(/\/checkout$/);
        expect(checkout[0].analytics).toMatchObject({ utm_source: "billing-test", utm_medium: "e2e", utm_campaign: "retention" });
        const billingEvents = () => events.filter(e => ["checkout_started", "subscription_cancelled"].includes(e.event));
        if (consent === "Accept") {
          await expect.poll(() => billingEvents().length, { timeout: 15_000 }).toBe(2);
          expect(billingEvents().find(e => e.event === "checkout_started")?.properties).toMatchObject({
            plan: "pay-as-you-go", amount: 14.8, currency: "USD", billing_cycle: null,
            utm_source: "billing-test", utm_medium: "e2e", utm_campaign: "retention",
            browser: "Chrome", device_type: testInfo.project.name === "mobile" ? "mobile" : "desktop",
          });
          expect(billingEvents().find(e => e.event === "subscription_cancelled")?.properties).toMatchObject({ plan: "PREMIUM", days_subscribed: 10 });
        }
        await page.waitForTimeout(3500);
        expect(billingEvents().length).toBe(consent === "Accept" ? 2 : 0);
        expect(checkout).toHaveLength(1);
        expect(cancellations).toHaveLength(1);
        if (consent === "Reject") expect(telemetry).toEqual([]);
        expect(failures).toEqual([]);
      } finally {
        collector.closeAllConnections();
        await new Promise<void>((resolve, reject) => collector.close(error => error ? reject(error) : resolve()));
      }
    });
  });
}
