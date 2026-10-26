<script lang="ts">
  import type { LanguageOption } from "$lib/core/types/LanguageOption";
  import type { Msg } from "$lib/core/types/Msg";
  import { i18n } from "$lib/i18n/index.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  interface Props {
    label: string;
    value: string | null;
    options: readonly LanguageOption[];
    preferred: readonly LanguageOption[];
    autoDetect?: Msg | null;
    autoDetectSelected?: boolean;
    phone?: boolean;
    onchange: (language: string | null) => void;
  }
  let { label, value, options, preferred, autoDetect = null, autoDetectSelected = false, phone = false, onchange }: Props = $props();
  let open = $state(false), search = $state("");
  const selected = $derived(options.find((o) => o.language === value));
  const matches = (o: LanguageOption) => o.name.toLocaleLowerCase().includes(search.toLocaleLowerCase());
  const filtered = $derived(options.filter(matches));
  function choose(language: string | null) { open = false; search = ""; onchange(language); }
</script>
<button type="button" class="flex min-h-11 w-full items-center justify-between gap-3 rounded-md border border-border bg-surface px-3 py-2" aria-label={`${label}: ${selected?.name ?? (autoDetect && autoDetectSelected ? i18n.t(autoDetect) : i18n.tk("common", "Choose the language"))}`} onclick={() => (open = true)}>
  <span class="text-sm text-muted">{label}</span>
  <span class:uppercase={phone}>{selected ? `${selected.flag} ${selected.name}` : autoDetect && autoDetectSelected ? i18n.t(autoDetect) : i18n.tk("common", "Choose the language")}</span>
</button>
<Dialog {open} title={label} kind={phone ? "sheet" : "dialog"} onclose={() => { open = false; search = ""; }}>
  <input type="search" class="min-h-11 rounded-md border border-border bg-bg px-3" aria-label={i18n.tk("common", "Search languages...")} placeholder={i18n.tk("common", "Search languages...")} bind:value={search} />
  <div class="max-h-80 overflow-y-auto">
    {#if autoDetect && !search}
      <button type="button" class="min-h-11 w-full rounded px-3 text-left hover:bg-cursor-bg" aria-pressed={autoDetectSelected} onclick={() => choose(null)}>{i18n.t(autoDetect)}</button>
    {/if}
    {#if !search && preferred.length}
      <ul class="mb-2 border-b border-border pb-2">
        {#each preferred as option (option.language)}
          <li><button type="button" class="min-h-11 w-full rounded px-3 text-left hover:bg-cursor-bg" aria-pressed={value === option.language} onclick={() => choose(option.language)}>{option.flag} {option.name}</button></li>
        {/each}
      </ul>
    {/if}
    <ul>
      {#each filtered as option (option.language)}
        <li><button type="button" class="min-h-11 w-full rounded px-3 text-left hover:bg-cursor-bg" aria-pressed={value === option.language} onclick={() => choose(option.language)}>{option.flag} {option.name}</button></li>
      {/each}
    </ul>
    {#if !filtered.length}<p class="p-3 text-muted">{i18n.tk("common", "No languages found")}</p>{/if}
  </div>
</Dialog>
