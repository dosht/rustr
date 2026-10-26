<script lang="ts">
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import type { TranscriptIntent } from "$lib/core/types/TranscriptIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import Button from "$lib/ui/Button.svelte";
  let { vm, phone = false, onIntent, onfeedback }: { vm: TranscriptVm; phone?: boolean; onIntent: (intent: TranscriptIntent) => void; onfeedback: () => void } = $props();
  let options: HTMLDetailsElement | undefined = $state();
</script>
<div class="flex flex-wrap items-center gap-4">
  <Button disabled={!vm.download_enabled} onclick={() => onIntent({ type: "download_pressed", all_formats: false })}>{i18n.tk("app", "Download Transcript")}</Button>
  {#if phone}
    {@render toggles()}
  {:else}
    {#if vm.can_copy}<Button onclick={() => onIntent({ type: "copy_pressed" })}>{i18n.tk("app", "Copy all transcripts")}</Button>{/if}
    <details bind:this={options} class="relative">
      <summary class="flex min-h-11 cursor-pointer items-center rounded-md border border-border px-3">{i18n.tk("app", "More options")}</summary>
      <div class="absolute right-0 z-30 mt-1 w-64 space-y-3 rounded-lg border border-border bg-surface p-3 shadow-lg">
        {@render toggles()}
        <Button disabled={!vm.download_enabled} onclick={() => { if (options) options.open = false; onIntent({ type: "download_pressed", all_formats: true }); }}>{i18n.tk("common", "Download all formats")}</Button>
        <Button onclick={() => { if (options) options.open = false; onfeedback(); }}>{i18n.tk("app", "Feedback")}</Button>
      </div>
    </details>
  {/if}
</div>
{#snippet toggles()}
  {#if vm.time_toggle}<Switch label={i18n.tk("app", "Show Time")} checked={vm.show_time} onchange={(on) => onIntent({ type: "show_time_toggled", on })} />{/if}
  {#if phone ? vm.phone_speaker_toggle : vm.speaker_toggle}<Switch label={i18n.tk("app", "Show Speaker")} checked={vm.show_speakers} onchange={(on) => onIntent({ type: "show_speakers_toggled", on })} />{/if}
{/snippet}
