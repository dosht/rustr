// Container: holds the latest view and forwards intents. No logic of its own;
// the state machine lives in Rust.
import type { TaskCore, TaskIntent, TaskListView } from "../core";

export interface TaskListModel {
  readonly view: TaskListView;
  send(intent: TaskIntent): void;
}

export function createTaskListModel(core: TaskCore): TaskListModel {
  let view = $state<TaskListView>(core.dispatch({ type: "Opened" }));
  return {
    get view() {
      return view;
    },
    send(intent) {
      view = core.dispatch(intent);
    },
  };
}
