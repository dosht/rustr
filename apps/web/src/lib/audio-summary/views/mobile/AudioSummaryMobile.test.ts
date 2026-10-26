import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { summaryScreen } from "../../../core/fixtures";
import AudioSummaryMobile from "./AudioSummaryMobile.svelte";

describe("AudioSummaryMobile", () => {
  it("requires a per-action external-processing acknowledgement", async () => {
    render(AudioSummaryMobile, { view: summaryScreen("ready"), onIntent: vi.fn() });
    const generate = screen.getByRole("button", { name: "Generate summary" });
    expect(generate).toBeDisabled();
    await userEvent.click(screen.getByRole("checkbox", { name: "I agree to external AI processing for this summary" }));
    expect(generate).toBeEnabled();
  });
  it("keeps interruption and remote uncertainty visible with retained partial text", () => {
    render(AudioSummaryMobile, { view: summaryScreen("interrupted"), onIntent: vi.fn() });
    expect(screen.getByLabelText("Summary text")).toHaveTextContent("Retained partial");
    expect(screen.getByRole("alert")).toHaveTextContent("Connection interrupted");
    expect(screen.getByText(/does not confirm remote cancellation/)).toBeVisible();
  });
  it("shows the authoritative saved replacement rather than a generating indicator", () => {
    render(AudioSummaryMobile, { view: summaryScreen("saved"), onIntent: vi.fn() });
    expect(screen.getByLabelText("Summary text")).toHaveTextContent("Authoritative saved summary.");
    expect(screen.getByRole("status")).toHaveTextContent("Summary saved.");
    expect(screen.queryByRole("button", { name: "Stop generation" })).toBeNull();
  });
});
