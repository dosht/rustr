import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { FeedbackVm } from "$lib/core/types/FeedbackVm";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopFeedback from "./DesktopFeedback.svelte";

const label = (key: string) => ({ ns: "common", key, args: {} }) as FeedbackVm["options"][number]["label"];

function vm(patch: Partial<FeedbackVm> = {}): FeedbackVm {
  return {
    open: true,
    thanked: false,
    options: [
      { rating: 0, label: label("Disappointed"), selected: false },
      { rating: 5, label: label("Satisfied"), selected: true },
      { rating: 10, label: label("Outstanding"), selected: false },
    ],
    comment: "",
    submitting: false,
    can_submit: true,
    notice: null,
    ...patch,
  };
}

function setup(model: FeedbackVm) {
  const intents = { onselect: vi.fn(), oncomment: vi.fn(), onsubmit: vi.fn(), onclose: vi.fn() };
  render(DesktopFeedback, { vm: model, ...intents });
  return intents;
}

beforeAll(() => i18n.use("en"));

describe("DesktopFeedback", () => {
  it("offers the three ratings as one named radio group and reports the chosen value", async () => {
    const intents = setup(vm());
    expect(screen.getByRole("dialog", { name: "Rate Your Experience" })).toBeVisible();
    expect(screen.getByRole("group", { name: "How was your experience?" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Satisfied" })).toBeChecked();
    await userEvent.click(screen.getByRole("radio", { name: "Outstanding" }));
    expect(intents.onselect).toHaveBeenCalledWith(10);
  });

  it("submits the form and reports typed comments", async () => {
    const intents = setup(vm());
    await userEvent.type(screen.getByRole("textbox", { name: "Tell us more about your experience" }), "x");
    expect(intents.oncomment).toHaveBeenCalledWith("x");
    await userEvent.click(screen.getByRole("button", { name: "Submit" }));
    expect(intents.onsubmit).toHaveBeenCalledOnce();
  });

  it("while submitting: busy, inputs locked, no second submit", async () => {
    const intents = setup(vm({ submitting: true, can_submit: false }));
    const busy = screen.getByRole("button", { name: "Submitting..." });
    expect(busy).toHaveAttribute("aria-busy", "true");
    await userEvent.click(busy);
    expect(intents.onsubmit).not.toHaveBeenCalled();
    expect(screen.getByRole("radio", { name: "Disappointed" })).toBeDisabled();
  });

  it("shows core's notice as an alert instead of the source's alert()", () => {
    setup(vm({ notice: label("An error occurred while submitting your feedback. Please try again.") }));
    expect(screen.getByRole("alert")).toHaveTextContent("An error occurred while submitting your feedback. Please try again.");
  });

  it("after a stored submission shows the thanks and only a Close action", async () => {
    const intents = setup(vm({ thanked: true }));
    expect(screen.getByRole("dialog", { name: "Thank You!" })).toHaveTextContent("Your response helps us improve our service.");
    expect(screen.queryByRole("button", { name: "Submit" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(intents.onclose).toHaveBeenCalledOnce();
  });
});
