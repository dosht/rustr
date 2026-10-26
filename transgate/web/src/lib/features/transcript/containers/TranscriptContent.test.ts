import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { createMediaPlatform } from "$lib/platform/media";
import { i18n } from "$lib/i18n/index.svelte";
import TranscriptContent from "./TranscriptContent.svelte";

beforeAll(() => i18n.use("en"));

it("opens the phone transcript options while the media URL is pending without requiring a store", async () => {
  const vm = fixture("SignedInVm", "phone_job");
  const dispatch = vi.fn();
  const view = render(TranscriptContent, { vm, dispatch, media: createMediaPlatform(), menuOpen: false });
  expect(screen.getByRole("button", { name: "Play" })).toBeDisabled();
  await view.rerender({ menuOpen: true });
  const showTime = screen.getByRole("switch", { name: "Show Time" });
  await userEvent.click(showTime);
  expect(dispatch).toHaveBeenCalledWith({ type: "transcript", intent: { type: "show_time_toggled", on: !vm.transcript!.show_time } });
});
