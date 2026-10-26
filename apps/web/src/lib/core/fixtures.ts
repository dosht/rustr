// Named screens produced by the Rust core (`cargo test -p taskr-wasm` writes
// fixtures.json). Stories and component tests render these, so they show
// exactly what the core would hand the shell.
import data from "./fixtures.json";
import type { AppView, SignedInView, SignInView, TaskListView } from ".";
import type { AudioSummaryView } from "./types/AudioSummaryView";

export const taskScreens = data.tasks as unknown as Record<string, TaskListView>;
export const appScreens = data.app as unknown as Record<string, AppView>;

export function taskScreen(name: string): TaskListView {
  const view = taskScreens[name];
  if (!view) throw new Error(`no task fixture named ${name}`);
  return view;
}

export function signedIn(name: string): SignedInView {
  const view = appScreens[name];
  if (view?.state !== "signedIn") throw new Error(`${name} is not a signed-in fixture`);
  return view.view;
}

export function signIn(name: string): SignInView {
  const view = appScreens[name];
  if (view?.state !== "signedOut") throw new Error(`${name} is not a sign-in fixture`);
  return view.view;
}

export function summaryScreen(name: string): AudioSummaryView {
  // Generated from Rust by the same exporter as task/app fixtures.
  const generated = data as unknown as { summary: Record<string, AudioSummaryView> };
  const view = generated.summary?.[name];
  if (!view) throw new Error(`no summary fixture named ${name}; regenerate with cargo test -p taskr-wasm`);
  return view;
}
