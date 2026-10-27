<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Upload from "@lucide/svelte/icons/upload";
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Badge from "$lib/ui/Badge.svelte";
  let { vm }: { vm: TranscriptVm } = $props();
  // Core projects the status, tone, glyph and spinner (StatusBadge); the view only draws them.
</script>
<header class="flex flex-wrap items-center justify-between gap-3 border-b border-border p-4">
  <div class="min-w-0"><h1 class="truncate text-xl font-semibold" title={vm.title}>{vm.title}</h1>{#if vm.duration_minutes !== null}<p class="text-sm text-muted">{vm.duration_minutes} {i18n.t(vm.minutes_unit)}</p>{/if}</div>
  <div class="flex items-center gap-2">
    {#if vm.is_translation && vm.translation_flag}<span>{vm.translation_flag}</span>{/if}
    {#if vm.status_badge}
      <Badge tone={vm.status_badge.tone} spinner={vm.status_badge.spinning}>
        {#if vm.status_badge.icon === "check"}<Check size={12} aria-hidden="true" />{:else if vm.status_badge.icon === "cross"}<X size={12} aria-hidden="true" />{:else if vm.status_badge.icon === "upload"}<Upload size={12} aria-hidden="true" />{:else if vm.status_badge.icon === "dot"}<span class="size-2 rounded-full bg-current" aria-hidden="true"></span>{/if}
        {i18n.t(vm.status_badge.label)}
      </Badge>
    {/if}
  </div>
</header>
