import { render, screen, within } from "@testing-library/svelte";
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

describe("phone analysis view", () => {
  it("says partial results are shown when an interrupted analysis kept sections", async () => {
    const { onintent } = analysis("interrupted");
    expect(screen.getByRole("status")).toHaveTextContent("Analysis incomplete. Partial results shown.");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onintent).toHaveBeenCalledWith({ type: "retry" });
  });

  it("copies a lecture section and exports Markdown", async () => {
    const { onintent } = analysis("lecture");
    await userEvent.click(within(screen.getByTestId("topics-list")).getByRole("button", { name: "Copy to clipboard" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "copy_section", section: "topics" });
    await userEvent.click(screen.getByRole("button", { name: "Export" }));
    await userEvent.click(screen.getByRole("button", { name: /Export as Markdown/ }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "export", format: "md" });
  });

  it("explains an empty transcript instead of starting", () => {
    analysis("no_transcript");
    expect(screen.getByText("No transcript content available")).toBeVisible();
  });
});

describe("phone chat view", () => {
  it("starts chatting from the introduction", async () => {
    const onconsent = vi.fn();
    render(ChatPanel, { consent: fixture("ConsentPanelVm", "required"), view: fixture("ChatPanelVm", "welcome"), onintent: vi.fn(), onconsent });
    await userEvent.click(screen.getByRole("button", { name: /Start Chatting/ }));
    expect(onconsent).toHaveBeenCalledWith({ type: "enable" });
  });

  it("invites a first question when there is no history", () => {
    chat("welcome");
    expect(screen.getByText("Start a conversation")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Copy chat" })).toBeNull();
  });

  it("sends the draft and clears it", async () => {
    const { onintent } = chat("history", "And the venue?");
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(onintent).toHaveBeenCalledWith({ type: "send", text: "And the venue?" });
    expect(screen.getByRole("textbox")).toHaveValue("");
  });

  it("locks the input while a refusal is shown, as the source did", () => {
    chat("limit_reached", "One more");
    expect(screen.getByRole("textbox")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("always shows answer actions on phones and copies the whole chat", async () => {
    const { onintent } = chat("history");
    const toolbar = screen.getByRole("toolbar", { name: "Message actions" });
    await userEvent.click(within(toolbar).getByRole("button", { name: "Helpful" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "rate", index: 1, rating: "like" });
    await userEvent.click(screen.getByRole("button", { name: "Copy chat" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "copy_chat" });
  });
});
