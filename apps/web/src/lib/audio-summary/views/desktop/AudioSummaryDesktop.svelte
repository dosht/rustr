<script lang="ts">
  import type { AudioSummaryView } from "../../../core";
  import type { SummaryAction } from "../../summaryModel.svelte";
  import SummaryPanel from "../SummaryPanel.svelte";
  interface Props { view: AudioSummaryView; onIntent: (intent: SummaryAction) => void | Promise<void> }
  let { view, onIntent }: Props = $props();
</script>

<section class="audio-summary" aria-label="Audio summaries">
  <header><h1>Audio summaries</h1><button disabled={view.isLoading} onclick={() => onIntent({ type: "Refresh" })}>Refresh audio work</button></header>
  <div class="columns">
    <nav aria-label="Audio work">
      {#each view.works as work (work.id)}
        <button class:selected={work.selected} aria-current={work.selected ? "true" : undefined} onclick={() => onIntent({ type: "Select", id: work.id })}>
          <span>{work.name}</span><small>{work.status}</small>
        </button>
      {:else}<p>No audio work. Completed audio work appears here.</p>{/each}
    </nav>
    {#key view.selectedId}<SummaryPanel {view} {onIntent} />{/key}
  </div>
</section>

<style>
  .audio-summary { padding: 24px; }
  header { display: flex; justify-content: space-between; align-items: center; gap: 16px; margin-bottom: 24px; }
  h1 { margin: 0; font-size: 1.4rem; }
  .columns { display: grid; grid-template-columns: minmax(180px, 260px) minmax(0, 1fr); gap: 32px; }
  nav { display: flex; flex-direction: column; gap: 8px; }
  button { min-height: 48px; }
  nav button { text-align: left; display: flex; flex-direction: column; gap: 4px; background: none; color: var(--fg); border: 1px solid var(--border); }
  nav button.selected { background: var(--cursor-bg); border-color: var(--accent); }
  small { color: var(--muted); }
</style>
