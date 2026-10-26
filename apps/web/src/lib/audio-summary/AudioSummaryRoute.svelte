<script lang="ts">
  import { onMount } from "svelte";
  import { loadAudioSummary } from "../core";
  import AudioSummaryContainer from "./AudioSummaryContainer.svelte";
  import { createSummaryModel, type SummaryModel } from "./summaryModel.svelte";
  let { apiUrl, mobile }: { apiUrl: string; mobile: boolean } = $props();
  let model = $state<SummaryModel | null>(null);
  let error = $state<string | null>(null);
  onMount(() => {
    let disposed = false;
    void loadAudioSummary(apiUrl).then((core) => {
      if (disposed) { core.dispose(); return; }
      model = createSummaryModel(core);
    }).catch((reason) => { if (!disposed) error = String(reason); });
    return () => { disposed = true; model?.dispose(); };
  });
</script>
{#if error}<p role="alert">{error}</p>
{:else if model}<AudioSummaryContainer {model} {mobile} />
{:else}<p role="status">Loading audio summaries…</p>{/if}
