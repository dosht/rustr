<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Upload from "@lucide/svelte/icons/upload";
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import type { JobStatus } from "$lib/core/types/JobStatus";
  import { i18n } from "$lib/i18n/index.svelte";
  let { vm }: { vm: TranscriptVm } = $props();
  // Visual mapping only. Status and spinner are core projections, never inferred transitions.
  const color: Record<JobStatus, string> = { draft: "text-muted", not_started: "text-muted", pending: "text-accent", running: "text-progress", completed: "text-success", failed: "text-error", canceled: "text-muted", deleted: "text-muted", speaker_labeling: "text-accent", uploaded: "text-progress", processing: "text-accent", ready: "text-success" };
</script>
<header class="flex flex-wrap items-center justify-between gap-3 border-b border-border p-4">
  <div class="min-w-0"><h1 class="truncate text-xl font-semibold" title={vm.title}>{vm.title}</h1>{#if vm.duration_minutes !== null}<p class="text-sm text-muted">{vm.duration_minutes} {i18n.t(vm.minutes_unit)}</p>{/if}</div>
  <div class="flex items-center gap-2">
    {#if vm.is_translation && vm.translation_flag}<span>{vm.translation_flag}</span>{/if}
    {#if vm.status_badge}
      <span class={["inline-flex items-center gap-1 rounded-full bg-cursor-bg px-2 py-1 text-xs font-medium", color[vm.status_badge.status]]}>
        {#if vm.status_badge.spinning}<span class="size-3 animate-spin rounded-full border-2 border-current border-t-transparent" aria-hidden="true"></span>{:else if vm.status_badge.status === "completed" || vm.status_badge.status === "ready"}<Check size={12} aria-hidden="true" />{:else if vm.status_badge.status === "failed"}<X size={12} aria-hidden="true" />{:else if vm.status_badge.status === "uploaded"}<Upload size={12} aria-hidden="true" />{/if}
        {i18n.t(vm.status_badge.label)}
      </span>
    {/if}
  </div>
</header>
