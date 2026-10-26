import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import type { JobListVm } from "$lib/core/types/JobListVm";
import type { JobRow } from "$lib/core/types/JobRow";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopHistoryTable from "./DesktopHistoryTable.svelte";

const sample = fixture("JobListVm", "sample");

function row(id: number, patch: Partial<JobRow>): JobRow {
  return { ...sample.rows[0], id, selected: false, downloading: false, ...patch };
}

function setup(vm: JobListVm) {
  const intents = {
    onopen: vi.fn(),
    ondownload: vi.fn(),
    onfeedback: vi.fn(),
    ondelete: vi.fn(),
    onconfirmdelete: vi.fn(),
    oncanceldelete: vi.fn(),
    onloadmore: vi.fn(),
  };
  render(DesktopHistoryTable, { vm, ...intents });
  return intents;
}

beforeAll(() => i18n.use("en"));

describe("DesktopHistoryTable", () => {
  it("offers download and feedback only where core allows them, and actions never open the row", async () => {
    const vm: JobListVm = {
      ...sample,
      rows: [
        row(1, { display_name: "done", can_download: true, can_feedback: true }),
        row(2, { display_name: "busy", can_download: false, can_feedback: false }),
      ],
    };
    const intents = setup(vm);
    const table = screen.getByRole("table");
    const [, done, busy] = within(table).getAllByRole("row");
    expect(within(done).getByRole("button", { name: "Download" })).toBeInTheDocument();
    expect(within(done).getByRole("button", { name: "Feedback" })).toBeInTheDocument();
    expect(within(busy).queryByRole("button", { name: "Download" })).toBeNull();
    expect(within(busy).queryByRole("button", { name: "Feedback" })).toBeNull();

    await userEvent.click(within(done).getByRole("button", { name: "Download" }));
    expect(intents.ondownload).toHaveBeenCalledWith(1);
    expect(intents.onopen).not.toHaveBeenCalled();
    await userEvent.click(within(busy).getByText("busy"));
    expect(intents.onopen).toHaveBeenCalledWith(2);
  });

  it("shows core's fallbacks for a missing name, language and duration", () => {
    setup({ ...sample, rows: [row(3, { display_name: null, language: null, duration: null })] });
    const table = screen.getByRole("table");
    expect(within(table).getByText(i18n.t(sample.untitled))).toBeInTheDocument();
    expect(within(table).getByText(i18n.t(sample.auto_detect))).toBeInTheDocument();
    expect(within(table).getByText(i18n.t(sample.not_available))).toBeInTheDocument();
  });

  it("asks core to delete, then renders its confirmation and reports the answer", async () => {
    const intents = setup({ ...sample, rows: [row(4, { display_name: "old" })] });
    await userEvent.click(within(screen.getByRole("table")).getByRole("button", { name: "Delete" }));
    expect(intents.ondelete).toHaveBeenCalledWith(4);

    const confirming = setup({ ...sample, confirm_delete: 4, rows: [row(4, { display_name: "old" })] });
    const dialog = screen.getByRole("dialog", { name: "Are you sure you want to delete this file?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Yes" }));
    expect(confirming.onconfirmdelete).toHaveBeenCalledOnce();
  });

  it("marks the open job's row as current", () => {
    setup({ ...sample, rows: [row(5, { display_name: "open", selected: true })] });
    const current = within(screen.getByRole("table")).getAllByRole("row").find((r) => r.getAttribute("aria-current") === "true");
    expect(current).toHaveTextContent("open");
  });

  it("renders the loading, error and empty states core decides", () => {
    setup(fixture("JobListVm", "loading"));
    expect(screen.getByRole("status")).toHaveTextContent("Loading jobs...");
  });

  it("shows the load error with its detail", () => {
    const vm = fixture("JobListVm", "error");
    setup(vm);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(i18n.t(vm.error!));
    if (vm.error_detail) expect(alert).toHaveTextContent(vm.error_detail);
  });
});
