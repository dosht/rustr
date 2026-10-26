<script lang="ts">
  // Phone AI Analysis view (source MobileSummaryView): pure, props in and
  // intents out. The container decides when to reset `scrollResetKey`.
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Copy from "@lucide/svelte/icons/copy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import type { AnalysisPanelVm } from "$lib/core/types/AnalysisPanelVm";
  import type { ConsentPanelVm } from "$lib/core/types/ConsentPanelVm";
  import Button from "$lib/ui/Button.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Progress from "$lib/ui/Progress.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { AnalysisIntent, ConsentIntent } from "../../intents";
  import AiGate from "../shared/AiGate.svelte";
  import AnalysisContent from "../shared/AnalysisContent.svelte";
  import ExportMenu from "../shared/ExportMenu.svelte";
  import TypeBadge from "../shared/TypeBadge.svelte";

  interface Props {
    consent: ConsentPanelVm;
    view: AnalysisPanelVm;
    /** Changing it scrolls the content back to the top (another job). */
    scrollResetKey?: number;
    onconsent: (intent: ConsentIntent) => void;
    onintent: (intent: AnalysisIntent) => void;
  }

  let { consent, view, scrollResetKey = 0, onconsent, onintent }: Props = $props();
  let copied = $state(false);
  let scroller: HTMLDivElement | undefined = $state();

  $effect(() => {
    void scrollResetKey;
    if (scroller) scroller.scrollTop = 0;
  });

  $effect(() => {
    if (!copied) return;
    const timer = setTimeout(() => (copied = false), 2000);
    return () => clearTimeout(timer);
  });

  function copy() {
    onintent({ type: "copy" });
    copied = true;
  }
</script>

{#snippet progress()}
  <div class="space-y-3" data-testid="analysis-progress">
    <div class="space-y-1">
      <p class="text-sm text-muted">{view.progress ? i18n.t(view.progress.stage) : i18n.tk("ai", "Analyzing...")}</p>
      <Progress label={view.progress ? i18n.t(view.progress.stage) : i18n.tk("ai", "Analyzing...")} value={view.progress?.percentage ?? 0} />
    </div>
    <Skeleton label={i18n.tk("ai", "Analyzing...")} lines={4} />
  </div>
{/snippet}

<AiGate {consent} feature="summary" phone onintent={onconsent}>
  <div class="flex h-full flex-col" data-testid="analysis-panel">
    <div class="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
      <div class="flex min-w-0 items-center gap-2">
        <span class="flex h-7 w-7 items-center justify-center rounded-full bg-cursor-bg text-accent"><Sparkles size={14} aria-hidden="true" /></span>
        <div class="min-w-0">
          <h4 class="text-sm font-semibold text-fg">{i18n.tk("ai", "AI Analysis")}</h4>
          {#if view.detected}<TypeBadge kind={view.detected.kind} />{/if}
        </div>
      </div>
      <div class="flex items-center gap-1">
        {#if view.state === "error" && view.can_retry}
          <IconButton label={i18n.tk("ai", "Retry")} icon={RefreshCw} onclick={() => onintent({ type: "retry" })} />
        {/if}
        {#if view.has_data}
          <IconButton label={i18n.tk("ai", "Copy to clipboard")} icon={copied ? Check : Copy} onclick={copy} />
          <ExportMenu
            busy={view.export_busy}
            error={view.export_error}
            onexport={(format) => onintent({ type: "export", format })}
            ondismiss={() => onintent({ type: "dismiss_export_error" })}
          />
        {/if}
      </div>
    </div>

    <div bind:this={scroller} class="flex-1 overflow-y-auto p-3">
      {#if view.state === "loading" && !view.has_data}
        {@render progress()}
      {/if}

      {#if view.state === "error" || view.state === "cancelled"}
        <div role="alert" class="flex items-start gap-2 rounded-lg border border-error bg-error-bg p-3">
          <CircleAlert size={18} aria-hidden="true" class="shrink-0 text-error" />
          <div class="min-w-0">
            <p class="text-sm font-medium text-fg">
              {i18n.tk("ai", view.state === "cancelled" ? "Summary generation cancelled" : "Failed to generate summary")}
            </p>
            {#if view.error_detail}<p class="mt-1 break-words text-xs text-muted">{view.error_detail}</p>{/if}
            {#if view.error_hint}<p class="mt-1 text-xs text-muted">{i18n.t(view.error_hint)}</p>{/if}
          </div>
        </div>
      {/if}

      {#if view.state === "interrupted"}
        <div role="status" class="mb-3 flex items-center gap-2 rounded-lg border border-progress bg-notice-bg p-3">
          <CircleAlert size={18} aria-hidden="true" class="shrink-0 text-progress" />
          <p class="min-w-0 flex-1 text-sm text-fg">
            {i18n.tk("ai", "Analysis incomplete.")}
            <span class="text-muted">{i18n.tk("ai", view.has_data ? "Partial results shown." : "No data received.")}</span>
          </p>
          {#if view.can_retry}
            <Button size="sm" onclick={() => onintent({ type: "retry" })}>
              <RefreshCw size={14} aria-hidden="true" />
              {i18n.tk("ai", "Retry")}
            </Button>
          {/if}
        </div>
      {/if}

      {#if view.has_data}
        <AnalysisContent summary={view.summary} analysis={view.analysis} compact oncopy={(section) => onintent({ type: "copy_section", section })} />
        {#if view.state === "loading"}
          <div class="mt-4">{@render progress()}</div>
        {/if}
      {/if}

      {#if view.no_transcript}
        <p class="py-8 text-center text-sm text-muted">{i18n.tk("ai", "No transcript content available")}</p>
      {:else if view.state === "idle"}
        <div role="status" class="flex items-center justify-center gap-2 py-8 text-sm text-muted">
          <LoaderCircle size={16} aria-hidden="true" class="animate-spin" />
          <span>{i18n.tk("ai", "Preparing summary...")}</span>
        </div>
      {/if}
    </div>
  </div>
</AiGate>
