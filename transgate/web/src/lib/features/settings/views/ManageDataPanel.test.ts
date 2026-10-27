import { render, screen } from "@testing-library/svelte";
import { beforeAll, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import ManageDataPanel from "./ManageDataPanel.svelte";

beforeAll(() => i18n.use("en"));

const props = (name: string) => ({
  vm: fixture("DataManagementVm", name), kind: "dialog" as const,
  oncleanup: vi.fn(), ondeletemonthly: vi.fn(), ondeleteall: vi.fn(), ondeleteaccount: vi.fn(), onconfirm: vi.fn(), oncancel: vi.fn(),
});

it("shows core's refused-settings-write notice while the switches keep the stored values", () => {
  render(ManageDataPanel, props("save_failed"));
  expect(screen.getByRole("alert")).toHaveTextContent("Your setting could not be saved. Please try again.");
  expect(screen.getByRole("switch", { name: "Automatic transcript cleanup (30 days)" })).toBeChecked();
});

it("shows no error notice when core reports none", () => {
  render(ManageDataPanel, props("loaded"));
  expect(screen.queryByRole("alert")).toBeNull();
});
