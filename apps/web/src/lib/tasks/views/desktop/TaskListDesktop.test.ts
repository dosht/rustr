import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { taskScreen } from "../../../core/fixtures";
import TaskListDesktop from "./TaskListDesktop.svelte";

describe("TaskListDesktop", () => {
  it("renders the rows, columns and tab counts it is given", () => {
    render(TaskListDesktop, { view: taskScreen("sample"), onIntent: vi.fn() });
    expect(screen.getByRole("tab", { name: "Open (3)" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("columnheader", { name: "TITLE" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "Write the architecture doc" })).toBeInTheDocument();
  });

  it("reports intents instead of deciding anything", async () => {
    const onIntent = vi.fn();
    render(TaskListDesktop, { view: taskScreen("sample"), onIntent });
    await userEvent.type(screen.getByLabelText("New task title"), "Buy milk{Enter}");
    expect(onIntent).toHaveBeenCalledWith({ type: "AddSubmitted", title: "Buy milk" });

    await userEvent.click(screen.getByRole("tab", { name: "Done (1)" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "ScopeChanged", scope: "done" });

    await userEvent.click(screen.getByLabelText('Mark "Write the architecture doc" done'));
    expect(onIntent).toHaveBeenCalledWith({ type: "ToggleRequested", id: 1 });
  });

  it("is read only for viewers", () => {
    render(TaskListDesktop, { view: taskScreen("read-only"), onIntent: vi.fn() });
    expect(screen.queryByLabelText("New task title")).not.toBeInTheDocument();
    expect(screen.getByText(/You can view this list/)).toBeInTheDocument();
    for (const box of screen.getAllByRole("checkbox")) expect(box).toBeDisabled();
  });

  it("shows the core's empty and error messages verbatim", () => {
    const { unmount } = render(TaskListDesktop, { view: taskScreen("empty"), onIntent: vi.fn() });
    expect(screen.getByText("Nothing here yet. Add your first task.")).toBeInTheDocument();
    unmount();
    render(TaskListDesktop, { view: taskScreen("error"), onIntent: vi.fn() });
    expect(screen.getByRole("alert")).toHaveTextContent("There is already an open task with that title.");
  });
});
