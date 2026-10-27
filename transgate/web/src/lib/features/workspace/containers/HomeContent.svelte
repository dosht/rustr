<script lang="ts">
  import type { SignedInVm } from "$lib/core/types/SignedInVm";
  import type { AppIntent } from "$lib/core/types/AppIntent";
  import type { JobsIntent } from "$lib/core/types/JobsIntent";
  import type { UploadIntent } from "$lib/core/types/UploadIntent";
  import type { MediaPlatform } from "$lib/platform/media";
  import DesktopWorkspace from "../views/desktop/DesktopWorkspace.svelte";
  import MobileWorkspace from "../views/mobile/MobileWorkspace.svelte";
  import DesktopHistoryTable from "$lib/features/history/views/desktop/DesktopHistoryTable.svelte";
  import MobileHistoryList from "$lib/features/history/views/mobile/MobileHistoryList.svelte";
  import TranscriptContent from "$lib/features/transcript/containers/TranscriptContent.svelte";
  interface Props { vm: SignedInVm; dispatch: (intent: AppIntent) => void; media: MediaPlatform; menuOpen?: boolean; }
  let { vm, dispatch, media, menuOpen = $bindable(false) }: Props = $props();
  const job = $derived(vm.transcript?.job ?? null);
  $effect(() => { void job; menuOpen = false; });
  function upload(intent: UploadIntent) { dispatch({ type: "upload", intent }); }
  function files(kind: "picked" | "dropped", selected: File[]) { upload({ type: kind, files: media.files.hold(selected) }); }
  function jobs(intent: JobsIntent) { dispatch({ type: "jobs", intent }); }
  function open(job: number) { dispatch({ type: "navigate", intent: { route: { page: "home", view: { kind: "job", job } } } }); }
  function create() { dispatch({ type: "navigate", intent: { route: vm.new_transcript_route } }); }
</script>
{#if vm.transcript}
  {#key job}<TranscriptContent {vm} {dispatch} {media} bind:menuOpen />{/key}
{:else if vm.route.page === "home" && vm.route.view.kind === "all_transcripts"}
  {#if vm.layouts.frame === "phone"}
    <MobileHistoryList vm={vm.jobs} onopen={open} onnew={create} ondelete={(job) => jobs({ type: "delete_requested", job })} onconfirmdelete={() => jobs({ type: "delete_confirmed" })} oncanceldelete={() => jobs({ type: "delete_canceled" })} onloadmore={() => jobs({ type: "load_more" })} onrefresh={() => jobs({ type: "refresh" })} />
  {:else}
    <!-- MAIN is a bounded flex column; min-h-0 + flex-1 gives this wrapper the remaining height so it scrolls instead of growing past the frame. -->
    <div class="min-h-0 flex-1 overflow-y-auto p-4 md:p-8"><DesktopHistoryTable vm={vm.jobs} onopen={open} ondownload={(job) => jobs({ type: "download_requested", job })} onfeedback={(job) => dispatch({ type: "feedback", intent: { type: "opened", job } })} ondelete={(job) => jobs({ type: "delete_requested", job })} onconfirmdelete={() => jobs({ type: "delete_confirmed" })} oncanceldelete={() => jobs({ type: "delete_canceled" })} onloadmore={() => jobs({ type: "load_more" })} /></div>
  {/if}
{:else if vm.layouts.frame === "phone"}
  <MobileWorkspace vm={vm.queue} fullName={vm.profile?.full_name ?? ""} youtube={vm.capabilities.youtube} onpick={(selected) => files("picked", selected)} ondrop={(selected) => files("dropped", selected)} onIntent={upload} onpurchase={() => dispatch({ type: "purchase_opened" })} />
{:else}
  <DesktopWorkspace vm={vm.queue} fullName={vm.profile?.full_name ?? ""} youtube={vm.capabilities.youtube} onpick={(selected) => files("picked", selected)} ondrop={(selected) => files("dropped", selected)} onIntent={upload} onpurchase={() => dispatch({ type: "purchase_opened" })} />
{/if}
