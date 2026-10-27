import { cleanup, render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import AnalysisPanel from "./AnalysisPanel.svelte";
import ChatPanel from "./ChatPanel.svelte";

beforeAll(() => i18n.use("en"));

const granted = () => fixture("ConsentPanelVm", "granted");

function analysis(name: string, consent = granted()) {
  const onintent = vi.fn();
  const onconsent = vi.fn();
  render(AnalysisPanel, { consent, view: fixture("AnalysisPanelVm", name), onintent, onconsent });
  return { onintent, onconsent };
}

function chat(name: string, draft = "") {
  const onintent = vi.fn();
  render(ChatPanel, { consent: granted(), view: fixture("ChatPanelVm", name), draft, onintent, onconsent: vi.fn() });
  return { onintent };
}

describe("desktop analysis panel", () => {
  it("asks for consent from the introduction and its learn-more modal, disclosing provider processing without unsupported promises", async () => {
    const { onconsent, onintent } = analysis("meeting", fixture("ConsentPanelVm", "required"));
    expect(screen.queryByTestId("analysis-panel")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /Start Summarizing/ }));
    expect(onconsent).toHaveBeenLastCalledWith({ type: "enable" });

    await userEvent.click(screen.getByRole("button", { name: "Learn More" }));
    const modal = screen.getByRole("dialog", { name: "AI Features Consent" });
    expect(modal).toHaveTextContent(i18n.tk("app", "p5.consent.provider_processing", { provider: "deepseek" }));
    expect(modal).toHaveTextContent(i18n.tk("app", "p5.consent.provider_retention"));
    expect(modal).toHaveTextContent(i18n.tk("app", "p5.consent.decline_keeps"));
    expect(modal).not.toHaveTextContent(/not permanently stored|cached locally/);
    await userEvent.click(within(modal).getByRole("button", { name: "Decline" }));
    expect(onconsent).toHaveBeenLastCalledWith({ type: "decline" });
    expect(onintent).not.toHaveBeenCalled();
  });

  it("copies one section or the whole analysis and exports the chosen format", async () => {
    const { onintent } = analysis("meeting");
    const actions = screen.getByTestId("action-items-list");
    await userEvent.click(within(actions).getByRole("button", { name: "Copy to clipboard" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "copy_section", section: "action_items" });

    const header = screen.getAllByRole("button", { name: "Copy to clipboard" })[0];
    await userEvent.click(header);
    expect(onintent).toHaveBeenLastCalledWith({ type: "copy" });

    await userEvent.click(screen.getByRole("button", { name: "Export" }));
    await userEvent.click(screen.getByRole("button", { name: /Export as PDF/ }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "export", format: "pdf" });
    expect(screen.queryByRole("button", { name: /Export as PDF/ })).toBeNull();
  });

  it("operates the export disclosure from the keyboard and closes it with Escape or an outside click", async () => {
    const { onintent } = analysis("meeting");
    const trigger = screen.getByRole("button", { name: "Export" });
    trigger.focus();
    await userEvent.keyboard("{Enter}");
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: /Export as Word/ })).toHaveAccessibleDescription("Microsoft Word format");

    await userEvent.tab();
    expect(screen.getByRole("button", { name: /Export as Word/ })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole("button", { name: /Export as Word/ })).toBeNull();

    await userEvent.keyboard("{Enter}");
    await userEvent.click(document.body);
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    trigger.focus();
    await userEvent.keyboard("{Enter}");
    await userEvent.tab();
    await userEvent.keyboard("{Enter}");
    expect(onintent).toHaveBeenLastCalledWith({ type: "export", format: "docx" });
  });

  it.each(["generic", "meeting", "lecture", "sales", "support", "interview"])("renders the %s shape with its type badge", (name) => {
    analysis(name);
    if (name === "generic") expect(screen.queryByTestId("analysis-type")).toBeNull();
    else expect(screen.getByTestId("analysis-type")).toBeVisible();
    expect(screen.getByTestId(`${name}-analysis`)).toBeVisible();
  });

  it("keeps streamed sections on screen under the progress stage", () => {
    analysis("streaming");
    expect(screen.getByTestId("action-items-list")).toBeVisible();
    expect(screen.getByTestId("analysis-progress")).toHaveTextContent("Analyzing (2/6 sections)");
  });

  it("shows interrupted partials with retry", async () => {
    const { onintent } = analysis("interrupted");
    expect(screen.getByText("Analysis was interrupted.")).toBeVisible();
    expect(screen.getByTestId("action-items-list")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onintent).toHaveBeenCalledWith({ type: "retry" });
  });

  it("shows the server's refusal text, or the unconfirmed hint, under the failure", () => {
    analysis("rejected");
    expect(screen.getByRole("alert")).toHaveTextContent("AI analysis is temporarily unavailable.");
    cleanup();
    analysis("unconfirmed");
    expect(screen.getByRole("alert")).toHaveTextContent("The AI request could not be confirmed. Check your connection and retry.");
  });

  it("reports an export failure until dismissed", async () => {
    const { onintent } = analysis("export_failed");
    const alert = screen.getByRole("alert");
    await userEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(onintent).toHaveBeenCalledWith({ type: "dismiss_export_error" });
  });

  it("disables the export trigger while a document is being written", () => {
    analysis("exporting");
    expect(screen.getByRole("button", { name: "Export" })).toBeDisabled();
  });
});

describe("desktop chat panel", () => {
  it("sends the typed question on Enter, keeps Shift+Enter as a newline and clears the draft", async () => {
    const { onintent } = chat("history", "Who owns it?");
    const box = screen.getByRole("textbox", { name: "Ask about the transcript..." });
    await userEvent.type(box, "{Shift>}{Enter}{/Shift}");
    expect(onintent).not.toHaveBeenCalled();
    await userEvent.type(box, "{Enter}");
    expect(onintent).toHaveBeenCalledWith({ type: "send", text: "Who owns it?\n" });
    expect(box).toHaveValue("");
  });

  it("does not send while an answer is pending", async () => {
    const { onintent } = chat("typing", "Again?");
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox"), "{Enter}");
    expect(onintent).not.toHaveBeenCalled();
  });

  it("acts on a finished answer by its displayed index", async () => {
    const { onintent } = chat("history");
    const toolbar = screen.getByRole("toolbar", { name: "Message actions" });
    await userEvent.click(within(toolbar).getByRole("button", { name: /Retry/ }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "retry", index: 1 });
    await userEvent.click(within(toolbar).getByRole("button", { name: "Not helpful" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "rate", index: 1, rating: "dislike" });
    await userEvent.click(within(toolbar).getByRole("button", { name: /Copy/ }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "copy_message", index: 1 });
  });

  it("renders finished answers as escaped markdown", () => {
    chat("history");
    const answer = screen.getByTestId("chat-message-assistant");
    expect(within(answer).getByText("1 May").tagName).toBe("STRONG");
  });

  it("shows the usage limits with their reset", () => {
    chat("history");
    const limits = screen.getByTestId("chat-limits");
    expect(limits).toHaveTextContent("8 / 10");
    expect(limits).toHaveTextContent("Resets in 5 hours");
  });

  it("shows a limit refusal, keeps the box usable as core reports, and dismisses it", async () => {
    const { onintent } = chat("limit_reached");
    expect(screen.getByRole("textbox")).toBeEnabled();
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Failed to get response");
    await userEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(onintent).toHaveBeenCalledWith({ type: "dismiss_error" });
  });

  it("warns that an unsaved answer may disappear", () => {
    chat("sync_warning");
    expect(screen.getByText("This answer could not be saved. It may disappear after refresh.")).toBeVisible();
  });

  it("cannot be used while the host reports chat disabled", () => {
    chat("disabled", "Hello");
    expect(screen.getByRole("textbox")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });
});
