<script lang="ts">
  import type { YouTubeDialog } from "$lib/core/types/YouTubeDialog";
  import type { UploadIntent } from "$lib/core/types/UploadIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import Button from "$lib/ui/Button.svelte";
  let { vm, phone = false, onIntent }: { vm: YouTubeDialog; phone?: boolean; onIntent: (intent: UploadIntent) => void } = $props();
</script>
<Dialog open={vm.open} title={i18n.tk("common", "Add YouTube Video")} kind={phone ? "sheet" : "dialog"} locked={vm.busy} onclose={() => onIntent({ type: "you_tube_dialog_closed" })}>
  <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); if (!vm.busy) onIntent({ type: "you_tube_submitted" }); }}>
    <p class="text-sm text-muted">{i18n.tk("common", "Paste a YouTube link to transcribe. Videos with available captions are transcribed instantly.")}</p>
    <label class="block space-y-1">
      <span>{i18n.tk("common", "YouTube URL")}</span>
      <input type="text" inputmode="url" class="min-h-11 w-full rounded-md border border-border bg-bg px-3" value={vm.url} disabled={vm.busy} oninput={(event) => onIntent({ type: "you_tube_url_changed", url: event.currentTarget.value })} />
    </label>
    {#if vm.error}<p role="alert" class="text-error">{i18n.t(vm.error)}</p>{/if}
    <div class="flex justify-end gap-2">
      <Button disabled={vm.busy} onclick={() => onIntent({ type: "you_tube_dialog_closed" })}>{i18n.tk("common", "Cancel")}</Button>
      <Button type="submit" variant="primary" busy={vm.busy}>{i18n.tk("common", vm.busy ? "Adding..." : "Add Video")}</Button>
    </div>
  </form>
</Dialog>
