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
  });
}
