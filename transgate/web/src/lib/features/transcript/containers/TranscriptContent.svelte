<script lang="ts">
  import type { SignedInVm } from "$lib/core/types/SignedInVm";
  import type { AppIntent } from "$lib/core/types/AppIntent";
  import type { TranscriptIntent } from "$lib/core/types/TranscriptIntent";
  import type { MediaPlatform } from "$lib/platform/media";
  import { activeLineIndex, type PlayerSnapshot } from "$lib/platform/media/player";
  import type { AnalysisIntent, ChatIntent, ConsentIntent } from "$lib/features/ai/intents";
  import DesktopAnalysis from "$lib/features/ai/views/desktop/AnalysisPanel.svelte";
  import MobileAnalysis from "$lib/features/ai/views/mobile/AnalysisPanel.svelte";
  import DesktopChat from "$lib/features/ai/views/desktop/ChatPanel.svelte";
  import MobileChat from "$lib/features/ai/views/mobile/ChatPanel.svelte";
  import DesktopTranscript from "../views/desktop/DesktopTranscript.svelte";
  import MobileTranscript from "../views/mobile/MobileTranscript.svelte";
  import TranscriptHeader from "../views/shared/TranscriptHeader.svelte";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import Button from "$lib/ui/Button.svelte";
  interface Props { vm: SignedInVm; dispatch: (intent: AppIntent) => void; media: MediaPlatform; menuOpen?: boolean; }
  let { vm, dispatch, media, menuOpen = $bindable(false) }: Props = $props();
  // This container is keyed by job in HomeContent. Only UI draft/scroll/playhead live here.
  let draft = $state(""), scrollTop = $state(0), time = $state(0);
  let lastPlaying = false, analysisShown = false;
  const transcript = $derived(vm.transcript);
  const phone = $derived(vm.layouts.frame === "phone");
  const touchVideo = $derived(transcript?.media.kind === "you_tube" && vm.layouts.youtube === "touch");
  const tabbed = $derived(phone || touchVideo || vm.layouts.transcript === "tabbed");
  const ends = $derived(transcript?.rows.map((row) => row.end_seconds) ?? []);
  const activeIndex = $derived(activeLineIndex(ends, time));
  function intent(value: TranscriptIntent) { dispatch({ type: "transcript", intent: value }); }
  function onPlayerState(next: PlayerSnapshot) {
    time = next.time;
    if (next.playing !== lastPlaying) { lastPlaying = next.playing; intent({ type: "player_state", playing: next.playing }); }
  }
  function analysisVisible(shown: boolean) {
    if (shown === analysisShown) return;
    analysisShown = shown;
    dispatch({ type: "analysis", intent: { type: shown ? "shown" : "hidden" } });
  }
  function consent(value: ConsentIntent) {
    switch (value.type) {
      case "enable": dispatch({ type: "consent", intent: { type: "enable_requested" } }); break;
      case "decline": dispatch({ type: "consent", intent: { type: "decline_requested" } }); break;
      case "disable": dispatch({ type: "consent", intent: { type: "disable_confirmed" } }); break;
      case "dismiss_error": dispatch({ type: "consent", intent: { type: "error_dismissed" } }); break;
    }
  }
  function analysisIntent(value: AnalysisIntent) {
    switch (value.type) {
      case "retry": dispatch({ type: "analysis", intent: { type: "retry_requested" } }); break;
      case "copy": dispatch({ type: "analysis", intent: { type: "copy_requested" } }); break;
      case "copy_section": dispatch({ type: "analysis", intent: { type: "copy_section_requested", section: value.section } }); break;
      case "export": dispatch({ type: "analysis", intent: { type: "export_requested", format: value.format } }); break;
      case "dismiss_export_error": dispatch({ type: "analysis", intent: { type: "export_error_dismissed" } }); break;
    }
  }
  function chatIntent(value: ChatIntent) {
    switch (value.type) {
      case "send": dispatch({ type: "chat", intent: { type: "send_requested", text: value.text } }); break;
      case "retry": dispatch({ type: "chat", intent: { type: "retry_requested", index: value.index } }); break;
      case "copy_message": dispatch({ type: "chat", intent: { type: "copy_message_requested", index: value.index } }); break;
      case "rate": dispatch({ type: "chat", intent: { type: "rate_requested", index: value.index, rating: value.rating } }); break;
      case "copy_chat": dispatch({ type: "chat", intent: { type: "copy_chat_requested" } }); break;
      case "download": dispatch({ type: "chat", intent: { type: "download_requested" } }); break;
      case "dismiss_error": dispatch({ type: "chat", intent: { type: "error_dismissed" } }); break;
      case "dismiss_notice": dispatch({ type: "chat", intent: { type: "notice_dismissed" } }); break;
    }
  }
  function feedback() { if (transcript?.job !== null && transcript?.job !== undefined) dispatch({ type: "feedback", intent: { type: "opened", job: transcript.job } }); }
</script>
{#if transcript}
  <div class="flex h-full min-h-0 flex-col">
    {#if !phone}<div class="order-1"><TranscriptHeader vm={transcript} /></div>{/if}
    <!-- Stable sibling across frame/tab/mode switches: never a second iframe or audio engine. -->
    <div class={["shrink-0 border-border bg-surface p-3", phone || touchVideo ? "order-4 border-t pb-[calc(.75rem+env(safe-area-inset-bottom))]" : "order-2 border-b"]}>
      <MediaPlayer vm={transcript} {media} {phone} touch={vm.layouts.youtube === "touch"} expandedEnabled={vm.capabilities.mobile_expanded_player} onstate={onPlayerState} />
    </div>
    {#if transcript.no_audio_alert}<div role="alert" class="order-2 flex items-center justify-between gap-3 bg-error-bg p-3 text-error"><p>{i18n.t(transcript.no_audio_alert)}</p><Button onclick={() => intent({ type: "no_audio_alert_dismissed" })}>{i18n.tk("common", "Close")}</Button></div>{/if}
    <div class="order-3 min-h-0 flex-1">
      {#if tabbed}
        <MobileTranscript vm={transcript} {phone} {activeIndex} bind:menuOpen onIntent={intent} onupgrade={() => dispatch({ type: "purchase_opened" })} onfeedback={feedback} onanalysisvisible={analysisVisible} analysis={vm.capabilities.ai && vm.analysis ? analysis : undefined} chat={vm.capabilities.ai_chat && vm.chat ? chat : undefined} />
      {:else}
        <DesktopTranscript vm={transcript} showHeader={false} {activeIndex} onIntent={intent} onupgrade={() => dispatch({ type: "purchase_opened" })} onfeedback={feedback} onanalysisvisible={analysisVisible} analysis={vm.capabilities.ai && vm.analysis ? analysis : undefined} chat={vm.capabilities.ai_chat && vm.chat ? chat : undefined} />
      {/if}
    </div>
  </div>
{/if}
{#snippet analysis()}
  {#if vm.analysis}
    {#if vm.layouts.phone_ai}<MobileAnalysis consent={vm.consent} view={vm.analysis} scrollResetKey={transcript?.job ?? 0} onconsent={consent} onintent={analysisIntent} />
    {:else}<DesktopAnalysis consent={vm.consent} view={vm.analysis} onconsent={consent} onintent={analysisIntent} />{/if}
  {/if}
{/snippet}
{#snippet chat()}
  {#if vm.chat}
    {#if vm.layouts.phone_ai}<MobileChat consent={vm.consent} view={vm.chat} bind:draft scrollResetKey={transcript?.job ?? 0} onconsent={consent} onintent={chatIntent} />
    {:else}<DesktopChat consent={vm.consent} view={vm.chat} bind:draft bind:scrollTop onconsent={consent} onintent={chatIntent} />{/if}
  {/if}
{/snippet}
