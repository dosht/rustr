<script lang="ts">
  // Source SupportAnalysis: issue resolution and customer sentiment (no copy
  // button in the source), agent performance, KB updates, follow-up actions.
  import type { SectionId } from "$lib/core/types/SectionId";
  import type { SupportAnalysis } from "$lib/core/types/SupportAnalysis";
  import { i18n } from "$lib/i18n/index.svelte";
  import { clock } from "../../format";
  import SectionHeader from "./SectionHeader.svelte";

  let { data, oncopy }: { data: SupportAnalysis; oncopy: (section: SectionId) => void } = $props();
  const issue = $derived(data.issueResolution);
  const sentiment = $derived(data.sentiment);
  const strengths = $derived(data.insights?.strengths ?? []);
  const improvements = $derived(data.insights?.improvements ?? []);
  const kb = $derived(data.kbUpdates ?? []);
  const followUps = $derived(data.followUpActions ?? []);
  const emoji: Record<string, string> = { frustrated: "😤", neutral: "😐", satisfied: "😊", relieved: "😌" };
  const statusTone = { resolved: "border-success text-success", unresolved: "border-error text-error", escalated: "border-progress text-progress" } as const;
</script>

<div data-testid="support-analysis" class="space-y-6">
  {#if issue}
    <section data-testid="issue-resolution">
      <SectionHeader title="Issue Resolution" />
      <div class="space-y-3 rounded-md border border-border p-3 text-sm">
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <p class="text-xs text-muted">{i18n.tk("ai", "Problem")}</p>
            <p class="break-words font-medium">{issue.problem}</p>
          </div>
          <span class="shrink-0 rounded border px-2 py-1 text-xs font-medium {statusTone[issue.status]}">
            {issue.status.charAt(0).toUpperCase() + issue.status.slice(1)}
          </span>
        </div>
        {#if issue.rootCause}
          <div><p class="text-xs text-muted">{i18n.tk("ai", "Root Cause")}</p><p class="break-words">{issue.rootCause}</p></div>
        {/if}
        {#if issue.resolution}
          <div><p class="text-xs text-muted">{i18n.tk("ai", "Resolution")}</p><p class="break-words">{issue.resolution}</p></div>
        {/if}
        {#if issue.resolutionTimeMinutes != null}
          <div><p class="text-xs text-muted">{i18n.tk("ai", "Resolution Time")}</p><p>{issue.resolutionTimeMinutes} {i18n.tk("ai", "minutes")}</p></div>
        {/if}
      </div>
    </section>
  {/if}
  {#if sentiment}
    <section data-testid="sentiment-analysis">
      <SectionHeader title="Customer Sentiment" />
      <div class="space-y-3 text-sm">
        <div class="flex items-center justify-around rounded-md border border-border p-3 text-center">
          <div>
            <p class="text-xs text-muted">{i18n.tk("ai", "Initial")}</p>
            <p aria-hidden="true" class="text-2xl">{emoji[sentiment.initial] ?? "😐"}</p>
            <p class="capitalize">{sentiment.initial}</p>
          </div>
          <span aria-hidden="true" class="text-muted">→</span>
          <div>
            <p class="text-xs text-muted">{i18n.tk("ai", "Final")}</p>
            <p aria-hidden="true" class="text-2xl">{emoji[sentiment.final] ?? "😐"}</p>
            <p class="capitalize">{sentiment.final}</p>
          </div>
        </div>
        {#if sentiment.keyMoments && sentiment.keyMoments.length > 0}
          <div>
            <p class="mb-1 text-xs font-medium text-muted">{i18n.tk("ai", "Key Moments")}</p>
            <ul class="space-y-1">
              {#each sentiment.keyMoments as moment, i (i)}
                <li class="flex items-center gap-2">
                  <span class="font-mono text-xs text-muted">{clock(moment.timestamp)}</span>
                  <span aria-hidden="true">{emoji[moment.sentiment] ?? "😐"}</span>
                  <span class="break-words">{moment.reason || moment.sentiment}</span>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    </section>
  {/if}
  {#if strengths.length + improvements.length > 0}
    <section data-testid="support-insights">
      <SectionHeader title="Agent Performance" section="support_insights" {oncopy} />
      <div class="space-y-3 text-sm">
        {#if strengths.length > 0}
          <div>
            <h4 class="mb-1 font-medium text-success">{i18n.tk("ai", "Strengths")}</h4>
            <ul class="space-y-1">{#each strengths as s, i (i)}<li class="flex gap-2"><span aria-hidden="true" class="text-success">✓</span><span>{s}</span></li>{/each}</ul>
          </div>
        {/if}
        {#if improvements.length > 0}
          <div>
            <h4 class="mb-1 font-medium text-progress">{i18n.tk("ai", "Areas for Improvement")}</h4>
            <ul class="space-y-1">{#each improvements as s, i (i)}<li class="flex gap-2"><span aria-hidden="true" class="text-progress">→</span><span>{s}</span></li>{/each}</ul>
          </div>
        {/if}
      </div>
    </section>
  {/if}
  {#if kb.length > 0}
    <section data-testid="kb-updates">
      <SectionHeader title="Knowledge Base Updates" count={kb.length} section="kb_updates" {oncopy} />
      <ul class="space-y-2">{#each kb as item, i (i)}<li class="flex gap-2 text-sm"><span aria-hidden="true">📝</span><span class="break-words">{item}</span></li>{/each}</ul>
    </section>
  {/if}
  {#if followUps.length > 0}
    <section data-testid="follow-up-actions">
      <SectionHeader title="Follow-Up Actions" count={followUps.length} section="follow_up_actions" {oncopy} />
      <ul class="space-y-2">{#each followUps as item, i (i)}<li class="flex gap-2 text-sm"><span aria-hidden="true">☐</span><span class="break-words">{item}</span></li>{/each}</ul>
    </section>
  {/if}
</div>
