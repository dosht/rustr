<script lang="ts">
  import type { SignedInVm } from "$lib/core/types/SignedInVm";
  import type { AppIntent } from "$lib/core/types/AppIntent";
  import type { MediaPlatform } from "$lib/platform/media";
  import DesktopSidebar from "../views/desktop/DesktopSidebar.svelte";
  import MobileRecent from "../views/mobile/MobileRecent.svelte";
  interface Props { vm: SignedInVm; dispatch: (intent: AppIntent) => void; media: MediaPlatform; collapsed?: boolean; }
  let { vm, dispatch, collapsed = false }: Props = $props();
  function open(job: number) { dispatch({ type: "navigate", intent: { route: { page: "home", view: { kind: "job", job } } } }); }
</script>
{#if vm.layouts.frame === "phone"}
  <MobileRecent vm={vm.jobs} onopen={open} />
{:else}
  <DesktopSidebar vm={vm.jobs} {collapsed} onopen={open} showConfirmation={!(vm.route.page === "home" && vm.route.view.kind === "all_transcripts")}
    onnew={() => dispatch({ type: "navigate", intent: { route: vm.new_transcript_route } })}
    onall={() => dispatch({ type: "navigate", intent: { route: { page: "home", view: { kind: "all_transcripts" } } } })}
    onfeedback={(job) => dispatch({ type: "feedback", intent: { type: "opened", job } })}
    ondelete={(job) => dispatch({ type: "jobs", intent: { type: "delete_requested", job } })}
    onconfirmdelete={() => dispatch({ type: "jobs", intent: { type: "delete_confirmed" } })}
    oncanceldelete={() => dispatch({ type: "jobs", intent: { type: "delete_canceled" } })}
    onloadmore={() => dispatch({ type: "jobs", intent: { type: "load_more" } })} />
{/if}
