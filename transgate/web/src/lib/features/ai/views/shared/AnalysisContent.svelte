<script lang="ts">
  // The summary paragraph, then the shape core typed by the detected kind.
  import type { Analysis } from "$lib/core/types/Analysis";
  import type { SectionId } from "$lib/core/types/SectionId";
  import GenericAnalysis from "./GenericAnalysis.svelte";
  import InterviewAnalysis from "./InterviewAnalysis.svelte";
  import LectureAnalysis from "./LectureAnalysis.svelte";
  import MeetingAnalysis from "./MeetingAnalysis.svelte";
  import SalesAnalysis from "./SalesAnalysis.svelte";
  import SupportAnalysis from "./SupportAnalysis.svelte";

  interface Props {
    summary: string | null;
    analysis: Analysis | null;
    oncopy: (section: SectionId) => void;
    /** Phone spacing is tighter, as the source's mt-4/pt-4 against mt-6/pt-6. */
    compact?: boolean;
  }

  let { summary, analysis, oncopy, compact = false }: Props = $props();
</script>

{#if summary}
  <p class="whitespace-pre-wrap text-sm leading-relaxed text-fg">{summary}</p>
{/if}
{#if analysis}
  <div class={summary ? (compact ? "mt-4 border-t border-border pt-4" : "mt-6 border-t border-border pt-6") : ""}>
    {#if analysis.type === "meeting"}
      <MeetingAnalysis data={analysis} {oncopy} />
    {:else if analysis.type === "lecture"}
      <LectureAnalysis data={analysis} {oncopy} />
    {:else if analysis.type === "sales"}
      <SalesAnalysis data={analysis} {oncopy} />
    {:else if analysis.type === "support"}
      <SupportAnalysis data={analysis} {oncopy} />
    {:else if analysis.type === "interview"}
      <InterviewAnalysis data={analysis} {oncopy} />
    {:else}
      <GenericAnalysis data={analysis} />
    {/if}
  </div>
{/if}
