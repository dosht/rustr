import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopSidebar from "./desktop/DesktopSidebar.svelte";
import MobileRecent from "./mobile/MobileRecent.svelte";
beforeAll(() => i18n.use("en"));
it("sidebar offers feedback for an unfinished job independently of the table capability", async () => {
  const vm = fixture("JobListVm", "sample"), onfeedback = vi.fn(), onopen = vi.fn();
  render(DesktopSidebar, { vm, collapsed: false, onopen, onfeedback, ondelete: vi.fn(), onconfirmdelete: vi.fn(), oncanceldelete: vi.fn(), onloadmore: vi.fn(), onnew: vi.fn(), onall: vi.fn() });
  const row = screen.getByText(vm.rows[0].sidebar_name).closest("li")!;
  await userEvent.click(within(row).getByRole("button", { name: "More options" }));
  await userEvent.click(within(row).getByRole("button", { name: "Feedback" }));
  expect(onfeedback).toHaveBeenCalledWith(vm.rows[0].id);
  expect(onopen).not.toHaveBeenCalled();
});
it("phone drawer keeps raw filenames and source duration text, with no delete or paging", async () => {
  const vm = fixture("JobListVm", "sample"), onopen = vi.fn();
  render(MobileRecent, { vm, onopen });
  const first = screen.getByRole("button", { name: new RegExp(vm.recent[0].file_name!) });
  expect(first).toHaveTextContent(vm.recent[0].duration!);
  await userEvent.click(first);
  expect(onopen).toHaveBeenCalledWith(vm.recent[0].id);
  expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  expect(screen.queryByRole("button", { name: /Loading more/ })).toBeNull();
});
it("a collapsed sidebar keeps navigation but does not mount transcript rows", () => {
  render(DesktopSidebar, { vm: fixture("JobListVm", "sample"), collapsed: true, onopen: vi.fn(), onfeedback: vi.fn(), ondelete: vi.fn(), onconfirmdelete: vi.fn(), oncanceldelete: vi.fn(), onloadmore: vi.fn(), onnew: vi.fn(), onall: vi.fn() });
  expect(screen.getByRole("button", { name: "New Transcript" })).toBeInTheDocument();
  expect(screen.queryByText("podcast")).toBeNull();
});
