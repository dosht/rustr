import type { AudioSummaryCore, AudioSummaryIntent, AudioSummaryView } from "../core";

export type SummaryAction = Exclude<AudioSummaryIntent, { type: "Start" }> | { type: "Start"; accepted: boolean };
export interface SummaryModel {
  readonly view: AudioSummaryView;
  send(intent: SummaryAction): Promise<void>;
  dispose(): void;
}

export function createSummaryModel(core: AudioSummaryCore): SummaryModel {
  let view = $state<AudioSummaryView>(core.current());
  let disposed = false;
  const publish = (next: AudioSummaryView) => { if (!disposed) view = next; };
  return {
    get view() { return view; },
    async send(intent) {
      if (disposed) return;
      const action: AudioSummaryIntent = intent.type === "Start"
        ? { ...intent, request_id: crypto.randomUUID() }
        : intent;
      try {
        publish(await core.dispatch(action, publish));
      } catch (error) {
        // A binding failure is not a durable provider failure. Keep the presenter's
        // current state; dispatch handles authenticated HTTP failures in Rust.
        publish({ ...core.current(), error: String(error) });
      }
    },
    dispose() { disposed = true; core.dispose(); },
  };
}
