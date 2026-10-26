import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { summaryScreen } from "../../../core/fixtures";
import AudioSummaryDesktop from "./AudioSummaryDesktop.svelte";

describe("AudioSummaryDesktop", () => {
  it("requires disclosure acknowledgement before sending a transcript", async () => {
    render(AudioSummaryDesktop, { view: summaryScreen("ready"), onIntent: vi.fn() });
    expect(screen.getByRole("button", { name: "Generate summary" })).toBeDisabled();
    await userEvent.click(screen.getByRole("checkbox", { name: "I agree to external AI processing for this summary" }));
    expect(screen.getByRole("button", { name: "Generate summary" })).toBeEnabled();
    expect(screen.getByText(/configured external AI provider/)).toBeVisible();
  });
  it("renders untrusted provisional content as text, not HTML", () => {
    const { container } = render(AudioSummaryDesktop, { view: summaryScreen("provisional"), onIntent: vi.fn() });
    expect(screen.getByLabelText("Summary text")).toHaveTextContent("<b>Provisional</b>");
    expect(container.querySelector("b")).toBeNull();
    expect(screen.queryByText("Summary saved.")).toBeNull();
  });
  it("offers resuming rather than regeneration while detached", () => {
    render(AudioSummaryDesktop, { view: summaryScreen("detached"), onIntent: vi.fn() });
    expect(screen.getByRole("button", { name: "Resume updates" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Generate summary" })).toBeNull();
  });
});
