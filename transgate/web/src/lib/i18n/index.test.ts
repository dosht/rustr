import { describe, expect, it } from "vitest";
import config from "../../../locales/config.json";
import { Translator } from "./index.svelte";

async function translator(catalogs: Record<string, Record<string, Record<string, string>>>, locale = "en") {
  const t = new Translator(true);
  for (const [lng, namespaces] of Object.entries(catalogs)) {
    for (const ns of config.namespaces) t.add(lng, ns, namespaces[ns] ?? {});
  }
  await t.use(locale);
  return t;
}

describe("Translator", () => {
  it("keeps English-sentence keys containing dots and colons literal", async () => {
    const t = await translator({ en: { common: { "Email: ": "Email: ", "Checkout failed. Try again.": "Checkout failed. Try again." } } });
    expect(t.tk("common", "Email: ")).toBe("Email: ");
    expect(t.tk("common", "Checkout failed. Try again.")).toBe("Checkout failed. Try again.");
  });

  it("selects source _one/_other plurals by the numeric count", async () => {
    const t = await translator({
      en: { common: { redirectMessage_one: "in {{count}} second.", redirectMessage_other: "in {{count}} seconds." } },
    });
    expect(t.t({ ns: "common", key: "redirectMessage", args: { count: 1 } })).toBe("in 1 second.");
    expect(t.t({ ns: "common", key: "redirectMessage", args: { count: 5 } })).toBe("in 5 seconds.");
  });

  it("translates a nested message argument before interpolating it", async () => {
    const t = await translator({
      en: { app: { "Status: {{status}}": "Status: {{status}}", Completed: "Completed" } },
    });
    expect(t.t({ ns: "app", key: "Status: {{status}}", args: { status: { ns: "app", key: "Completed" } } })).toBe("Status: Completed");
  });

  it("never falls back to English for another locale", async () => {
    const t = await translator({ en: { common: { Yes: "Yes" } }, tr: { common: {} } }, "tr");
    expect(() => t.tk("common", "Yes")).toThrow(/missing translation tr\/common/);
  });

  it("does not escape values; Svelte escapes at render", async () => {
    const t = await translator({ en: { common: { "Hi {{name}}": "Hi {{name}}" } } });
    expect(t.tk("common", "Hi {{name}}", { name: "<b>&" })).toBe("Hi <b>&");
  });

  it("refuses a locale outside the sixteen", async () => {
    const t = new Translator(true);
    await expect(t.use("xx")).rejects.toThrow(/unsupported UI locale/);
  });

  it.each(config.locales)("loads every real namespace for %s through the production glob", async (locale) => {
    const t = new Translator(true);
    await t.use(locale);
    expect(t.locale).toBe(locale);
    const label = t.tk("auth", "sign_in.button_label");
    if (locale === "en") expect(label).toBe("Sign in");
    else {
      expect(label).not.toBe("Sign in");
      expect(label).not.toBe("sign_in.button_label");
    }
  });
});
