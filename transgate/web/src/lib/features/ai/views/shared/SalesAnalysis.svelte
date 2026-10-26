<script lang="ts">
  // Source SalesAnalysis: call metrics, coaching tips, customer insights and
  // recommended next steps.
  import type { SalesAnalysis } from "$lib/core/types/SalesAnalysis";
  import type { SectionId } from "$lib/core/types/SectionId";
  import { i18n } from "$lib/i18n/index.svelte";
  import SectionHeader from "./SectionHeader.svelte";

  let { data, oncopy }: { data: SalesAnalysis; oncopy: (section: SectionId) => void } = $props();
  const metrics = $derived(data.callMetrics);
  const tips = $derived(data.coachingTips ?? []);
  const strengths = $derived(tips.filter((t) => t.category === "strength"));
  const improvements = $derived(tips.filter((t) => t.category === "improvement"));
  const insights = $derived(data.customerInsights);
  const groups = $derived(
    [
      { title: "Pain Points", items: insights?.painPoints ?? [], tone: "text-error" },
      { title: "Budget Signals", items: insights?.budgetSignals ?? [], tone: "text-accent" },
      { title: "Objections", items: insights?.objections ?? [], tone: "text-progress" },
      { title: "Buying Signals", items: insights?.buyingSignals ?? [], tone: "text-success" },
    ].filter((g) => g.items.length > 0),
  );
  const steps = $derived(data.nextSteps ?? []);
  const hasMetrics = $derived(
    !!metrics && (!!metrics.talkListenRatio || metrics.questionsAsked != null || metrics.objectionsHandled != null || metrics.nextStepsConfirmed != null),
  );
</script>

<div data-testid="sales-analysis" class="space-y-6">
  {#if metrics && hasMetrics}
    <section data-testid="call-metrics">
      <SectionHeader title="Call Metrics" section="call_metrics" {oncopy} />
      <dl class="grid grid-cols-2 gap-3 text-sm">
        {#if metrics.talkListenRatio}
          <div class="rounded-md border border-border p-2"><dt class="text-xs text-muted">{i18n.tk("ai", "Talk:Listen Ratio")}</dt><dd class="font-medium">{metrics.talkListenRatio}</dd></div>
        {/if}
        {#if metrics.questionsAsked != null}
          <div class="rounded-md border border-border p-2"><dt class="text-xs text-muted">{i18n.tk("ai", "Questions Asked")}</dt><dd class="font-medium">{metrics.questionsAsked}</dd></div>
        {/if}
        {#if metrics.objectionsHandled != null}
          <div class="rounded-md border border-border p-2"><dt class="text-xs text-muted">{i18n.tk("ai", "Objections Handled")}</dt><dd class="font-medium">{metrics.objectionsHandled}</dd></div>
        {/if}
        {#if metrics.nextStepsConfirmed != null}
          <div class="rounded-md border border-border p-2">
            <dt class="text-xs text-muted">{i18n.tk("ai", "Next Steps Confirmed")}</dt>
            <dd class="font-medium {metrics.nextStepsConfirmed ? 'text-success' : 'text-error'}">
              {metrics.nextStepsConfirmed ? `✓ ${i18n.tk("common", "Yes")}` : `✗ ${i18n.tk("common", "No")}`}
            </dd>
          </div>
        {/if}
      </dl>
    </section>
  {/if}
  {#if tips.length > 0}
    <section data-testid="coaching-tips">
      <SectionHeader title="Coaching Tips" count={tips.length} section="coaching_tips" {oncopy} />
      <div class="space-y-3 text-sm">
        {#if strengths.length > 0}
          <div>
            <h4 class="mb-1 font-medium text-success">{i18n.tk("ai", "Strengths")}</h4>
            <ul class="space-y-1">{#each strengths as tip, i (i)}<li class="flex gap-2"><span aria-hidden="true" class="text-success">✓</span><span>{tip.tip}</span></li>{/each}</ul>
          </div>
        {/if}
        {#if improvements.length > 0}
          <div>
            <h4 class="mb-1 font-medium text-progress">{i18n.tk("ai", "Areas for Improvement")}</h4>
            <ul class="space-y-1">{#each improvements as tip, i (i)}<li class="flex gap-2"><span aria-hidden="true" class="text-progress">→</span><span>{tip.tip}</span></li>{/each}</ul>
          </div>
        {/if}
      </div>
    </section>
  {/if}
  {#if groups.length > 0}
    <section data-testid="customer-insights">
      <SectionHeader title="Customer Insights" section="customer_insights" {oncopy} />
      <div class="space-y-3 text-sm">
        {#each groups as group (group.title)}
          <div>
            <h4 class="mb-1 font-medium {group.tone}">{i18n.tk("ai", group.title)}</h4>
            <ul class="list-disc space-y-1 pl-5">{#each group.items as item, i (i)}<li>{item}</li>{/each}</ul>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  {#if steps.length > 0}
    <section data-testid="sales-next-steps">
      <SectionHeader title="Recommended Next Steps" count={steps.length} section="sales_next_steps" {oncopy} />
      <ol class="space-y-2">
        {#each steps as step, i (i)}
          <li class="flex gap-2 text-sm"><span class="shrink-0 font-medium text-accent">{i + 1}.</span><span class="break-words">{step}</span></li>
        {/each}
      </ol>
    </section>
  {/if}
</div>
