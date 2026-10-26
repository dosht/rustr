<script lang="ts">
  import { onMount } from "svelte";
  import type { SummaryModel } from "./summaryModel.svelte";
  import AudioSummaryDesktop from "./views/desktop/AudioSummaryDesktop.svelte";
  import AudioSummaryMobile from "./views/mobile/AudioSummaryMobile.svelte";
  let { model, mobile }: { model: SummaryModel; mobile: boolean } = $props();
  onMount(() => { void model.send({ type: "Refresh" }); });
</script>

{#if mobile}<AudioSummaryMobile view={model.view} onIntent={model.send} />
{:else}<AudioSummaryDesktop view={model.view} onIntent={model.send} />{/if}
