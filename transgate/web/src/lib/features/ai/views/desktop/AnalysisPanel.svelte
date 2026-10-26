<script lang="ts">
  // Desktop AI Analysis tab (source DesktopSummaryView): pure, props in and
  // intents out. Only the two-second copied mark is local.
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Copy from "@lucide/svelte/icons/copy";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
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
    onconsent: (intent: ConsentIntent) => void;
    onintent: (intent: AnalysisIntent) => void;
  }

  let { consent, view, onconsent, onintent }: Props = $props();
  let copied = $state(false);

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
  <div class="space-y-4" data-testid="analysis-progress">
    <div class="space-y-1">
      <p class="text-sm text-muted">{view.progress ? i18n.t(view.progress.stage) : i18n.tk("ai", "Analyzing...")}</p>
      <Progress label={view.progress ? i18n.t(view.progress.stage) : i18n.tk("ai", "Analyzing...")} value={view.progress?.percentage ?? 0} />
    </div>
    <Skeleton label={i18n.tk("ai", "Analyzing...")} lines={4} />
  </div>
{/snippet}

<AiGate {consent} feature="summary" onintent={onconsent}>
  <div class="flex h-full flex-col" data-testid="analysis-panel">
    <div class="flex items-center justify-between gap-2 border-b border-border px-4 py-3">
      <div class="flex min-w-0 items-center gap-2">
        <h3 class="text-sm font-semibold text-fg">{i18n.tk("ai", "AI Analysis")}</h3>
        {#if view.detected}<TypeBadge kind={view.detected.kind} />{/if}
      </div>
      <div class="flex items-center gap-1">
        {#if view.state === "error" && view.can_retry}
          <IconButton label={i18n.tk("ai", "Retry")} icon={RefreshCw} size="sm" onclick={() => onintent({ type: "retry" })} />
        {/if}
        {#if view.has_data}
          <IconButton label={i18n.tk("ai", "Copy to clipboard")} icon={copied ? Check : Copy} size="sm" onclick={copy} />
          <ExportMenu
            busy={view.export_busy}
            error={view.export_error}
            onexport={(format) => onintent({ type: "export", format })}
            ondismiss={() => onintent({ type: "dismiss_export_error" })}
          />
        {/if}
      </div>
    </div>

    <div class="flex-1 overflow-y-auto p-4">
      {#if view.state === "loading" && !view.has_data}
        {@render progress()}
      {/if}

      {#if view.state === "error"}
        <div role="alert" class="flex items-start gap-3 rounded-lg border border-error bg-error-bg p-4">
          <CircleAlert size={20} aria-hidden="true" class="shrink-0 text-error" />
          <div class="min-w-0">
            <p class="text-sm font-medium text-fg">{i18n.tk("ai", "Failed to generate summary")}</p>
            {#if view.error_detail}<p class="mt-1 break-words text-sm text-muted">{view.error_detail}</p>{/if}
            {#if view.error_hint}<p class="mt-1 text-sm text-muted">{i18n.t(view.error_hint)}</p>{/if}
          </div>
        </div>
      {/if}

      {#if view.state === "interrupted"}
        <div role="status" class="mb-4 flex items-start gap-3 rounded-lg border border-progress bg-notice-bg p-4">
          <CircleAlert size={20} aria-hidden="true" class="shrink-0 text-progress" />
          <div class="min-w-0 flex-1">
            <p class="text-sm font-medium text-fg">{i18n.tk("ai", "Analysis was interrupted.")}</p>
            <p class="mt-1 text-sm text-muted">{i18n.tk("ai", "The analysis did not complete. Any sections that finished are shown below.")}</p>
          </div>
          {#if view.can_retry}
            <Button size="sm" onclick={() => onintent({ type: "retry" })}>
              <RefreshCw size={14} aria-hidden="true" />
              {i18n.tk("ai", "Retry")}
            </Button>
          {/if}
        </div>
      {/if}

      {#if view.has_data}
        <AnalysisContent summary={view.summary} analysis={view.analysis} oncopy={(section) => onintent({ type: "copy_section", section })} />
        {#if view.state === "loading"}
          <div class="mt-6">{@render progress()}</div>
        {/if}
      {/if}

      {#if view.no_transcript}
        <p class="py-8 text-center text-sm text-muted">{i18n.tk("ai", "No transcript content available")}</p>
      {/if}
    </div>
  </div>
</AiGate>
