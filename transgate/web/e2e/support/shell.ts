// Shared harness for the hermetic Transgate specs: the real built shell from
// `vite preview`, the host's `tg-config` island injected into each document, and
// every request answered by the spec. Nothing reaches an Axum host, GoTrue or a
// provider; any origin a spec does not answer is aborted.
import type { BrowserContext, Request, Route } from "@playwright/test";

export const USER = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b";
export const EMAIL = "e2e-owner@example.test";
export const DISPLAY_NAME = "e2e-owner";

export interface ShellConfig {
  apiUrl: string;
  authUrl: string;
  authKey: null;
  siteUrl: string;
  captcha: null;
  telemetry: { posthog: { key: string; host: string } | null; sentry: { dsn: string; environment: string } | null };
}

export const CONFIG: ShellConfig = {
  apiUrl: "/api",
  authUrl: "https://auth.example.test/auth/v1",
  authKey: null,
  siteUrl: "https://app.example.test",
  captcha: null,
  telemetry: { posthog: null, sentry: null },
};

/** core's SessionProfile (`GET /api/session`), every optional capability off. */
export const SESSION = {
  account: USER, email: EMAIL, full_name: null, display_name: DISPLAY_NAME, joined_on: null,
  capabilities: {
    ai: false, ai_chat: false, youtube: false, microphone: false, google_drive: false, onedrive: false, show_original: false,
    mobile_ux_jobs: true, mobile_expanded_player: false, mobile_ux_ai: true, billing_history: true, customer_lookup: false,
  },
};

/** Answers a request by returning the promise of its answer (`route.fulfill`,
 * `route.continue`, ...); returning `undefined` passes it to the next handler. */
export type Handler = (request: Request, url: URL, route: Route) => Promise<unknown> | undefined;

export interface ShellOptions {
  config?: ShellConfig;
  /** Restores a signed-in browser session and answers `/api/session`. */
  signedIn?: boolean;
  /** Pre-records a cookie choice so the consent banner does not cover the page. */
  consent?: "accepted" | "rejected";
  /** Same-origin `/api/*` and `/_app/*` answers, tried in order. */
  handlers?: Handler[];
  /** Answers for non-app origins (for example telemetry); others are aborted. */
  external?: Handler;
}

/** Unanswered `/api/*` reads: an error body that is not catalog text, so a shown message is never mistaken for UI copy. */
export const UNANSWERED = { status: 404, json: { code: "not_found", error: "e2e: not answered" } };

export async function shell(context: BrowserContext, options: ShellOptions = {}): Promise<void> {
  const config = options.config ?? CONFIG;
  if (options.signedIn) {
    await context.addInitScript(
      ([user, email]) => {
        const session = { access_token: "e2e-access", refresh_token: "e2e-refresh", expires_at: Math.floor(Date.now() / 1000) + 3600, user_id: user, email };
        localStorage.setItem("transgate.session", JSON.stringify(session));
      },
      [USER, EMAIL],
    );
  }
  if (options.consent) {
    const value = options.consent;
    await context.addInitScript((choice) => { document.cookie = `cookieConsent=${choice}; Path=/`; }, value);
  }
  await context.route("**/*", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.hostname !== "127.0.0.1") {
      const answer = options.external?.(request, url, route);
      if (answer !== undefined) return answer;
      return route.abort();
    }
    for (const handler of options.handlers ?? []) {
      const answer = handler(request, url, route);
      if (answer !== undefined) return answer;
    }
    if (request.resourceType() === "document") {
      const response = await route.fetch();
      const island = `<script type="application/json" id="tg-config">${JSON.stringify(config).replace(/</g, "\\u003c")}</script>`;
      return route.fulfill({ response, body: (await response.text()).replace("<!--tg-inject-->", island) });
    }
    if (url.pathname.startsWith("/api/")) {
      if (options.signedIn && url.pathname === "/api/session") return route.fulfill({ json: SESSION });
      return route.fulfill(UNANSWERED);
    }
    return route.continue();
  });
}
