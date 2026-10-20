import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { taskScreen } from "../../../core/fixtures";
import TaskListMobile from "./TaskListMobile.svelte";

describe("TaskListMobile", () => {
  it("shows each task as a tappable card", async () => {
    const onIntent = vi.fn();
    render(TaskListMobile, { view: taskScreen("sample"), onIntent });
    const card = screen.getByRole("button", { name: "Write the architecture doc, open" });
    await userEvent.click(card);
    expect(onIntent).toHaveBeenCalledWith({ type: "ToggleRequested", id: 1 });
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("adds from the bottom bar", async () => {
    const onIntent = vi.fn();
    render(TaskListMobile, { view: taskScreen("sample"), onIntent });
    await userEvent.type(screen.getByLabelText("New task title"), "Call mom");
    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "AddSubmitted", title: "Call mom" });
  });

  it("is read only for viewers", () => {
    render(TaskListMobile, { view: taskScreen("read-only"), onIntent: vi.fn() });
    expect(screen.queryByLabelText("New task title")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Write the architecture doc, open" })).toBeDisabled();
  });
});
