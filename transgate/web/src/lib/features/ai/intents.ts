// What the AI views ask for. Views never mint request keys or read clocks.
// The container maps callbacks to generated AppIntent; the core app projection
// supplies keys and clock/layout context when constructing leaf events (P5 D6).
import type { DocumentFormat } from "$lib/core/types/DocumentFormat";
import type { Rating } from "$lib/core/types/Rating";
import type { SectionId } from "$lib/core/types/SectionId";

export type ConsentIntent =
  /** Introduction "Start ..." / "Accept & Enable", or settings "Enable AI Features". */
  | { type: "enable" }
  /** "Decline" in the introduction's learn-more modal. */
  | { type: "decline" }
  /** The settings "turn off AI and delete saved AI data" confirmation: the revoking DELETE (D22). */
  | { type: "disable" }
  | { type: "dismiss_error" };

export type AnalysisIntent =
  | { type: "retry" }
  /** The whole displayed analysis, as captured at the click. */
  | { type: "copy" }
  | { type: "copy_section"; section: SectionId }
  | { type: "export"; format: DocumentFormat }
  | { type: "dismiss_export_error" };

export type ChatIntent =
  /** The question exactly as typed; the view clears its draft. */
  | { type: "send"; text: string }
  /** `index` counts displayed messages, as the source's message actions did. */
  | { type: "retry"; index: number }
  | { type: "copy_message"; index: number }
  | { type: "rate"; index: number; rating: Rating }
  | { type: "copy_chat" }
  | { type: "download" }
  | { type: "dismiss_error" }
  | { type: "dismiss_notice" };
