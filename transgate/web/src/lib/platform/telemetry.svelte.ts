// Cookie consent and the optional browser telemetry it gates (P5 D12).
// Source: CookieConsent.tsx (cookie "cookieConsent" = accepted|rejected,
// 365 days), PostHogProvider.tsx (init options, manual pageviews, no PostHog
// for the `id` UI locale), sentry.client.config.ts (consent check, scrubbing,
// masked replay). Correction recorded under D12: no SDK script is loaded and
// no request is made before consent (the source loaded both and opted out);
// GA/Hotjar/New Relic are not carried over. Server analytics is separate.

export const CONSENT_COOKIE = "cookieConsent";
const CONSENT_MAX_AGE = 365 * 24 * 60 * 60;

export type Consent = "accepted" | "rejected" | null;
/** Null values are forwarded unchanged, as PostHog records them. */
type Properties = Record<string, string | number | boolean | null>;

export interface TelemetryConfig {
  posthog: { key: string; host: string } | null;
  sentry: { dsn: string; environment: string } | null;
}

/** The two SDKs, injected so tests can observe them without a network. */
export interface TelemetrySdks {
  posthog(): Promise<PostHogLike>;
  sentry(): Promise<SentryLike>;
}

export interface PostHogLike {
  init(key: string, options: Record<string, unknown>): unknown;
  capture(event: string, properties?: Properties): unknown;
  opt_in_capturing(): void;
  opt_out_capturing(): void;
  reset(): void;
}

export interface SentryLike {
  init(options: Record<string, unknown>): unknown;
  close(): Promise<boolean>;
  replayIntegration(options: Record<string, unknown>): unknown;
}

const realSdks: TelemetrySdks = {
  posthog: async () => (await import("posthog-js")).default as unknown as PostHogLike,
  sentry: async () => (await import("@sentry/browser")) as unknown as SentryLike,
};

export function readConsent(doc: Document = document): Consent {
  for (const part of doc.cookie.split(";")) {
    const [name, ...rest] = part.trim().split("=");
    if (name !== CONSENT_COOKIE) continue;
    const value = decodeURIComponent(rest.join("="));
    return value === "accepted" || value === "rejected" ? value : null;
  }
  return null;
}

function writeConsent(value: "accepted" | "rejected", doc: Document): void {
  const secure = doc.location?.protocol === "https:" ? "; Secure" : "";
  doc.cookie = `${CONSENT_COOKIE}=${value}; Max-Age=${CONSENT_MAX_AGE}; Path=/; SameSite=Lax${secure}`;
}

const PRIVACY_PATTERNS: [RegExp, string][] = [
  [/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g, "[EMAIL_REDACTED]"],
  [/["']?password["']?\s*[:=]\s*["']?[^"'\s,}&]+["']?/gi, '"password":"[REDACTED]"'],
  [/["']?(api[_-]?key|token|secret|auth)["']?\s*[:=]\s*["']?[^"'\s,}&]+["']?/gi, '"$1":"[REDACTED]"'],
  [/\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b/g, "[CARD_REDACTED]"],
];
const CONTENT_KEYS = ["transcript", "transcription", "content", "text", "body"];

/** Source scrubbing (sentry.client.config.ts:51-118), applied before any event leaves. */
export function scrub(value: string): string {
  return PRIVACY_PATTERNS.reduce((text, [pattern, replacement]) => text.replace(pattern, replacement), value);
}

interface ScrubbableEvent {
  message?: string;
  exception?: { values?: { value?: string }[] };
  breadcrumbs?: { message?: string; data?: Record<string, unknown> }[];
  extra?: Record<string, unknown>;
}

export function scrubEvent<E extends ScrubbableEvent>(event: E): E {
  if (event.message) event.message = scrub(event.message);
  for (const exception of event.exception?.values ?? []) {
    if (exception.value) exception.value = scrub(exception.value);
  }
  for (const crumb of event.breadcrumbs ?? []) {
    if (crumb.message) crumb.message = scrub(crumb.message);
    for (const [key, value] of Object.entries(crumb.data ?? {})) {
      if (typeof value === "string") crumb.data![key] = scrub(value);
    }
  }
  for (const key of CONTENT_KEYS) {
    if (event.extra && key in event.extra) event.extra[key] = "[CONTENT_REDACTED]";
  }
  return event;
}

export class Telemetry {
  consent: Consent = $state(null);
  /** PostHog while capturing; null before consent and after refusal. */
  #posthog: PostHogLike | null = null;
  /** The PostHog SDK after its one `init`; a later re-accept only opts back in. */
  #posthogLoaded: PostHogLike | null = null;
  #sentry: SentryLike | null = null;
  #starting: Promise<void> | null = null;
  readonly #config: TelemetryConfig;
  readonly #sdks: TelemetrySdks;
  readonly #doc: Document;
  readonly #locale: () => string;

  constructor(config: TelemetryConfig, locale: () => string, options: { sdks?: TelemetrySdks; doc?: Document } = {}) {
    this.#config = config;
    this.#locale = locale;
    this.#sdks = options.sdks ?? realSdks;
    this.#doc = options.doc ?? document;
    this.consent = readConsent(this.#doc);
    if (this.consent === "accepted") void this.#start(false);
  }

  /** Whether the banner must ask (no stored answer). */
  get undecided(): boolean {
    return this.consent === null;
  }

  async accept(): Promise<void> {
    writeConsent("accepted", this.#doc);
    this.consent = "accepted";
    await this.#start(true);
  }

  async reject(): Promise<void> {
    writeConsent("rejected", this.#doc);
    this.consent = "rejected";
    await this.#stop();
  }

  /** Browser event; dropped unless consent was given and PostHog is running. */
  track(event: string, properties: Properties = {}): void {
    if (this.consent !== "accepted") return;
    this.#posthog?.capture(event, properties);
  }

  /** Manual pageview after a route change (source captured pageviews manually). */
  pageview(): void {
    this.track("$pageview");
  }

  /** New anonymous identity after sign-out (IDN-024); never loads an SDK. */
  reset(): void {
    this.#posthogLoaded?.reset();
  }

  #start(justAccepted: boolean): Promise<void> {
    this.#starting ??= (async () => {
      const { posthog, sentry } = this.#config;
      // Source: no PostHog for the Indonesian UI locale (PostHogProvider.tsx:28-31).
      if (posthog && this.#locale() !== "id") {
        let sdk = this.#posthogLoaded;
        if (!sdk) {
          sdk = await this.#sdks.posthog();
          sdk.init(posthog.key, {
            api_host: posthog.host,
            person_profiles: "identified_only",
            capture_pageview: false,
            capture_pageleave: true,
            autocapture: true,
            opt_out_capturing_by_default: true,
          });
          this.#posthogLoaded = sdk;
        }
        if (this.consent !== "accepted") return;
        sdk.opt_in_capturing();
        this.#posthog = sdk;
        if (justAccepted) sdk.capture("cookie_consent_accepted");
        sdk.capture("$pageview");
      }
      if (sentry && this.consent === "accepted" && !this.#sentry) {
        const sdk = await this.#sdks.sentry();
        if (this.consent !== "accepted") return;
        sdk.init({
          dsn: sentry.dsn,
          environment: sentry.environment,
          replaysSessionSampleRate: 0.1,
          replaysOnErrorSampleRate: 1.0,
          integrations: [sdk.replayIntegration({ maskAllText: true, blockAllMedia: true })],
          beforeSend: (event: ScrubbableEvent) => (this.consent === "accepted" ? scrubEvent(event) : null),
        });
        this.#sentry = sdk;
      }
    })().finally(() => {
      this.#starting = null;
      // A refusal that arrived while the SDKs were loading wins.
      if (this.consent !== "accepted") void this.#stop();
    });
    return this.#starting;
  }

  async #stop(): Promise<void> {
    this.#posthogLoaded?.opt_out_capturing();
    this.#posthog = null;
    const sentry = this.#sentry;
    this.#sentry = null;
    await sentry?.close();
  }
}
