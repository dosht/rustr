<script lang="ts">
  // Source MeetingAnalysis and its four lists, in source order.
  import type { MeetingAnalysis } from "$lib/core/types/MeetingAnalysis";
  import type { SectionId } from "$lib/core/types/SectionId";
  import { i18n } from "$lib/i18n/index.svelte";
  import { clock } from "../../format";
  import SectionHeader from "./SectionHeader.svelte";

  let { data, oncopy }: { data: MeetingAnalysis; oncopy: (section: SectionId) => void } = $props();
  const actionItems = $derived(data.actionItems ?? []);
  const decisions = $derived(data.decisions ?? []);
  const points = $derived(data.discussionPoints ?? []);
  const steps = $derived(data.nextSteps ?? []);
  const mark = { done: "☑", "in-progress": "◐", pending: "☐" } as const;
  const statusTone = { done: "border-success text-success", "in-progress": "border-progress text-progress", pending: "border-border text-muted" } as const;
</script>

{#if actionItems.length + decisions.length + points.length + steps.length === 0}
  <div data-testid="meeting-analysis-empty" class="p-4 text-sm text-muted">
    {i18n.tk("ai", "No meeting insights available for this transcript.")}
  </div>
{:else}
  <div data-testid="meeting-analysis" class="space-y-6">
    {#if actionItems.length > 0}
      <section data-testid="action-items-list">
        <SectionHeader title="Action Items" count={actionItems.length} section="action_items" {oncopy} />
        <ul class="space-y-2">
          {#each actionItems as item, i (i)}
            <li class="flex items-start gap-2 text-sm">
              <span aria-hidden="true" class="mt-0.5 shrink-0 text-muted">{mark[item.status]}</span>
              <div class="min-w-0 flex-1 break-words">
                <span class={item.status === "done" ? "text-muted line-through" : ""}>
                  {#if item.owner}<span class="font-medium">{item.owner}: </span>{/if}{item.task}
                </span>
                {#if item.deadline}
                  <span class="ml-2 whitespace-nowrap text-xs text-muted">{i18n.tk("ai", "Due:")} {item.deadline}</span>
                {/if}
              </div>
              <span class="hidden shrink-0 rounded border px-2 py-0.5 text-xs sm:inline {statusTone[item.status]}">{item.status}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if decisions.length > 0}
      <section data-testid="decisions-list">
        <SectionHeader title="Decisions Made" count={decisions.length} section="decisions" {oncopy} />
        <ul class="space-y-2">
          {#each decisions as item, i (i)}
            <li class="rounded-md border border-border p-2.5 text-sm sm:p-3">
              <p class="break-words font-medium">{item.decision}</p>
              {#if item.participants && item.participants.length > 0}
                <p class="mt-1 break-words text-xs text-muted">{i18n.tk("ai", "Participants:")} {item.participants.join(", ")}</p>
              {/if}
              {#if item.timestamp != null}
                <p class="mt-1 text-xs text-muted">{i18n.tk("ai", "At:")} {clock(item.timestamp)}</p>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if points.length > 0}
      <section data-testid="discussion-points-list">
        <SectionHeader title="Discussion Points" count={points.length} section="discussion_points" {oncopy} />
        <ul class="space-y-2">
          {#each points as point, i (i)}
            <li class="flex gap-2 text-sm"><span aria-hidden="true" class="text-accent">•</span><span class="break-words">{point}</span></li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if steps.length > 0}
      <section data-testid="next-steps-list">
        <SectionHeader title="Next Steps" count={steps.length} section="next_steps" {oncopy} />
        <ol class="space-y-2">
          {#each steps as step, i (i)}
            <li class="flex gap-2 text-sm"><span class="shrink-0 font-medium text-accent">{i + 1}.</span><span class="break-words">{step}</span></li>
          {/each}
        </ol>
      </section>
    {/if}
  </div>
{/if}
