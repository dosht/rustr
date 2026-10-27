<script lang="ts">
  import type { UploadQueueVm } from "$lib/core/types/UploadQueueVm";
  import type { UploadIntent } from "$lib/core/types/UploadIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import UploadAlerts from "../shared/UploadAlerts.svelte";
  import UploadConfiguration from "../shared/UploadConfiguration.svelte";
  import UploadFileRow from "../shared/UploadFileRow.svelte";
  import YouTubeDialog from "../shared/YouTubeDialog.svelte";
  interface Props { vm: UploadQueueVm; fullName: string; youtube: boolean; onpick: (files: File[]) => void; ondrop: (files: File[]) => void; onIntent: (intent: UploadIntent) => void; onpurchase: () => void; }
  let { vm, fullName, youtube, onpick, ondrop, onIntent, onpurchase }: Props = $props();
  let input: HTMLInputElement | undefined = $state();
  let dragging = $state(false);
</script>
<section class="mx-auto max-w-4xl space-y-6 p-4 md:p-8">
  <h1 class="text-2xl font-semibold">{i18n.tk("common", "welcome-message", { fullname: fullName, transgate: "Transgate" }).replace(/<\/?transgate>/g, "")}</h1>
  <UploadAlerts {vm} {onIntent} {onpurchase} />
  {#if youtube}<aside class="rounded-lg bg-notice-bg p-3 text-sm"><strong>{i18n.tk("common", "Analyze YouTube Videos")}</strong> · {i18n.tk("common", "Beta")}<p>{i18n.tk("common", "YouTube transcription is free during beta — no credits required.")}</p></aside>{/if}
  <div class="flex gap-2" role="group" aria-label={i18n.tk("common", "Transcription")}>
    <Button variant={vm.mode === "transcription" ? "primary" : "secondary"} aria-pressed={vm.mode === "transcription"} onclick={() => onIntent({ type: "tab_changed", mode: "transcription" })}>{i18n.tk("common", "Transcription")}</Button>
    <Button variant={vm.mode === "translation" ? "primary" : "secondary"} aria-pressed={vm.mode === "translation"} onclick={() => onIntent({ type: "tab_changed", mode: "translation" })}>{i18n.tk("common", "Translation")}</Button>
  </div>
  <section class={["space-y-5 rounded-xl border-2 border-dashed p-6", dragging ? "border-accent bg-notice-bg" : "border-border bg-surface"]}
    aria-label={i18n.tk("common", "Upload files")}
    ondragover={(event) => { event.preventDefault(); dragging = true; }}
    ondragleave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) dragging = false; }}
    ondrop={(event) => { event.preventDefault(); dragging = false; ondrop(Array.from(event.dataTransfer?.files ?? [])); }}>
    <input bind:this={input} data-testid="upload-files" type="file" multiple accept={vm.picker_accept} class="sr-only" tabindex="-1" aria-label={i18n.tk("common", "Upload files")} onchange={(event) => { onpick(Array.from(event.currentTarget.files ?? [])); event.currentTarget.value = ""; }} />
    {#if vm.initializing}<p role="status">{i18n.tk("common", "Loading...")}</p>{/if}
    {#if vm.rows.length}
      <ul class="space-y-3">{#each vm.rows as row (row.id)}<UploadFileRow {row} onremove={(file) => onIntent({ type: "removed", file })} />{/each}</ul>
    {:else}
      <div class="py-12 text-center"><h2 class="text-xl">{i18n.tk("common", vm.mode === "translation" ? "Drop your audio files here to translate" : "Drop your audio files here to transcribe")}</h2><p class="mt-2 text-muted">{i18n.tk("common", "Any audio or video file")}</p></div>
    {/if}
    {#if vm.show_language_selector}<UploadConfiguration {vm} {onIntent} />{/if}
    {#if !vm.max_files_reached}
      <div class="flex justify-center"><Menu label={i18n.tk("common", "Upload files")} trigger={addIcon} items={[{ id: "file", label: i18n.tk("common", "Upload files") }, ...(youtube ? [{ id: "youtube", label: i18n.tk("common", "Add from YouTube") }] : [])]} onselect={(id) => id === "file" ? input?.click() : onIntent({ type: "you_tube_dialog_opened" })} /></div>
    {/if}
  </section>
  <Button data-testid="upload-start" variant="primary" class="w-full" disabled={vm.start_disabled} busy={vm.transcribing} onclick={() => onIntent({ type: "start_pressed" })}>{i18n.tk("common", vm.mode === "translation" ? "Start Translating" : "Start Transcribing")}</Button>
  <YouTubeDialog vm={vm.youtube_dialog} {onIntent} />
</section>
{#snippet addIcon()}<Plus size={24} aria-hidden="true" />{/snippet}
