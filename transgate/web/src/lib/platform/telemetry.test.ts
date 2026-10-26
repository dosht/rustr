import { afterEach, describe, expect, it, vi } from "vitest";
import { CONSENT_COOKIE, readConsent, scrubEvent, Telemetry, type PostHogLike, type SentryLike, type TelemetrySdks } from "./telemetry.svelte";

const config = { posthog: { key: "phc_test", host: "https://ph.example.test" }, sentry: { dsn: "https://k@sentry.example.test/1", environment: "test" } };

function fakes() {
  const posthog = { init: vi.fn(), capture: vi.fn(), opt_in_capturing: vi.fn(), opt_out_capturing: vi.fn(), reset: vi.fn() } satisfies PostHogLike;
  const sentry = { init: vi.fn(), close: vi.fn(async () => true), replayIntegration: vi.fn(() => ({})) } satisfies SentryLike;
  const loads = { posthog: 0, sentry: 0 };
  const sdks: TelemetrySdks = {
    posthog: async () => (loads.posthog++, posthog),
    sentry: async () => (loads.sentry++, sentry),
  };
  return { posthog, sentry, loads, sdks };
}

function clearConsent() {
  document.cookie = `${CONSENT_COOKIE}=; Max-Age=0; Path=/`;
}

afterEach(clearConsent);

describe("consent-gated telemetry", () => {
  it("loads no SDK and drops events before an answer", async () => {
    const f = fakes();
    const telemetry = new Telemetry(config, () => "en", { sdks: f.sdks });
    telemetry.track("transcription_started", { jobs: 1 });
    await Promise.resolve();
    expect(telemetry.undecided).toBe(true);
    expect(f.loads).toEqual({ posthog: 0, sentry: 0 });
  });

  it("starts both SDKs on accept, stores the answer, and delivers events", async () => {
    const f = fakes();
    const telemetry = new Telemetry(config, () => "en", { sdks: f.sdks });
    await telemetry.accept();
    expect(readConsent()).toBe("accepted");
    expect(f.posthog.init).toHaveBeenCalledWith("phc_test", expect.objectContaining({ api_host: "https://ph.example.test", capture_pageview: false }));
    expect(f.posthog.capture.mock.calls.map((c) => c[0])).toEqual(["cookie_consent_accepted", "$pageview"]);
    expect(f.sentry.init).toHaveBeenCalledOnce();
    telemetry.track("transcript_downloaded", { format: "srt" });
    expect(f.posthog.capture).toHaveBeenLastCalledWith("transcript_downloaded", { format: "srt" });
  });

  it("restores an earlier acceptance without a consent event", async () => {
    document.cookie = `${CONSENT_COOKIE}=accepted; Path=/`;
    const f = fakes();
    new Telemetry(config, () => "en", { sdks: f.sdks });
    await vi.waitFor(() => expect(f.sentry.init).toHaveBeenCalled());
    expect(f.posthog.capture.mock.calls.map((c) => c[0])).toEqual(["$pageview"]);
  });

  it("stops capturing and closes Sentry on refusal, and re-accept inits PostHog only once", async () => {
    const f = fakes();
    const telemetry = new Telemetry(config, () => "en", { sdks: f.sdks });
    await telemetry.accept();
    await telemetry.reject();
    expect(readConsent()).toBe("rejected");
    expect(f.posthog.opt_out_capturing).toHaveBeenCalled();
    expect(f.sentry.close).toHaveBeenCalledOnce();
    const before = f.posthog.capture.mock.calls.length;
    telemetry.track("ai_chat_message_sent");
    expect(f.posthog.capture.mock.calls.length).toBe(before);

    await telemetry.accept();
    expect(f.posthog.init).toHaveBeenCalledOnce();
    expect(f.sentry.init).toHaveBeenCalledTimes(2);
  });

  it("a refusal while the SDKs load wins", async () => {
    const f = fakes();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const telemetry = new Telemetry(config, () => "en", {
      sdks: { posthog: async () => (await gate, f.posthog), sentry: f.sdks.sentry },
    });
    const accepting = telemetry.accept();
    await telemetry.reject();
    release();
    await accepting;
    expect(f.posthog.opt_in_capturing).not.toHaveBeenCalled();
    expect(f.sentry.init).not.toHaveBeenCalled();
  });

  it("sign-out reset never loads PostHog, and resets it once it was started", async () => {
    const f = fakes();
    const telemetry = new Telemetry(config, () => "en", { sdks: f.sdks });
    telemetry.reset();
    expect(f.loads.posthog).toBe(0);
    await telemetry.accept();
    telemetry.reset();
    expect(f.posthog.reset).toHaveBeenCalledOnce();
  });

  it("never starts PostHog for the Indonesian UI locale, as the source", async () => {
    const f = fakes();
    await new Telemetry(config, () => "id", { sdks: f.sdks }).accept();
    expect(f.loads.posthog).toBe(0);
    expect(f.sentry.init).toHaveBeenCalledOnce();
  });

  it("drops Sentry events after consent is withdrawn and scrubs the rest", async () => {
    const f = fakes();
    const telemetry = new Telemetry(config, () => "en", { sdks: f.sdks });
    await telemetry.accept();
    const beforeSend = f.sentry.init.mock.calls[0][0].beforeSend as (e: object) => object | null;
    expect(beforeSend({ message: "mail ada@example.test token=abc", extra: { transcript: "secret words" } })).toEqual({
      message: 'mail [EMAIL_REDACTED] "token":"[REDACTED]"',
      extra: { transcript: "[CONTENT_REDACTED]" },
    });
    await telemetry.reject();
    expect(beforeSend({ message: "x" })).toBeNull();
  });

  it("scrubs exception values, breadcrumbs and card numbers", () => {
    const event = scrubEvent({
      exception: { values: [{ value: "card 4111 1111 1111 1111" }] },
      breadcrumbs: [{ message: "password: hunter2", data: { url: "/x?api_key=k1" } }],
    });
    expect(event.exception?.values?.[0].value).toBe("card [CARD_REDACTED]");
    expect(event.breadcrumbs?.[0].message).toBe('"password":"[REDACTED]"');
    expect(event.breadcrumbs?.[0].data?.url).toBe('/x?"api_key":"[REDACTED]"');
  });
});
