<script lang="ts">
  import type { UploadQueueVm } from "$lib/core/types/UploadQueueVm";
  import type { UploadIntent } from "$lib/core/types/UploadIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Button from "$lib/ui/Button.svelte";
  let { vm, onIntent, onpurchase }: { vm: UploadQueueVm; onIntent: (intent: UploadIntent) => void; onpurchase: () => void } = $props();
</script>
<div class="space-y-3">
  {#each vm.alerts as alert (alert)}
    <section role="alert" class="rounded-lg border border-border bg-notice-bg p-4">
      {#if alert === "balance"}
        <h2 class="font-semibold">{i18n.tk("common", "Not enough balance.")}</h2>
        <p>{i18n.tk("common", "Your balance is not enough to run this job. Please top up your minutes and try again.")}</p>
        <Button onclick={onpurchase}>{i18n.tk("common", "Add minutes")}</Button>
      {:else if alert === "language"}
        <h2 class="font-semibold">{i18n.tk("common", "Language Selection Required")}</h2>
        <p>{i18n.tk("common", "Please select the audio language before starting transcription.")}</p>
      {:else if alert === "processing_timeout"}
        <h2 class="font-semibold">{i18n.tk("common", "Processing Timeout")}</h2>
        <p>{i18n.tk("common", "File preparation took longer than expected. This could be due to file processing issues or upload problems.")}</p>
        <p>{i18n.tk("common", "Please try uploading the file again. If the problem persists, contact support.")}</p>
      {:else if vm.youtube_error}
        <h2 class="font-semibold">{i18n.tk("common", "YouTube Transcription Failed")}</h2>
        <p>{i18n.t(vm.youtube_error)}</p>
      {/if}
      <div class="mt-2 flex justify-end"><Button onclick={() => onIntent({ type: "dismiss_alert", alert })}>{i18n.tk("common", "OK")}</Button></div>
    </section>
  {/each}
  {#if vm.youtube_error && !vm.alerts.includes("you_tube")}
    <p role="alert" class="rounded-lg border border-border bg-notice-bg p-4 text-error">{i18n.t(vm.youtube_error)}</p>
  {/if}
</div>
