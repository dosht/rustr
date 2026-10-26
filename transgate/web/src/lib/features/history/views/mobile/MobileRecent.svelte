<script lang="ts">
  import type { JobListVm } from "$lib/core/types/JobListVm";
  import { i18n } from "$lib/i18n/index.svelte";
  let { vm, onopen }: { vm: JobListVm; onopen: (job: number) => void } = $props();
</script>
<section class="space-y-2 p-3">
  <h2 class="text-xs font-semibold text-muted">{i18n.tk("common", "mobile.navigation.recentSection")}</h2>
  {#if !vm.recent.length}<p class="p-3 text-sm text-muted">{i18n.t(vm.recent_empty)}</p>{/if}
  <ul class="space-y-1">
    {#each vm.recent as row (row.id)}
      <li><button type="button" class={["block min-h-12 w-full rounded-lg p-3 text-left hover:bg-cursor-bg", row.selected && "bg-notice-bg"]} aria-current={row.selected ? "page" : undefined} onclick={() => onopen(row.id)}>
        <span class="block truncate">{row.file_name ?? i18n.t(vm.untitled)}</span>
        <span class="flex gap-2 text-xs text-muted">{#if row.duration}<span>{row.duration}</span>{/if}<span>{new Date(row.created_at_ms).toLocaleDateString()}</span></span>
      </button></li>
    {/each}
  </ul>
</section>
