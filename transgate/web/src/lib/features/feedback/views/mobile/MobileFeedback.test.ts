import { render, screen } from "@testing-library/svelte";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { FeedbackVm } from "$lib/core/types/FeedbackVm";
import { i18n } from "$lib/i18n/index.svelte";
import MobileFeedback from "./MobileFeedback.svelte";

const label = (key: string) => ({ ns: "common", key, args: {} }) as FeedbackVm["options"][number]["label"];

beforeAll(() => i18n.use("en"));

describe("MobileFeedback", () => {
  it("Escape asks to close the sheet, unless a submission is in flight", async () => {
    const vm: FeedbackVm = {
      open: true,
      thanked: false,
      options: [{ rating: 0, label: label("Disappointed"), selected: false }],
      comment: "",
      submitting: false,
      can_submit: true,
      notice: null,
    };
    const intents = { onselect: vi.fn(), oncomment: vi.fn(), onsubmit: vi.fn(), onclose: vi.fn() };
    const { rerender } = render(MobileFeedback, { vm, ...intents });
    const sheet = screen.getByRole("dialog", { name: "Rate Your Experience" });
    sheet.dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(intents.onclose).toHaveBeenCalledOnce();

    await rerender({ vm: { ...vm, submitting: true }, ...intents });
    sheet.dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(intents.onclose).toHaveBeenCalledOnce();
  });
});
