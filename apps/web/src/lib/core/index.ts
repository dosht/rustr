// The web shell's only door into the Rust core. Components depend on the
// `TaskCore` and `AppCore` interfaces, so stories and tests can pass a fake
// instead of wasm. Every dispatch is async because the core's ports are.
import init, { AppPresenter, TaskListPresenter, appPreviewScreens, previewScreens } from "./pkg/taskr_wasm.js";
import type { AppIntentDto } from "./types/AppIntentDto";
import type { AppView } from "./types/AppView";
import type { PreviewScreen } from "./types/PreviewScreen";
import type { TaskIntent } from "./types/TaskIntent";
import type { TaskListView } from "./types/TaskListView";

export type { AppIntentDto } from "./types/AppIntentDto";
export type { AppScreen } from "./types/AppScreen";
export type { AppView } from "./types/AppView";
export type { ColumnView } from "./types/ColumnView";
export type { MemberRole } from "./types/MemberRole";
export type { MembersIntentDto } from "./types/MembersIntentDto";
export type { MembersView } from "./types/MembersView";
export type { PreviewScreen } from "./types/PreviewScreen";
export type { ScopeTabView } from "./types/ScopeTabView";
export type { ScreenView } from "./types/ScreenView";
export type { SignedInView } from "./types/SignedInView";
export type { SignInView } from "./types/SignInView";
export type { TaskIntent } from "./types/TaskIntent";
export type { TaskListView } from "./types/TaskListView";
export type { TaskRowView } from "./types/TaskRowView";
export type { TaskScope } from "./types/TaskScope";
export type { WorkspaceNavView } from "./types/WorkspaceNavView";

export interface TaskCore {
  dispatch(intent: TaskIntent): Promise<TaskListView>;
  current(): TaskListView;
}

export interface AppCore {
  /** Resolves to the final view; `onView` gets the pending view each time requests start. */
  dispatch(intent: AppIntentDto, onView?: (view: AppView) => void): Promise<AppView>;
  current(): AppView;
}

export interface AppConfig {
  /** Absolute API root, e.g. https://app.example.com/api */
  apiUrl: string;
  /** Supabase Auth root. Without it, the app uses dev sign-in. */
  authUrl?: string;
  authKey?: string;
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
    dispatch: (intent) => presenter.dispatch(intent) as Promise<TaskListView>,
    current: () => presenter.current() as TaskListView,
  };
}

export async function loadApp(config: AppConfig): Promise<AppCore> {
  await loadWasm();
  const presenter = new AppPresenter(config);
  return {
    dispatch: (intent, onView) => presenter.dispatch(intent, onView) as Promise<AppView>,
    current: () => presenter.current() as AppView,
  };
}

export async function loadPreviewScreens(): Promise<PreviewScreen[]> {
  await loadWasm();
  return previewScreens() as PreviewScreen[];
}

export async function loadAppPreviewScreens(): Promise<[string, AppView][]> {
  await loadWasm();
  return appPreviewScreens() as [string, AppView][];
}
