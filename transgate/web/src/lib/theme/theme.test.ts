import { afterEach, describe, expect, it } from "vitest";
import { THEME_KEY, ThemeStore } from "./theme.svelte";

afterEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("theme", () => {
  it("defaults to system and leaves the attribute to the media query", () => {
    const theme = new ThemeStore();
    expect(theme.choice).toBe("system");
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("restores a stored choice and ignores an unknown one", () => {
    localStorage.setItem(THEME_KEY, "dark");
    expect(new ThemeStore().choice).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");

    localStorage.setItem(THEME_KEY, "sepia");
    expect(new ThemeStore().choice).toBe("system");
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("persists a change and moves back to system", () => {
    const theme = new ThemeStore();
    theme.set("light");
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    theme.set("system");
    expect(localStorage.getItem(THEME_KEY)).toBe("system");
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});
