import { fireEvent, render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import type { JobListVm } from "$lib/core/types/JobListVm";
import type { JobRow } from "$lib/core/types/JobRow";
import { i18n } from "$lib/i18n/index.svelte";
import MobileHistoryList from "./MobileHistoryList.svelte";

const sample = fixture("JobListVm", "sample");

function row(id: number, patch: Partial<JobRow> = {}): JobRow {
  return { ...sample.rows[0], id, display_name: `job ${id}`, selected: false, ...patch };
}

function setup(vm: JobListVm) {
  const intents = {
    onopen: vi.fn(),
    ondelete: vi.fn(),
    onconfirmdelete: vi.fn(),
    oncanceldelete: vi.fn(),
    onloadmore: vi.fn(),
    onrefresh: vi.fn(),
    onnew: vi.fn(),
  };
  render(MobileHistoryList, { vm, ...intents });
  return intents;
}

function touch(element: Element, type: string, clientX: number) {
  fireEvent(element, new PointerEvent(type, { bubbles: true, pointerType: "touch", clientX }));
}

beforeAll(() => i18n.use("en"));

describe("MobileHistoryList", () => {
  it("opens a job on tap", async () => {
    const intents = setup({ ...sample, rows: [row(1)] });
    await userEvent.click(screen.getByRole("button", { name: /job 1/ }));
    expect(intents.onopen).toHaveBeenCalledWith(1);
  });

  it("a left swipe past the threshold reveals Delete instead of opening; Delete asks core", async () => {
    const intents = setup({ ...sample, rows: [row(2)] });
    const card = screen.getByRole("button", { name: /job 2/ });
    touch(card, "pointerdown", 200);
    touch(card, "pointermove", 100);
    touch(card, "pointerup", 100);
    expect(card.style.transform).toBe("translateX(-120px)");
    await userEvent.click(card);
    expect(intents.onopen).not.toHaveBeenCalled();
    expect(card.style.transform).toBe("translateX(0px)");

    touch(card, "pointerdown", 200);
    touch(card, "pointermove", 100);
    touch(card, "pointerup", 100);
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(intents.ondelete).toHaveBeenCalledWith(2);
  });

  it("a short swipe snaps back and keyboard users still reach Delete", async () => {
    const intents = setup({ ...sample, rows: [row(3)] });
    const card = screen.getByRole("button", { name: /job 3/ });
    touch(card, "pointerdown", 200);
    touch(card, "pointermove", 150);
    touch(card, "pointerup", 150);
    expect(card.style.transform).toBe("translateX(0px)");
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "Delete" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(intents.ondelete).toHaveBeenCalledWith(3);
  });

  it("shows the speakers metadata only when core sends it", () => {
    setup({ ...sample, rows: [row(4, { speakers: { ns: "app", key: "2+ speakers", args: {} } as JobRow["speakers"] }), row(5, { speakers: null })] });
    expect(screen.getAllByText(i18n.t({ ns: "app", key: "2+ speakers", args: {} }))).toHaveLength(1);
  });

  it("offers Try Again on a load error and a new transcription when empty", async () => {
    const failed = setup(fixture("JobListVm", "error"));
    await userEvent.click(screen.getByRole("button", { name: "Try Again" }));
    expect(failed.onrefresh).toHaveBeenCalledOnce();
  });

  it("starts a new transcription from the empty state", async () => {
    const empty = setup(fixture("JobListVm", "empty"));
    await userEvent.click(screen.getByRole("button", { name: "New Transcription" }));
    expect(empty.onnew).toHaveBeenCalledOnce();
  });
});
