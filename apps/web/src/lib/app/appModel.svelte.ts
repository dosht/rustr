// Container for the signed-in app: the latest AppView, intents forwarded to
// Rust. Sign-in, navigation and session expiry are all decided there.
import type { AppCore, AppIntentDto, AppView } from "../core";

export interface AppModel {
  readonly view: AppView;
  send(intent: AppIntentDto): Promise<void>;
}

export async function createAppModel(core: AppCore): Promise<AppModel> {
  let view = $state<AppView>(await core.dispatch({ type: "Started" }));
  return {
    get view() {
      return view;
    },
    async send(intent) {
      // Show the pending screen (e.g. the new workspace loading) while
      // requests run, so the old one does not stay interactive.
      await core.dispatch(intent, (pending) => (view = pending));
      view = core.current();
    },
  };
}
