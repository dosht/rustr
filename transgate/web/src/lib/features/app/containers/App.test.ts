import { render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { TransgateCore } from "$lib/core";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import { createMediaPlatform } from "$lib/platform/media";
import { InvoicePreview } from "$lib/platform/preview.svelte";
import { Telemetry } from "$lib/platform/telemetry.svelte";
import { ThemeStore } from "$lib/theme/theme.svelte";
import App from "./App.svelte";

beforeAll(() => i18n.use("en"));
afterEach(() => vi.unstubAllGlobals());

it("blocks the mounted app in phone landscape until rotation back, not Escape", async () => {
  const orientation = Object.assign(new EventTarget(), { type: "landscape-primary" });
  vi.stubGlobal("screen", { width: 839, height: 412, orientation });
  const vm = fixture("AppVm", "signed_out");
  const core: TransgateCore = { current: () => vm, dispatch: vi.fn(() => vm), subscribe: vi.fn(() => () => {}), dispose: vi.fn() };
  const { container } = render(App, { core, media: createMediaPlatform(), theme: new ThemeStore(),
    telemetry: new Telemetry({ posthog: null, sentry: null }, () => "en"), captcha: null, preview: new InvoicePreview(), copy: vi.fn().mockResolvedValue(true) });
  const notice = await screen.findByRole("dialog", { name: "Please rotate your device" });
  const email = screen.getByLabelText("Email address");
  // jsdom does not reflect HTMLElement.inert to an attribute; test the DOM property.
  const background = container.firstElementChild;
  expect(background).toHaveProperty("inert", true);
  expect(background).toContainElement(email);
  expect(background).not.toContainElement(notice);
  const escape = new Event("cancel", { cancelable: true });
  notice.dispatchEvent(escape);
  expect(escape.defaultPrevented).toBe(true);
  expect(notice).toHaveAttribute("open");
  orientation.type = "portrait-primary";
  orientation.dispatchEvent(new Event("change"));
  await waitFor(() => expect(notice).not.toHaveAttribute("open"));
  expect(background).toHaveProperty("inert", false);
  expect(screen.getByLabelText("Email address")).toBe(email);
});

it.each(["desktop", "phone"] as const)("renders the recovery password form, not the frame, for a signed-in new_password route (%s)", async (layout) => {
  const base = fixture("AppVm", "billing_owner");
  if (base.state !== "signed_in") throw new Error("billing fixture must be signed in");
  const vm = { ...base, route: { page: "new_password" as const }, layouts: { ...base.layouts, frame: layout } };
  const core: TransgateCore = { current: () => vm, dispatch: vi.fn(() => vm), subscribe: vi.fn(() => () => {}), dispose: vi.fn() };
  render(App, { core, media: createMediaPlatform(), theme: new ThemeStore(),
    telemetry: new Telemetry({ posthog: null, sentry: null }, () => "en"), captcha: null, preview: new InvoicePreview(), copy: vi.fn().mockResolvedValue(true) });
  expect(await screen.findByLabelText("New Password")).toHaveAttribute("type", "password");
  expect(screen.queryByRole("navigation")).toBeNull();
});
