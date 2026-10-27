import { render, screen, fireEvent } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopWorkspace from "./desktop/DesktopWorkspace.svelte";
import MobileWorkspace from "./mobile/MobileWorkspace.svelte";

beforeAll(() => {
  // jsdom cannot scroll a viewport; error visibility is asserted below.
  const scroll = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  return i18n.use("en").then(() => () => scroll.mockRestore());
});
for (const [name, Component] of [["desktop", DesktopWorkspace], ["phone", MobileWorkspace]] as const) {
  describe(`${name} upload workspace`, () => {
    it("keeps Start disabled until the core reports ready and forwards the selected files unchanged", async () => {
      const onpick = vi.fn(), onIntent = vi.fn();
      const view = render(Component, { vm: fixture("UploadQueueVm", "preparing"), youtube: true, fullName: "Ada", onpick, ondrop: vi.fn(), onIntent, onpurchase: vi.fn() });
      const start = screen.getByTestId("upload-start");
      expect(start).toBeDisabled();
      const file = new File(["media"], "recording.wav", { type: "audio/wav" });
      await fireEvent.change(screen.getByTestId("upload-files"), { target: { files: [file] } });
      expect(onpick.mock.calls[0][0][0]).toBe(file);
      await view.rerender({ vm: fixture("UploadQueueVm", "ready") });
      await userEvent.click(start);
      expect(onIntent).toHaveBeenCalledWith({ type: "start_pressed" });
    });
    it("renders source and target language choices without translating stored language names", async () => {
      const onIntent = vi.fn();
      render(Component, { vm: fixture("UploadQueueVm", "translation"), youtube: true, fullName: "", onpick: vi.fn(), ondrop: vi.fn(), onIntent, onpurchase: vi.fn() });
      await userEvent.click(screen.getByRole("button", { name: /Audio Language/ }));
      const search = screen.getByRole("searchbox");
      await userEvent.type(search, "Turkish");
      await userEvent.click(screen.getByRole("button", { name: /Turkish/ }));
      expect(onIntent).toHaveBeenCalledWith({ type: "source_language_chosen", language: "Turkish" });
    });
    it("does not offer the YouTube entry when the capability is off", async () => {
      render(Component, { vm: fixture("UploadQueueVm", "empty"), youtube: false, fullName: "", onpick: vi.fn(), ondrop: vi.fn(), onIntent: vi.fn(), onpurchase: vi.fn() });
      if (name === "desktop") await userEvent.click(screen.getByRole("button", { name: "Upload files" }));
      expect(screen.queryByRole("button", { name: "Add from YouTube" })).toBeNull();
    });
    it("keeps provider failure text visible without inventing a successful upload", () => {
      const vm = fixture("UploadQueueVm", "youtube_error");
      render(Component, { vm, youtube: true, fullName: "", onpick: vi.fn(), ondrop: vi.fn(), onIntent: vi.fn(), onpurchase: vi.fn() });
      expect(screen.getByRole("alert")).toHaveTextContent(i18n.t(vm.youtube_error!));
      if (name === "phone") expect(screen.queryByTestId("upload-start")).toBeNull();
      else expect(screen.getByTestId("upload-start")).toBeDisabled();
    });
    it("labels the 402 balance action with the source wording", async () => {
      const onpurchase = vi.fn();
      render(Component, { vm: fixture("UploadQueueVm", "balance_alert"), youtube: true, fullName: "", onpick: vi.fn(), ondrop: vi.fn(), onIntent: vi.fn(), onpurchase });
      expect(screen.queryByRole("button", { name: "Add Balance" })).toBeNull();
      await userEvent.click(screen.getByRole("button", { name: "Add minutes" }));
      expect(onpurchase).toHaveBeenCalled();
    });
    it("UPL-002 the picker takes several files and binds core's per-layout accept list", () => {
      const vm = fixture("UploadQueueVm", name === "desktop" ? "empty" : "preparing");
      render(Component, { vm, youtube: true, fullName: "", onpick: vi.fn(), ondrop: vi.fn(), onIntent: vi.fn(), onpurchase: vi.fn() });
      const picker = screen.getByTestId("upload-files");
      expect(picker).toHaveAttribute("multiple");
      expect(picker.getAttribute("accept")).toBe(vm.picker_accept);
      expect(vm.picker_accept).toBe(name === "desktop" ? "audio/*,video/*,.m4a" : "audio/mpeg,audio/mp4,audio/wav,audio/webm,.mp3,.m4a,.wav,.webm");
    });
    it("UPL-003 the picker forwards every chosen file unvalidated; filtering belongs to core", async () => {
      const onpick = vi.fn();
      render(Component, { vm: fixture("UploadQueueVm", "empty"), youtube: true, fullName: "", onpick, ondrop: vi.fn(), onIntent: vi.fn(), onpurchase: vi.fn() });
      const files = [new File(["a"], "a.m4a", { type: "" }), new File(["b"], "notes.txt", { type: "text/plain" })];
      await fireEvent.change(screen.getByTestId("upload-files"), { target: { files } });
      expect(onpick).toHaveBeenCalledWith(files);
    });
    it("UPL-004 the add buttons disappear when core reports the queue is full", () => {
      const props = { youtube: true, fullName: "", onpick: vi.fn(), ondrop: vi.fn(), onIntent: vi.fn(), onpurchase: vi.fn() };
      const view = render(Component, { vm: fixture("UploadQueueVm", "ready"), ...props });
      expect(screen.getByRole("button", { name: "Upload files" })).toBeInTheDocument();
      view.unmount();
      render(Component, { vm: fixture("UploadQueueVm", "max_files"), ...props });
      expect(screen.queryByRole("button", { name: "Upload files" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Add from YouTube" })).toBeNull();
    });
    if (name === "desktop") it("UPL-003/UPL-004 a drop forwards every dropped file to core without view-side filtering or capping", async () => {
      const ondrop = vi.fn();
      render(Component, { vm: fixture("UploadQueueVm", "ready"), youtube: true, fullName: "", onpick: vi.fn(), ondrop, onIntent: vi.fn(), onpurchase: vi.fn() });
      const files = [new File(["a"], "a.M4A", { type: "" }), new File(["b"], "pic.png", { type: "image/png" })];
      await fireEvent.drop(screen.getByRole("region", { name: "Upload files" }), { dataTransfer: { files } });
      expect(ondrop).toHaveBeenCalledWith(files);
    });
  });
}
