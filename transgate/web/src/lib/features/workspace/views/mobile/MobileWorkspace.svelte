<script lang="ts">
  import type { UploadQueueVm } from "$lib/core/types/UploadQueueVm";
  import type { UploadIntent } from "$lib/core/types/UploadIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Button from "$lib/ui/Button.svelte";
  import UploadAlerts from "../shared/UploadAlerts.svelte";
  import UploadConfiguration from "../shared/UploadConfiguration.svelte";
  import UploadFileRow from "../shared/UploadFileRow.svelte";
  import YouTubeDialog from "../shared/YouTubeDialog.svelte";
  interface Props { vm: UploadQueueVm; fullName: string; youtube: boolean; onpick: (files: File[]) => void; ondrop: (files: File[]) => void; onIntent: (intent: UploadIntent) => void; onpurchase: () => void; }
  let { vm, youtube, onpick, onIntent, onpurchase }: Props = $props();
  let input: HTMLInputElement | undefined = $state();
  let hintDismissed = $state(false);
  $effect(() => {
    if (!vm.rows.length || hintDismissed) return;
    const timer = setTimeout(() => (hintDismissed = true), 3000);
    return () => clearTimeout(timer);
  });
  $effect(() => { if (vm.alerts.length || vm.youtube_error) window.scrollTo({ top: 0, behavior: "smooth" }); });
</script>
<section class="space-y-4 p-4 pb-28">
  <UploadAlerts {vm} {onIntent} {onpurchase} />
  {#if vm.rows.length}<p class="text-sm"><strong>{i18n.tk("common", "Total Duration:")}</strong> {vm.total_minutes === null ? i18n.tk("common", "Calculating...") : i18n.tk("app", "p5.duration.minutes", { minutes: vm.total_minutes })}</p>{/if}
  {#if youtube}<aside class="hidden rounded-lg bg-notice-bg p-3 text-sm sm:block">{i18n.tk("common", "YouTube transcription is free during beta — no credits required.")}</aside>{/if}
  <input bind:this={input} data-testid="upload-files" type="file" multiple accept={vm.picker_accept} class="sr-only" tabindex="-1" aria-label={i18n.tk("common", "Upload files")} onchange={(event) => { onpick(Array.from(event.currentTarget.files ?? [])); event.currentTarget.value = ""; }} />
  {#if !vm.max_files_reached}
    <div class="grid grid-cols-1 gap-3">
      <Button onclick={() => input?.click()}>{i18n.tk("common", "Upload files")}</Button>
      {#if youtube}<Button onclick={() => onIntent({ type: "you_tube_dialog_opened" })}>{i18n.tk("common", "Add from YouTube")}</Button>{/if}
    </div>
  {/if}
  {#if vm.rows.length}
    <h2 class="font-medium">{i18n.tk("common", "Files")}</h2>
    <ul class="space-y-2">{#each vm.rows as row (row.id)}<UploadFileRow {row} phone onremove={(file) => onIntent({ type: "removed", file })} />{/each}</ul>
    {#if !hintDismissed}<p class="text-xs text-muted">{i18n.tk("common", "Swipe left to delete")}</p>{/if}
    <UploadConfiguration {vm} phone {onIntent} />
  {:else if vm.initializing}
    <p role="status" class="py-12 text-center">{i18n.tk("common", "Loading...")}</p>
  {:else}
    <div class="py-12 text-center"><h2 class="font-medium">{i18n.tk("common", "No files added yet")}</h2><p class="text-sm text-muted">{i18n.tk("common", "Upload audio files or record directly to get started")}</p></div>
  {/if}
  {#if vm.rows.length}
    <div class="fixed inset-x-0 bottom-0 z-10 border-t border-border bg-surface p-4 pb-[calc(1rem+env(safe-area-inset-bottom))]">
      <Button data-testid="upload-start" variant="primary" class="w-full" disabled={vm.start_disabled} busy={vm.transcribing} onclick={() => onIntent({ type: "start_pressed" })}>{i18n.tk("common", vm.transcribing ? "Starting..." : vm.files_ready ? vm.mode === "translation" ? "Start Translating" : "Start Transcribing" : "Preparing files...")}</Button>
    </div>
  {/if}
  <YouTubeDialog vm={vm.youtube_dialog} phone {onIntent} />
</section>
