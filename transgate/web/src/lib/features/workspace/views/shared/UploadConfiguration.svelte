<script lang="ts">
  import type { UploadQueueVm } from "$lib/core/types/UploadQueueVm";
  import type { UploadIntent } from "$lib/core/types/UploadIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import LanguagePicker from "./LanguagePicker.svelte";
  let { vm, phone = false, onIntent }: { vm: UploadQueueVm; phone?: boolean; onIntent: (intent: UploadIntent) => void } = $props();
</script>
<div class="space-y-4">
  {#if phone}
    <Switch label={i18n.tk("common", "Translation")} checked={vm.mode === "translation"} onchange={(on) => onIntent({ type: "tab_changed", mode: on ? "translation" : "transcription" })} />
  {/if}
  <LanguagePicker
    label={i18n.tk("common", "Audio Language")} {phone}
    value={vm.mode === "translation" ? vm.source_language : vm.language?.kind === "language" ? vm.language.language : null}
    options={vm.mode === "translation" ? vm.source_languages : vm.available}
    preferred={phone ? vm.phone_preferred : vm.preferred}
    autoDetect={vm.auto_detect_offered ? vm.auto_detect_label : null}
    autoDetectSelected={vm.language?.kind === "auto_detect"}
    onchange={(language) => onIntent(vm.mode === "translation" ? { type: "source_language_chosen", language } : { type: "language_chosen", choice: language === null ? { kind: "auto_detect" } : { kind: "language", language } })}
  />
  {#if vm.mode === "translation"}
    <LanguagePicker label={i18n.tk("common", "Translation Language")} {phone} value={vm.target_language} options={vm.target_languages} preferred={vm.preferred_targets} onchange={(language) => onIntent({ type: "target_language_chosen", language })} />
  {/if}
  <Switch label={i18n.tk("common", "Speaker Recognition")} description={i18n.tk("common", "Identify different speakers in the transcript")} checked={vm.speakers} onchange={(enabled) => onIntent({ type: "speakers_toggled", enabled })} />
</div>
