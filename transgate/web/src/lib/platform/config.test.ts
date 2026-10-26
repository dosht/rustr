import { afterEach, describe, expect, it } from "vitest";
import { readConfirmOutcome } from "$lib/features/commerce/confirm";
import { ConfigError, loadConfig, parseConfig } from "./config";

const valid = {
  apiUrl: "/api",
  authUrl: "/auth/v1",
  authKey: null,
  siteUrl: "https://app.example.test",
  captcha: null,
  telemetry: { posthog: null, sentry: { dsn: "https://k@s.example.test/1", environment: "staging" } },
};

function island(id: string, text: string) {
  const script = document.createElement("script");
  script.type = "application/json";
  script.id = id;
  script.textContent = text;
  document.head.append(script);
}

afterEach(() => document.head.replaceChildren());

describe("runtime config", () => {
  it("parses the host island", () => {
    island("tg-config", JSON.stringify(valid));
    expect(loadConfig(document, { DEV: false } as ImportMetaEnv)).toEqual(valid);
  });

  it("refuses a production page without config instead of guessing", () => {
    expect(() => loadConfig(document, { DEV: false } as ImportMetaEnv)).toThrow(ConfigError);
  });

  it("uses VITE_TG_* only in vite dev", () => {
    const config = loadConfig(document, { DEV: true, VITE_TG_AUTH_KEY: "anon" } as unknown as ImportMetaEnv);
    expect(config).toMatchObject({ apiUrl: "/api", authUrl: "/auth/v1", authKey: "anon", telemetry: { posthog: null, sentry: null } });
  });

  it.each([
    ["missing apiUrl", { ...valid, apiUrl: undefined }],
    ["empty siteUrl", { ...valid, siteUrl: " " }],
    ["unknown captcha", { ...valid, captcha: { provider: "hcaptcha", siteKey: "k" } }],
    ["half telemetry", { ...valid, telemetry: { posthog: { key: "k" }, sentry: null } }],
  ])("rejects %s", (_name, value) => {
    expect(() => parseConfig(value)).toThrow(ConfigError);
  });

  it("rejects a non-JSON island", () => {
    island("tg-config", "{not json");
    expect(() => loadConfig(document, { DEV: true } as ImportMetaEnv)).toThrow(ConfigError);
  });
});

describe("checkout confirm island", () => {
  it("is absent on ordinary documents", () => {
    expect(readConfirmOutcome()).toBeUndefined();
  });

  it("passes a confirmed envelope through unchanged, pending included", () => {
    const view = { payment_id: 7, status: "pending", redirect_after_seconds: 5 };
    island("tg-confirm", JSON.stringify({ outcome: "confirmed", view }));
    expect(readConfirmOutcome()).toEqual({ outcome: "confirmed", view });
  });

  it("keeps a failure's status, code and message", () => {
    island("tg-confirm", JSON.stringify({ outcome: "failed", status: 504, code: "provider_uncertain", message: "m" }));
    expect(readConfirmOutcome()).toEqual({ outcome: "failed", status: 504, code: "provider_uncertain", message: "m" });
  });

  it("refuses a raw ConfirmView", () => {
    island("tg-confirm", JSON.stringify({ payment_id: 7, status: "completed" }));
    expect(() => readConfirmOutcome()).toThrow(ConfigError);
  });
});
