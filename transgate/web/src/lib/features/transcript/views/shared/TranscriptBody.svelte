<script lang="ts">
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import type { TranscriptIntent } from "$lib/core/types/TranscriptIntent";
  import TranscriptLines from "./TranscriptLines.svelte";
  let { vm, activeIndex, onIntent, onupgrade }: { vm: TranscriptVm; activeIndex: number; onIntent: (intent: TranscriptIntent) => void; onupgrade: () => void } = $props();
</script>
<TranscriptLines {vm} {activeIndex} {onupgrade}
  ontab={(tab) => onIntent({ type: "tab_selected", tab })}
  onedit={(line) => onIntent({ type: "edit_started", line })}
  oncanceledit={() => onIntent({ type: "edit_canceled" })}
  onsubmitedit={(text) => onIntent({ type: "edit_submitted", text })}
  onspeaker={(line, speaker) => onIntent({ type: "speaker_chosen", line, speaker })}
  onrename={(from, to) => onIntent({ type: "speaker_renamed", from, to })}
  ontime={(line) => onIntent({ type: "time_clicked", line })} />
