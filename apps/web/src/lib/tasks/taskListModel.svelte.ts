// Container: holds the latest view and forwards intents. No logic of its own;
// the state machine lives in Rust.
import type { TaskCore, TaskIntent, TaskListView } from "../core";

export interface TaskListModel {
  readonly view: TaskListView;
  send(intent: TaskIntent): Promise<void>;
}

export async function createTaskListModel(core: TaskCore): Promise<TaskListModel> {
  let view = $state<TaskListView>(await core.dispatch({ type: "Opened" }));
  return {
    get view() {
      return view;
    },
    async send(intent) {
      await core.dispatch(intent);
      // The latest state, not this dispatch's, in case an earlier slow one finished last.
      view = core.current();
    },
  };
}
