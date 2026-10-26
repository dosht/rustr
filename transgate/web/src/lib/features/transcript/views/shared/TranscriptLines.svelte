<!--
  Transcript lines shared by the desktop and phone transcript views (WEB-017..024;
  TranscriptLinesView.tsx, TranscriptLineView.tsx, EditTranscript.tsx, FreemiumLimitsBanner.tsx).
  Every rule comes from core's TranscriptVm: visible rows, tabs, fades, RTL, toggles, speaker
  labels, empty/loading/error states and the freemium banner. The active line is the player's
  current position (`activeIndex`), a UI mechanic; editing uses a textarea so IME composition is safe.
-->
<script lang="ts">
  import { untrack } from "svelte";
  import ArrowUpCircle from "@lucide/svelte/icons/circle-arrow-up";
  import Check from "@lucide/svelte/icons/check";
  import Pencil from "@lucide/svelte/icons/pencil";
  import X from "@lucide/svelte/icons/x";
  import type { LanguageTab } from "$lib/core/types/LanguageTab";
  import type { LineRow } from "$lib/core/types/LineRow";
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Progress from "$lib/ui/Progress.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import Tabs from "$lib/ui/Tabs.svelte";
  import SpeakerMenu from "./SpeakerMenu.svelte";

  interface Props {
    vm: TranscriptVm;
    /** Index into `vm.rows` of the line being played, -1 for none. */
    activeIndex: number;
    ontab: (tab: LanguageTab) => void;
    onedit: (line: number) => void;
    oncanceledit: () => void;
    onsubmitedit: (text: string) => void;
    onspeaker: (line: number, speaker: string) => void;
    onrename: (from: string, to: string) => void;
    ontime: (line: number) => void;
    onupgrade: () => void;
  }

  let { vm, activeIndex, ontab, onedit, oncanceledit, onsubmitedit, onspeaker, onrename, ontime, onupgrade }: Props = $props();

  let draft = $state("");
  const editingId = $derived(vm.rows.find((row) => row.editing)?.id ?? null);
  $effect(() => {
    const id = editingId;
    if (id !== null) draft = untrack(() => vm.rows.find((row) => row.id === id)?.text ?? "");
  });

  const tabId = (tab: LanguageTab) => `${tab.language}|${tab.translation_status}`;
  const selectedId = $derived(vm.selected ? tabId(vm.selected) : vm.tabs[0] ? tabId(vm.tabs[0].tab) : "");
  const fade = $derived(new Map(vm.fades.map(([index, opacity]) => [index, opacity])));

  function startEdit(row: LineRow) {
    draft = row.text;
    onedit(row.id);
  }

  function keydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      onsubmitedit(draft);
    } else if (event.key === "Escape") {
      event.preventDefault();
      oncanceledit();
    }
  }
</script>

{#snippet lines()}
  {#if vm.rows.length === 0 && vm.loading_message}
    <div class="p-4" role="status">
      <p class="mb-3 text-sm text-muted">{i18n.t(vm.loading_message)}</p>
      <Skeleton label={i18n.t(vm.loading_message)} lines={4} />
    </div>
  {/if}

  {#if vm.rows.length > 0}
    <ol class="rounded-lg" dir={vm.rtl ? "rtl" : "ltr"}>
      {#each vm.rows as row, index (row.id)}
        {@const active = index === activeIndex && !row.editing}
        <li
          class="group relative w-full border-b border-border text-fg last:border-b-0"
          data-testid="transcript-line-{row.id}"
          data-active={active ? "true" : undefined}
          aria-current={active ? "true" : undefined}
        >
          {#if fade.has(index)}
            <!-- The core fade is decorative; readable text and controls never inherit its opacity. -->
            <span aria-hidden="true" class="pointer-events-none absolute inset-0 bg-notice-bg" style:opacity={`${fade.get(index)}%`}></span>
          {/if}
          <div class="relative flex flex-col items-start gap-1 px-2 py-0 text-base sm:py-2 md:flex-row md:px-4 lg:px-6">
            {#if vm.show_speakers && !vm.rtl}{@render speaker(row)}{/if}
            <div class={["relative w-full grow rounded-md", active && "bg-cursor-bg", vm.rtl ? "md:ml-6" : "md:mr-6"]}>
              {#if row.editing}
                <!-- svelte-ignore a11y_autofocus -->
                <textarea
                  class="block w-full resize-y rounded border border-border bg-surface p-2 text-sm sm:text-base"
                  dir={vm.rtl ? "rtl" : "ltr"}
                  aria-label={i18n.tk("app", "p5.transcript.edit_line")}
                  autofocus
                  bind:value={draft}
                  onkeydown={keydown}
                ></textarea>
              {:else}
                <p class={["block w-full p-1 text-sm text-wrap sm:p-2 md:text-base", vm.rtl ? "pl-8 text-right md:pl-0" : "pr-8 text-left md:pr-0"]}>{row.text}</p>
              {/if}
              {#if vm.show_time}
                <button
                  type="button"
                  dir="ltr"
                  class={["block min-h-11 px-2 text-xs text-muted hover:text-accent", vm.rtl ? "ml-auto text-right" : "text-left", row.editing && "pt-3"]}
                  onclick={() => ontime(row.id)}
                >
                  {row.time_text}
                </button>
              {/if}
              {#if row.editing}
                <div class={["mt-2 flex gap-2", vm.rtl && "flex-row-reverse"]}>
                  <IconButton size="sm" icon={X} label={i18n.tk("common", "Cancel")} onclick={oncanceledit} />
                  <IconButton size="sm" icon={Check} label={i18n.tk("app", "p5.transcript.save_line")} onclick={() => onsubmitedit(draft)} />
                </div>
              {:else}
                <span class={["absolute top-1 opacity-100 [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-focus-within:opacity-100 [@media(hover:hover)]:group-hover:opacity-100", vm.rtl ? "left-1" : "right-1"]}>
                  <IconButton size="sm" icon={Pencil} label={i18n.tk("app", "p5.transcript.edit_line")} onclick={() => startEdit(row)} />
                </span>
              {/if}
            </div>
            {#if vm.show_speakers && vm.rtl}{@render speaker(row)}{/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}

  {#if vm.empty}
    <p class="p-4 text-center text-muted">{i18n.t(vm.empty)}</p>
  {/if}

  {#if vm.freemium}
    {@render freemium(vm.freemium)}
  {/if}
{/snippet}

{#snippet speaker(row: LineRow)}
  {#if row.speaker_pending}
    <div class="flex shrink-0 flex-col items-end pt-2" aria-hidden="true">
      <div class="h-4 w-16 animate-pulse rounded bg-cursor-bg"></div>
    </div>
  {:else if row.speaker}
    <SpeakerMenu
      label={row.speaker}
      value={row.speaker_value}
      options={vm.speaker_options}
      rtl={vm.rtl}
      onchoose={(value) => onspeaker(row.id, value)}
      {onrename}
    />
  {/if}
{/snippet}

{#snippet freemium(banner: NonNullable<TranscriptVm["freemium"]>)}
  <section class="mt-4 rounded-lg border border-border bg-notice-bg p-4" aria-labelledby="freemium-title">
    <div class="mb-3 flex items-start justify-between gap-3">
      <div>
        <h5 id="freemium-title" class="font-semibold">{i18n.tk("common", "freemiumLimitReached")}</h5>
        <p class="text-xs text-muted">
          {banner.current_minutes} / {banner.max_minutes} {i18n.tk("common", "min")} {i18n.tk("common", "transcriptionProgress").toLowerCase()}
        </p>
      </div>
      <button type="button" class="inline-flex items-center gap-1 rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent hover:bg-accent-hover" onclick={onupgrade}>
        <ArrowUpCircle size={16} aria-hidden="true" />
        <span>{i18n.tk("common", "upgradeToPro")}</span>
      </button>
    </div>
    <Progress label={i18n.tk("common", "transcriptionProgress")} value={banner.percent} />
    <p class="my-3 text-sm">{i18n.tk("common", "freemiumLimitMessage")}</p>
    <p class="text-sm font-medium">{i18n.tk("common", "upgradeToUnlock")}:</p>
    <ul class="mt-1 list-inside list-disc text-sm text-muted">
      <li>{i18n.tk("common", "unlimitedTranscriptionTime")}</li>
      <li>{i18n.tk("common", "priorityProcessing")}</li>
    </ul>
  </section>
{/snippet}

{#snippet linesPanel(_tab: string)}
  {@render lines()}
{/snippet}

{#if vm.load_error}
  <div class="p-4 text-error" role="alert">
    {i18n.t(vm.load_error)}
    {#if vm.load_error_detail}{vm.load_error_detail}{/if}
  </div>
{:else if vm.show_tabs}
  <Tabs
    label={i18n.t(vm.lines_tab_label)}
    tabs={vm.tabs.map((option) => ({ id: tabId(option.tab), label: `${option.flag} ${option.name}` }))}
    selected={selectedId}
    onselect={(id) => {
      const option = vm.tabs.find((o) => tabId(o.tab) === id);
      if (option) ontab(option.tab);
    }}
    panel={linesPanel}
  />
{:else}
  {@render lines()}
{/if}
