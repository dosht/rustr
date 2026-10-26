// The host-injected public runtime configuration (web-shell.md section 7).
// One image serves every deployment, so nothing here is baked into the build,
// except in `vite dev`, where there is no host and VITE_TG_* values stand in.
// An invalid configuration is an error page, never a guessed default.

export interface RuntimeConfig {
  apiUrl: string;
  authUrl: string;
  authKey: string | null;
  siteUrl: string;
  captcha: { provider: "turnstile"; siteKey: string } | null;
  telemetry: {
    posthog: { key: string; host: string } | null;
    sentry: { dsn: string; environment: string } | null;
  };
}

export class ConfigError extends Error {
  constructor(reason: string) {
    super(`invalid runtime configuration: ${reason}`);
    this.name = "ConfigError";
  }
}

type Json = Record<string, unknown>;

function object(value: unknown, field: string): Json {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new ConfigError(`${field} must be an object`);
  return value as Json;
}

function text(value: unknown, field: string): string {
  if (typeof value !== "string" || value.trim() === "") throw new ConfigError(`${field} must be a non-empty string`);
  return value;
}

function optionalText(value: unknown, field: string): string | null {
  return value === null || value === undefined ? null : text(value, field);
}

function optionalObject<T>(value: unknown, field: string, parse: (json: Json) => T): T | null {
  return value === null || value === undefined ? null : parse(object(value, field));
}

export function parseConfig(value: unknown): RuntimeConfig {
  const json = object(value, "tg-config");
  const telemetry = object(json.telemetry ?? {}, "telemetry");
  return {
    apiUrl: text(json.apiUrl, "apiUrl"),
    authUrl: text(json.authUrl, "authUrl"),
    authKey: optionalText(json.authKey, "authKey"),
    siteUrl: text(json.siteUrl, "siteUrl"),
    captcha: optionalObject(json.captcha, "captcha", (c) => {
      if (c.provider !== "turnstile") throw new ConfigError("captcha.provider must be turnstile");
      return { provider: "turnstile", siteKey: text(c.siteKey, "captcha.siteKey") };
    }),
    telemetry: {
      posthog: optionalObject(telemetry.posthog, "telemetry.posthog", (p) => ({
        key: text(p.key, "telemetry.posthog.key"),
        host: text(p.host, "telemetry.posthog.host"),
      })),
      sentry: optionalObject(telemetry.sentry, "telemetry.sentry", (s) => ({
        dsn: text(s.dsn, "telemetry.sentry.dsn"),
        environment: text(s.environment, "telemetry.sentry.environment"),
      })),
    },
  };
}

/** Reads one host-injected JSON island (`tg-config`, `tg-confirm`). */
export function readIsland(id: string, doc: Document = document): unknown {
  const element = doc.getElementById(id);
  if (!(element instanceof HTMLScriptElement) || element.type !== "application/json") return undefined;
  try {
    return JSON.parse(element.textContent ?? "");
  } catch {
    throw new ConfigError(`${id} is not JSON`);
  }
}

function devConfig(env: ImportMetaEnv): unknown {
  return {
    apiUrl: env.VITE_TG_API_URL ?? "/api",
    authUrl: env.VITE_TG_AUTH_URL ?? "/auth/v1",
    authKey: env.VITE_TG_AUTH_KEY ?? null,
    siteUrl: env.VITE_TG_SITE_URL ?? window.location.origin,
    captcha: env.VITE_TG_TURNSTILE_SITE_KEY ? { provider: "turnstile", siteKey: env.VITE_TG_TURNSTILE_SITE_KEY } : null,
    telemetry: { posthog: null, sentry: null },
  };
}

/** The page's configuration; throws `ConfigError` when the host supplied none or an invalid one. */
export function loadConfig(doc: Document = document, env: ImportMetaEnv = import.meta.env): RuntimeConfig {
  const island = readIsland("tg-config", doc);
  if (island === undefined) {
    if (env.DEV) return parseConfig(devConfig(env));
    throw new ConfigError("tg-config is missing");
  }
  return parseConfig(island);
}
