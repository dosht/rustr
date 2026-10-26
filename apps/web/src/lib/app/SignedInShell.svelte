<script lang="ts">
  import type { AppIntentDto, SignedInView } from "../core";
  import AudioSummaryRoute from "../audio-summary/AudioSummaryRoute.svelte";
  import DesktopFrame from "./views/desktop/DesktopFrame.svelte";
  import MobileFrame from "./views/mobile/MobileFrame.svelte";
  interface Props { view: SignedInView; mobile: boolean; apiUrl: string; onIntent: (intent: AppIntentDto) => Promise<void> | void }
  let { view, mobile, apiUrl, onIntent }: Props = $props();
  // Selection of a shell-only sample surface is UI mechanics, not a core rule.
  let audioOpen = $state(false);
</script>
<nav class="sample-features" aria-label="Sample features">
  <button aria-pressed={!audioOpen} onclick={() => (audioOpen = false)}>Tasks and members</button>
  <button aria-pressed={audioOpen} onclick={() => (audioOpen = true)}>Audio summaries</button>
</nav>
{#if audioOpen}
  <header><span>{view.email}</span><button onclick={() => onIntent({ type: "SignOutRequested" })}>Sign out</button></header>
  <main><AudioSummaryRoute {apiUrl} {mobile} /></main>
{:else if mobile}<MobileFrame {view} {onIntent} />
{:else}<DesktopFrame {view} {onIntent} />{/if}
<style>
  .sample-features { display: flex; flex-wrap: wrap; gap: 8px; padding: 8px 16px; border-bottom: 1px solid var(--border); }
  button { min-height: 48px; }
  button[aria-pressed="true"] { outline: 2px solid var(--accent); outline-offset: -2px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px 16px; border-bottom: 1px solid var(--border); }
  header span { overflow-wrap: anywhere; min-width: 0; }
</style>
