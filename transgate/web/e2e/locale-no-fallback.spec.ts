// LNG-005 (transgate/docs/rules.md), as changed by P5 D3: there is no fallback
// language. Every label a visitor reaches on sign-in and in the signed-in app is
// the locale's own catalog text, and a key missing from a catalog is an error
// (logged, the key shown) rather than the English sentence.
import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { shell } from "./support/shell";

const LOCALES = new URL("../locales/", import.meta.url);
const config = JSON.parse(readFileSync(new URL("config.json", LOCALES), "utf8")) as { locales: string[]; namespaces: string[] };
const catalog = (locale: string, ns: string): Record<string, string> =>
  JSON.parse(readFileSync(new URL(`${locale}/${ns}.json`, LOCALES), "utf8"));

/** English sentences a locale translates differently; showing one is a fallback. */
function englishOnly(locale: string): Set<string> {
  const english = new Set<string>();
  for (const ns of config.namespaces) {
    const en = catalog("en", ns);
    const own = catalog(locale, ns);
    for (const [key, text] of Object.entries(en)) {
      if (own[key] !== undefined && own[key] !== text && /\p{L}{3}/u.test(text) && !text.includes("{{")) english.add(text.trim());
    }
  }
  return english;
}

async function shown(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const lines = document.body.innerText.split("\n").map((line) => line.trim());
    const attributes = [...document.querySelectorAll("[aria-label],[placeholder],[title]")].flatMap((element) =>
      ["aria-label", "placeholder", "title"].map((name) => element.getAttribute(name)?.trim() ?? ""),
    );
    return [...lines, ...attributes].filter((text) => text !== "");
  });
}

const NON_ENGLISH = config.locales.filter((locale) => locale !== "en");

test.describe("locale without fallback", () => {
  test("every sign-in label in each non-English locale is that locale's own text", async ({ context, page }) => {
    await shell(context, { consent: "rejected" });
    const fallbacks: string[] = [];
    for (const locale of NON_ENGLISH) {
      await page.goto(`/${locale}/signin`);
      await expect(page.locator('input[type="email"]')).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("lang", locale);
      const english = englishOnly(locale);
      fallbacks.push(...(await shown(page)).filter((text) => english.has(text)).map((text) => `${locale}: ${text}`));
    }
    expect(fallbacks).toEqual([]);
  });

  test("every signed-in home label in each non-English locale is that locale's own text", async ({ context, page }) => {
    await shell(context, { signedIn: true, consent: "rejected" });
    const fallbacks: string[] = [];
    for (const locale of NON_ENGLISH) {
      await page.goto(`/${locale}/home`);
      await expect(page.locator('input[type="file"]').first()).toBeAttached();
      await expect(page.locator("html")).toHaveAttribute("lang", locale);
      const english = englishOnly(locale);
      fallbacks.push(...(await shown(page)).filter((text) => english.has(text)).map((text) => `${locale}: ${text}`));
    }
    expect(fallbacks).toEqual([]);
  });

  test("a key missing from a catalog is reported and shown as the key, never as English", async ({ context, page }) => {
    const turkish = catalog("tr", "auth");
    const label = turkish["sign_in.button_label"];
    const english = catalog("en", "auth")["sign_in.button_label"];
    await shell(context, {
      consent: "rejected",
      handlers: [(request, url, route) => {
        if (!/^\/_app\/auth-[^/]+\.js$/.test(url.pathname)) return undefined;
        return (async () => {
          const response = await route.fetch();
          const source = await response.text();
          if (!source.includes(turkish["forgotten_password.link_text"])) return route.fulfill({ response, body: source });
          const removed = source.replace(/"sign_in\.button_label":(`[^`]*`|"(?:[^"\\]|\\.)*"),?/, "");
          expect(removed).not.toBe(source);
          return route.fulfill({ response, body: removed });
        })();
      }],
    });
    const errors: string[] = [];
    page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto("/tr/signin");
    await expect(page.locator('input[type="email"]')).toBeVisible();
    await expect(page.getByRole("button", { name: "sign_in.button_label", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: english, exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: label, exact: true })).toHaveCount(0);
    expect(errors.some((text) => text.includes('missing translation tr/auth: "sign_in.button_label"'))).toBe(true);
  });
});
