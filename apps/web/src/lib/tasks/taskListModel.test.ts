import { describe, expect, it, vi } from "vitest";
import type { TaskCore } from "../core";
import { taskScreen } from "../core/fixtures";
import { createTaskListModel } from "./taskListModel.svelte";

describe("createTaskListModel", () => {
  it("opens the list and then shows whatever the core holds", async () => {
    let current = taskScreen("empty");
    const core: TaskCore = {
      dispatch: vi.fn(async () => current),
      current: () => current,
    };
    const model = await createTaskListModel(core);
    expect(core.dispatch).toHaveBeenCalledWith({ type: "Opened" });
    expect(model.view.emptyMessage).toBe("Nothing here yet. Add your first task.");

    current = taskScreen("sample");
    await model.send({ type: "AddSubmitted", title: "x" });
    expect(model.view.rows).toHaveLength(3);
  });
});
