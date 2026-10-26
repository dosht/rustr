<script lang="ts">
  // Source GenericAnalysis: key highlights only (the typed generic shape has
  // no discussion points); no copy button.
  import Lightbulb from "@lucide/svelte/icons/lightbulb";
  import type { GenericAnalysis } from "$lib/core/types/GenericAnalysis";
  import { i18n } from "$lib/i18n/index.svelte";

  let { data }: { data: GenericAnalysis } = $props();
  const highlights = $derived(data.highlights ?? []);
</script>

{#if highlights.length > 0}
  <section data-testid="generic-analysis">
    <h3 class="mb-3 flex items-center gap-2 text-sm font-semibold text-fg">
      <Lightbulb size={16} aria-hidden="true" class="text-progress" />
      {i18n.tk("ai", "Key Highlights")}
    </h3>
    <ul class="space-y-2">
      {#each highlights as highlight, i (i)}
        <li class="flex gap-2 text-sm text-muted"><span aria-hidden="true" class="text-progress">•</span><span>{highlight}</span></li>
      {/each}
    </ul>
  </section>
{/if}
