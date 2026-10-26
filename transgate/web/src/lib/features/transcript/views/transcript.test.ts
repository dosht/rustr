import { render, screen, within, fireEvent } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopTranscript from "./desktop/DesktopTranscript.svelte";
import MobileTranscript from "./mobile/MobileTranscript.svelte";
import type { ComponentProps } from "svelte";
import { speakerText } from "./shared/speaker";
beforeAll(() => i18n.use("en"));
for (const name of ["desktop", "phone"] as const) {
  function renderTranscript(props: ComponentProps<typeof DesktopTranscript>) {
    return name === "desktop" ? render(DesktopTranscript, props) : render(MobileTranscript, props);
  }
  describe(`${name} transcript`, () => {
    it("preserves the source text on edit, ignores IME Enter, then submits the new text", async () => {
      const onIntent = vi.fn(), vm = fixture("TranscriptVm", "editing");
      renderTranscript({ vm, activeIndex: -1, onIntent, onupgrade: vi.fn(), onfeedback: vi.fn(), onanalysisvisible: vi.fn() });
      const editor = screen.getByRole("textbox", { name: "Edit line" });
      expect(editor).toHaveValue(vm.rows.find((r) => r.editing)!.text);
      await fireEvent.keyDown(editor, { key: "Enter", isComposing: true });
      expect(onIntent).not.toHaveBeenCalledWith(expect.objectContaining({ type: "edit_submitted" }));
      await fireEvent.input(editor, { target: { value: "Corrected transcript" } });
      await fireEvent.keyDown(editor, { key: "Enter" });
      expect(onIntent).toHaveBeenCalledWith({ type: "edit_submitted", text: "Corrected transcript" });
    });
    it("timestamp seeks the original line ID and active highlighting follows playback", async () => {
      const onIntent = vi.fn(), vm = fixture("TranscriptVm", "completed");
      renderTranscript({ vm, activeIndex: 1, onIntent, onupgrade: vi.fn(), onfeedback: vi.fn(), onanalysisvisible: vi.fn() });
      const line = screen.getByTestId(`transcript-line-${vm.rows[1].id}`);
      expect(line).toHaveAttribute("data-active", "true");
      await userEvent.click(within(line).getByRole("button", { name: vm.rows[1].time_text }));
      expect(onIntent).toHaveBeenCalledWith({ type: "time_clicked", line: vm.rows[1].id });
    });
    it("does not expose timestamp seeking for translated rows", () => {
      const vm = fixture("TranscriptVm", "translation");
      renderTranscript({ vm, activeIndex: -1, onIntent: vi.fn(), onupgrade: vi.fn(), onfeedback: vi.fn(), onanalysisvisible: vi.fn() });
      const line = screen.getByTestId(`transcript-line-${vm.rows[0].id}`);
      expect(within(line).queryByRole("button", { name: vm.rows[0].time_text })).toBeNull();
      expect(screen.queryByRole("switch", { name: "Show Time" })).toBeNull();
    });
    it("download stays unavailable while core reports an unfinished transcript", () => {
      const props = { vm: fixture("TranscriptVm", "running"), activeIndex: -1, onIntent: vi.fn(), onupgrade: vi.fn(), onfeedback: vi.fn(), onanalysisvisible: vi.fn() };
      if (name === "phone") render(MobileTranscript, { ...props, menuOpen: true });
      else render(DesktopTranscript, props);
      expect(screen.getByRole("button", { name: "Download Transcript" })).toBeDisabled();
    });
    it("keeps free-cut transcript text and controls opaque while retaining the upgrade boundary", () => {
      const vm = fixture("TranscriptVm", "freemium");
      renderTranscript({ vm, activeIndex: -1, onIntent: vi.fn(), onupgrade: vi.fn(), onfeedback: vi.fn(), onanalysisvisible: vi.fn() });
      for (const [index] of vm.fades) {
        const row = screen.getByTestId(`transcript-line-${vm.rows[index].id}`);
        const source = vm.rows[index];
        const controls = [within(row).getByRole("button", { name: source.time_text })];
        if (source.speaker) controls.push(within(row).getByRole("button", { name: speakerText(source.speaker) }));
        for (const content of [within(row).getByText(source.text), ...controls]) {
          for (let ancestor: HTMLElement | null = content; ancestor; ancestor = ancestor.parentElement) {
            expect(Number(getComputedStyle(ancestor).opacity || 1)).toBe(1);
          }
        }
      }
      expect(screen.getByRole("button", { name: i18n.tk("common", "upgradeToPro") })).toBeEnabled();
    });
    for (const variant of ["rtl", "long_strings"] as const) {
      it(`preserves ${variant} text and full speaker names through rename`, async () => {
        const vm = fixture("TranscriptVm", variant), onIntent = vi.fn();
        const source = vm.rows[1];
        renderTranscript({ vm, activeIndex: -1, onIntent, onupgrade: vi.fn(), onfeedback: vi.fn(), onanalysisvisible: vi.fn() });
        const row = screen.getByTestId(`transcript-line-${source.id}`);
        expect(row.closest("ol")).toHaveAttribute("dir", variant === "rtl" ? "rtl" : "ltr");
        expect(within(row).getByText(source.text)).toBeVisible();
        const timestamp = within(row).getByRole("button", { name: source.time_text });
        expect(timestamp).toHaveAttribute("dir", "ltr");
        await userEvent.click(timestamp);
        expect(onIntent).toHaveBeenCalledWith({ type: "time_clicked", line: source.id });
        if (!source.speaker) throw new Error(`${variant} fixture must include its stress speaker`);
        await userEvent.click(within(row).getByRole("button", { name: speakerText(source.speaker) }));
        await userEvent.click(within(row).getByRole("button", { name: "Edit speaker..." }));
        const editor = within(row).getByRole("textbox", { name: "Edit speaker label" });
        expect(editor).toHaveValue(source.speaker_value);
        const renamed = `${source.speaker_value} 2026`;
        await fireEvent.input(editor, { target: { value: renamed } });
        await fireEvent.keyDown(editor, { key: "Enter" });
        expect(onIntent).toHaveBeenCalledWith({ type: "speaker_renamed", from: source.speaker_value, to: renamed });
      });
    }
  });
}
