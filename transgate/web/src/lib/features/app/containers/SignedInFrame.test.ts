import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import { createMediaPlatform } from "$lib/platform/media";
import { ThemeStore } from "$lib/theme/theme.svelte";
import SignedInFrame from "./SignedInFrame.svelte";

beforeAll(() => i18n.use("en"));

it("composes one navigation set and keeps collapsed navigation usable", async () => {
  const vm = fixture("SignedInVm", "desktop_new");
  const dispatch = vi.fn();
  render(SignedInFrame, { vm, dispatch, media: createMediaPlatform(), theme: new ThemeStore(), previewUrl: null, oncopy: vi.fn().mockResolvedValue(true) });
  const sidebar = within(screen.getByRole("navigation", { name: "Recent transcripts" }));
  expect(sidebar.getAllByRole("button", { name: "New Transcript" })).toHaveLength(1);
  expect(sidebar.getAllByRole("button", { name: "All Transcripts" })).toHaveLength(1);
  expect(sidebar.getAllByRole("heading", { name: "Recent transcripts" })).toHaveLength(1);
  await userEvent.click(sidebar.getByRole("button", { name: "All Transcripts" }));
  expect(dispatch).toHaveBeenLastCalledWith({ type: "navigate", intent: { route: { page: "home", view: { kind: "all_transcripts" } } } });
  await userEvent.click(sidebar.getByRole("button", { name: "Collapse Sidebar" }));
  expect(sidebar.queryByRole("heading", { name: "Recent transcripts" })).toBeNull();
  await userEvent.click(sidebar.getByRole("button", { name: "New Transcript" }));
  expect(dispatch).toHaveBeenLastCalledWith({ type: "navigate", intent: { route: vm.new_transcript_route } });
});

it.each(["desktop", "phone"] as const)("shows the key screen inside the %s frame on its route and forwards its intents", async (layout) => {
  const base = fixture("SignedInVm", layout === "phone" ? "phone_job" : "desktop_new");
  const vm = { ...base, route: { page: "api_keys" as const }, api_keys: fixture("ApiKeysVm", "list") };
  const dispatch = vi.fn();
  render(SignedInFrame, { vm, dispatch, media: createMediaPlatform(), theme: new ThemeStore(), previewUrl: null, oncopy: vi.fn().mockResolvedValue(true) });
  expect(screen.getByRole("heading", { name: "API Keys", level: 1 })).toBeInTheDocument();
  await userEvent.click(screen.getAllByRole("button", { name: "Revoke" })[0]);
  expect(dispatch).toHaveBeenLastCalledWith({ type: "api_keys", intent: { type: "revoke_requested", id: "00000000-0000-0000-0000-000000000001" } });
});
