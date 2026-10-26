import { expect, it, vi } from "vitest";
import { Translator } from "./index.svelte";

const loads = vi.hoisted(() => {
  function deferred() {
    let resolve!: () => void;
    const promise = new Promise<void>((done) => { resolve = done; });
    return { promise, resolve };
  }
  return { older: deferred(), latest: deferred(), olderStarted: deferred(), latestStarted: deferred() };
});

// Delay only delivery; the real glob, JSON catalogs and translator still run.
vi.mock("../../../locales/tr/common.json", async (original) => {
  loads.olderStarted.resolve();
  await loads.older.promise;
  return original();
});
vi.mock("../../../locales/de/common.json", async (original) => {
  loads.latestStarted.resolve();
  await loads.latest.promise;
  return original();
});

it("keeps the latest requested locale when an earlier real catalog load completes last", async () => {
  const translator = new Translator(true);
  await translator.use("en");
  const older = translator.use("tr");
  const publications = vi.spyOn(translator, "locale", "set");
  await loads.olderStarted.promise;
  const latest = translator.use("de");
  await loads.latestStarted.promise;
  expect(translator.locale).toBe("en");
  expect(translator.tk("auth", "sign_in.button_label")).toBe("Sign in");

  loads.latest.resolve();
  await latest;
  expect(translator.locale).toBe("de");
  expect(translator.tk("auth", "sign_in.button_label")).toBe("Anmelden");

  loads.older.resolve();
  await older;
  expect(translator.locale).toBe("de");
  expect(translator.tk("auth", "sign_in.button_label")).toBe("Anmelden");
  // Observe every published transition, not just the final value: no stale flash.
  expect(publications.mock.calls.map(([locale]) => locale)).toEqual(["de"]);
  publications.mockRestore();
});
