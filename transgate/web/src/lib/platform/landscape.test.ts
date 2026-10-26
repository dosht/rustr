import { describe, expect, it } from "vitest";
import { isPhoneLandscape } from "./landscape";

function fakeWindow(options: { type?: string; legacy?: number; width?: number; height?: number; fine?: boolean }): Window {
  return {
    screen: {
      width: options.width,
      height: options.height,
      orientation: options.type === undefined ? undefined : { type: options.type },
    },
    orientation: options.legacy,
    matchMedia: () => ({ matches: options.fine ?? false }),
  } as unknown as Window;
}

describe("phone landscape (source useIsPhoneLandscape)", () => {
  it("is true only for a touch phone rotated to landscape", () => {
    expect(isPhoneLandscape(fakeWindow({ type: "landscape-primary", width: 915, height: 412 }))).toBe(true);
    expect(isPhoneLandscape(fakeWindow({ type: "portrait-primary", width: 412, height: 915 }))).toBe(false);
  });

  it("exempts tablets and fine pointers", () => {
    expect(isPhoneLandscape(fakeWindow({ type: "landscape-primary", width: 1180, height: 820 }))).toBe(false);
    expect(isPhoneLandscape(fakeWindow({ type: "landscape-primary", width: 915, height: 412, fine: true }))).toBe(false);
  });

  it("reads legacy iOS window.orientation degrees", () => {
    expect(isPhoneLandscape(fakeWindow({ legacy: -90, width: 390, height: 844 }))).toBe(true);
    expect(isPhoneLandscape(fakeWindow({ legacy: 0, width: 390, height: 844 }))).toBe(false);
  });

  it("fails closed when nothing can be read", () => {
    expect(isPhoneLandscape(fakeWindow({}))).toBe(false);
  });
});
