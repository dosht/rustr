<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import type { TranscriptIntent } from "$lib/core/types/TranscriptIntent";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import PanelRightClose from "@lucide/svelte/icons/panel-right-close";
  import Tabs from "$lib/ui/Tabs.svelte";
  import SplitPane from "$lib/ui/SplitPane.svelte";
  import TranscriptHeader from "../shared/TranscriptHeader.svelte";
  import TranscriptControls from "../shared/TranscriptControls.svelte";
  import TranscriptBody from "../shared/TranscriptBody.svelte";
  interface Props { vm: TranscriptVm; activeIndex: number; showHeader?: boolean; player?: Snippet; analysis?: Snippet; chat?: Snippet; onIntent: (intent: TranscriptIntent) => void; onupgrade: () => void; onfeedback: () => void; onanalysisvisible: (shown: boolean) => void; }
  let { vm, activeIndex, showHeader = true, player, analysis, chat, onIntent, onupgrade, onfeedback, onanalysisvisible }: Props = $props();
  let tab = $state("transcript"), collapsed = $state(false);
  $effect(() => { const shown = tab === "analysis" && !!analysis; untrack(() => onanalysisvisible(shown)); return () => untrack(() => onanalysisvisible(false)); });
  const tabs = $derived([{ id: "transcript", label: `${vm.lines_tab_flag ?? ""} ${i18n.t(vm.lines_tab_label)}`.trim() }, ...(analysis ? [{ id: "analysis", label: i18n.tk("ai", "AI Analysis") }] : [])]);
</script>
<div class="flex h-full min-h-0 flex-col">
  {#if showHeader}<TranscriptHeader {vm} />{/if}
  {#if player}<div class="border-b border-border p-4">{@render player()}</div>{/if}
  <div class="border-b border-border p-3"><TranscriptControls {vm} {onIntent} {onfeedback} /></div>
  <div class="min-h-0 flex-1">
    {#if chat}
      <SplitPane separatorLabel={i18n.tk("ai", "Ask AI")} primary={{ defaultSize: 60, minSize: 30 }} secondary={{ defaultSize: 35, minSize: 20, collapsedSize: 3 }} {collapsed} oncollapsedchange={(next) => (collapsed = next)} first={main} second={chatPane} />
    {:else}{@render main()}{/if}
  </div>
</div>
{#snippet main()}
  <div class="transcript-tabs h-full px-3 pb-6"><Tabs label={i18n.t(vm.lines_tab_label)} {tabs} selected={tab} onselect={(id) => (tab = id)} panel={panel} /></div>
{/snippet}
{#snippet panel(id: string)}
  {#if id === "analysis" && analysis}{@render analysis()}{:else}<TranscriptBody {vm} {activeIndex} {onIntent} {onupgrade} />{/if}
{/snippet}
{#snippet chatPane()}
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex justify-end"><IconButton size="sm" icon={collapsed ? MessageSquare : PanelRightClose} label={i18n.tk("ai", collapsed ? "Open Ask AI panel" : "Collapse Ask AI panel")} aria-expanded={!collapsed} onclick={() => (collapsed = !collapsed)} /></div>
    {#if !collapsed && chat}<div class="min-h-0 flex-1">{@render chat()}</div>{/if}
  </div>
{/snippet}
<style>
  .transcript-tabs > :global(div) { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .transcript-tabs > :global(div) > :global([role="tabpanel"]) { flex: 1; min-height: 0; overflow-y: auto; }
</style>
