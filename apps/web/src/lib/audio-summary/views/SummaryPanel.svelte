<script lang="ts">
  import type { AudioSummaryView } from "../../core";
  import type { SummaryAction } from "../summaryModel.svelte";
  interface Props { view: AudioSummaryView; onIntent: (intent: SummaryAction) => void | Promise<void> }
  let { view, onIntent }: Props = $props();
  let accepted = $state(false);
  $effect(() => { if (!view.canStart) accepted = false; });
</script>

<section aria-label="Audio work summary">
  <h2>{view.title}</h2>
  <p role="status" aria-live="polite">{view.statusLabel}</p>
  {#if view.error}<p class="error" role="alert">{view.error}</p>{/if}
  {#if view.remoteOutcomeUnknown}
    <p class="warning">Stopping locally does not confirm remote cancellation or deletion.</p>
  {/if}
  <div class="summary" aria-label="Summary text">{view.text}</div>
  {#if view.canStart}
    <div class="disclosure">
      <p>{view.disclosure}</p>
      <label><input type="checkbox" bind:checked={accepted} /> I agree to external AI processing for this summary</label>
      <button disabled={!accepted || view.isLoading} onclick={() => onIntent({ type: "Start", accepted })}>Generate summary</button>
    </div>
  {/if}
  <div class="actions">
    {#if view.canSuspend}<button onclick={() => onIntent({ type: "Suspend" })}>Pause updates</button>{/if}
    {#if view.canResume}<button onclick={() => onIntent({ type: "Resume" })}>Resume updates</button>{/if}
    {#if view.canCancel}<button onclick={() => onIntent({ type: "Cancel" })}>Stop generation</button>{/if}
  </div>
</section>

<style>
  section { min-width: 0; }
  h2 { margin-top: 0; font-size: 1.2rem; }
  .summary { white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.65; min-height: 4rem; }
  .disclosure { margin-top: 24px; padding-top: 16px; border-top: 1px solid var(--border); }
  .disclosure p { color: var(--muted); }
  label { display: flex; align-items: center; gap: 12px; min-height: 48px; }
  input { width: 20px; height: 20px; flex-shrink: 0; }
  button { min-height: 48px; padding: 8px 16px; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 16px; }
  .error { color: var(--error); }
  .warning { color: var(--muted); }
</style>
