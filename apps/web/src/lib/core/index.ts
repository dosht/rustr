// The web shell's only door into the Rust core. Components depend on the
// `TaskCore` interface, so stories and tests can pass a fake instead of wasm.
import init, { TaskListPresenter, previewScreens } from "./pkg/taskr_wasm.js";
import type { PreviewScreen } from "./types/PreviewScreen";
import type { TaskIntent } from "./types/TaskIntent";
import type { TaskListView } from "./types/TaskListView";

export type { ColumnView } from "./types/ColumnView";
export type { PreviewScreen } from "./types/PreviewScreen";
export type { ScopeTabView } from "./types/ScopeTabView";
export type { TaskIntent } from "./types/TaskIntent";
export type { TaskListView } from "./types/TaskListView";
export type { TaskRowView } from "./types/TaskRowView";
export type { TaskScope } from "./types/TaskScope";

export interface TaskCore {
  dispatch(intent: TaskIntent): TaskListView;
  current(): TaskListView;
}

let ready: Promise<unknown> | undefined;

function loadWasm(): Promise<unknown> {
  ready ??= init();
  return ready;
}

export async function loadCore(): Promise<TaskCore> {
  await loadWasm();
  const presenter = new TaskListPresenter();
  return {
    dispatch: (intent) => presenter.dispatch(intent) as TaskListView,
    current: () => presenter.current() as TaskListView,
  };
}

export async function loadPreviewScreens(): Promise<PreviewScreen[]> {
  await loadWasm();
  return previewScreens() as PreviewScreen[];
}
