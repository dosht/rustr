// The host's single trusted checkout confirmation result (tg-confirm island,
// web-shell.md section 7; p1R's serde-tagged `ConfirmOutcome`). The shell
// checks the tag and passes the envelope to core unchanged: a pending payment
// stays pending inside `view`, a failure keeps its HTTP status.
import { ConfigError, readIsland } from "$lib/platform/config";

export type ConfirmOutcomeJson =
  | { outcome: "confirmed"; view: Record<string, unknown> }
  | { outcome: "failed"; status: number; code: string; message: string };

/** `undefined` when this document is not a confirmation response. */
export function readConfirmOutcome(doc: Document = document): ConfirmOutcomeJson | undefined {
  const value = readIsland("tg-confirm", doc);
  if (value === undefined) return undefined;
  if (typeof value !== "object" || value === null) throw new ConfigError("tg-confirm must be an object");
  const json = value as Record<string, unknown>;
  if (json.outcome === "confirmed" && typeof json.view === "object" && json.view !== null && !Array.isArray(json.view)) {
    return { outcome: "confirmed", view: json.view as Record<string, unknown> };
  }
  if (
    json.outcome === "failed" &&
    Number.isInteger(json.status) &&
    typeof json.code === "string" &&
    typeof json.message === "string"
  ) {
    return { outcome: "failed", status: json.status as number, code: json.code, message: json.message };
  }
  throw new ConfigError("tg-confirm is not a ConfirmOutcome");
}
