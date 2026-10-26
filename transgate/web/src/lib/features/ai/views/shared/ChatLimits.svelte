<script lang="ts">
  // Source ChatLimitsInfo. Core picks the counters, bar widths, tones and the
  // reset label; the view only words them.
  import Clock from "@lucide/svelte/icons/clock";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Zap from "@lucide/svelte/icons/zap";
  import type { LimitsVm } from "$lib/core/types/LimitsVm";
  import type { UsageVm } from "$lib/core/types/UsageVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import { localDate } from "../../format";

  let { limits }: { limits: LimitsVm } = $props();

  const bar = { accent: "bg-accent", progress: "bg-progress", danger: "bg-error", neutral: "bg-muted", success: "bg-success" } as const;

  const reset = $derived.by(() => {
    const r = limits.reset;
    if (!r) return null;
    switch (r.type) {
      case "less_than_hour":
        return i18n.tk("ai", "Resets in less than 1 hour");
      case "in_hours":
        return `${i18n.tk("ai", "Resets in")} ${r.hours} ${i18n.tk("ai", r.hours > 1 ? "hours" : "hour")}`;
      case "in_days":
        return `${i18n.tk("ai", "Resets in")} ${r.days} ${i18n.tk("ai", r.days > 1 ? "days" : "day")}`;
      case "on":
        return `${i18n.tk("ai", "Resets on")} ${localDate(r.at, i18n.locale)}`;
    }
  });
</script>

{#snippet usage(label: string, icon: typeof Zap, u: UsageVm)}
  {@const Icon = icon}
  <div class="space-y-1">
    <div class="flex items-center justify-between text-xs">
      <span class="flex items-center gap-1 text-muted"><Icon size={12} aria-hidden="true" />{label}</span>
      <span class="font-medium text-fg">{u.current} / {u.max}</span>
    </div>
    <div class="h-1.5 overflow-hidden rounded-full bg-cursor-bg" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={u.max} aria-valuenow={u.current}>
      <div class="h-full rounded-full {bar[u.tone]}" style:width={`${u.width_percent}%`}></div>
    </div>
  </div>
{/snippet}

<div class="rounded-lg border border-border bg-notice-bg p-3" data-testid="chat-limits">
  <div class="mb-2 flex items-center gap-1.5">
    <TriangleAlert size={14} aria-hidden="true" class="text-progress" />
    <h5 class="text-xs font-semibold text-fg">{i18n.tk("ai", "Usage Limits")}</h5>
  </div>
  <div class="space-y-2">
    {#if limits.questions}{@render usage(i18n.tk("ai", "Questions"), MessageSquare, limits.questions)}{/if}
    {#if limits.tokens}{@render usage(i18n.tk("ai", "Tokens"), Zap, limits.tokens)}{/if}
    {#if reset}
      <p class="flex items-center gap-1 text-xs text-muted"><Clock size={12} aria-hidden="true" /><span>{reset}</span></p>
    {/if}
  </div>
</div>
