<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import type { TranscriptIntent } from "$lib/core/types/TranscriptIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import Tabs from "$lib/ui/Tabs.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import TranscriptBody from "../shared/TranscriptBody.svelte";
  import TranscriptControls from "../shared/TranscriptControls.svelte";
  interface Props { vm: TranscriptVm; activeIndex: number; phone?: boolean; menuOpen?: boolean; player?: Snippet; analysis?: Snippet; chat?: Snippet; onIntent: (intent: TranscriptIntent) => void; onupgrade: () => void; onfeedback: () => void; onanalysisvisible: (shown: boolean) => void; }
  let { vm, activeIndex, phone = true, menuOpen = $bindable(false), player, analysis, chat, onIntent, onupgrade, onfeedback, onanalysisvisible }: Props = $props();
  let tab = $state("transcript");
  const tabs = $derived([{ id: "transcript", label: i18n.t(vm.lines_tab_label) }, ...(analysis ? [{ id: "analysis", label: i18n.tk("ai", "AI Analysis") }] : []), ...(chat ? [{ id: "chat", label: i18n.tk("ai", "Ask AI") }] : [])]);
  $effect(() => { const shown = tab === "analysis" && !!analysis; untrack(() => onanalysisvisible(shown)); return () => untrack(() => onanalysisvisible(false)); });
</script>
<div class="flex h-full min-h-0 flex-col">
  {#if !phone}<div class="border-b border-border p-3"><TranscriptControls {vm} {onIntent} {onfeedback} /></div>{/if}
  <div class="transcript-tabs min-h-0 flex-1 px-3 pb-4"><Tabs label={i18n.t(vm.lines_tab_label)} {tabs} selected={tab} onselect={(id) => (tab = id)} panel={panel} /></div>
  {#if player}<div class="sticky bottom-0 z-10 shrink-0 border-t border-border bg-surface p-3 pb-[calc(.75rem+env(safe-area-inset-bottom))]">{@render player()}</div>{/if}
</div>
{#snippet panel(id: string)}
  {#if id === "analysis" && analysis}{@render analysis()}{:else if id === "chat" && chat}{@render chat()}{:else}<TranscriptBody {vm} {activeIndex} {onIntent} {onupgrade} />{/if}
{/snippet}
<Dialog open={menuOpen} kind="sheet" title={i18n.tk("app", "More options")} onclose={() => (menuOpen = false)}>
  <TranscriptControls {vm} phone onIntent={(intent) => { onIntent(intent); if (intent.type === "download_pressed") menuOpen = false; }} {onfeedback} />
</Dialog>
<style>
  .transcript-tabs > :global(div) { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .transcript-tabs > :global(div) > :global([role="tabpanel"]) { flex: 1; min-height: 0; overflow-y: auto; }
</style>
