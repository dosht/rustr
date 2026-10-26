// Core decides the speaker label (`SpeakerLabel`, EDT-004 `formatSpeakerLabel`); the view only
// renders it: "Speaker N" for numbered labels, the stored name otherwise, "N/A" when there is none
// (`SpeakerLabel.tsx:66`).
import type { SpeakerLabel } from "$lib/core/types/SpeakerLabel";
import { i18n } from "$lib/i18n/index.svelte";

export function speakerText(label: SpeakerLabel): string {
  switch (label.kind) {
    case "numbered":
      return `${i18n.tk("common", "Speaker")} ${label.number}`;
    case "named":
      return label.name;
    case "none":
      return i18n.tk("app", "N/A");
  }
}
