<script lang="ts">
  import type { AudioSummaryView } from "../../../core";
  import type { SummaryAction } from "../../summaryModel.svelte";
  import SummaryPanel from "../SummaryPanel.svelte";
  interface Props { view: AudioSummaryView; onIntent: (intent: SummaryAction) => void | Promise<void> }
  let { view, onIntent }: Props = $props();
</script>

<section class="audio-summary" aria-label="Audio summaries">
  <h1>Audio summaries</h1>
  <label for="audio-work">Audio work</label>
  <select id="audio-work" value={view.selectedId ?? ""} onchange={(event) => onIntent({ type: "Select", id: event.currentTarget.value })}>
    <option value="" disabled>Select audio work</option>
    {#each view.works as work (work.id)}<option value={work.id}>{work.name} · {work.status}</option>{/each}
  </select>
  <button disabled={view.isLoading} onclick={() => onIntent({ type: "Refresh" })}>Refresh audio work</button>
  {#if !view.works.length}<p>No audio work. Completed audio work appears here.</p>{/if}
  <div class="panel">{#key view.selectedId}<SummaryPanel {view} {onIntent} />{/key}</div>
</section>

<style>
  .audio-summary { padding: 16px; }
  h1 { font-size: 1.4rem; margin-top: 0; }
  label { display: block; margin-bottom: 8px; }
  select { width: 100%; min-height: 48px; padding: 8px; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 6px; }
  button { min-height: 48px; margin-top: 12px; }
  .panel { margin-top: 24px; }
</style>
