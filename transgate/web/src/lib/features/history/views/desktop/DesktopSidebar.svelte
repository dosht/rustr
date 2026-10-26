<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import List from "@lucide/svelte/icons/list";
  import Mic from "@lucide/svelte/icons/mic";
  import Languages from "@lucide/svelte/icons/languages";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import type { JobListVm } from "$lib/core/types/JobListVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import { sentinel } from "../shared/sentinel";
  interface Props { vm: JobListVm; collapsed: boolean; showConfirmation?: boolean; onnew: () => void; onall: () => void; onopen: (job: number) => void; onfeedback: (job: number) => void; ondelete: (job: number) => void; onconfirmdelete: () => void; oncanceldelete: () => void; onloadmore: () => void; }
  let { vm, collapsed, showConfirmation = true, onnew, onall, onopen, onfeedback, ondelete, onconfirmdelete, oncanceldelete, onloadmore }: Props = $props();
</script>
<div class="flex min-h-0 flex-1 flex-col gap-2 p-2">
  <button type="button" class="flex min-h-11 items-center gap-3 rounded-lg bg-accent px-3 text-on-accent" aria-label={i18n.tk("common", "New Transcript")} onclick={onnew}><Plus size={20} aria-hidden="true" />{#if !collapsed}<span>{i18n.tk("common", "New Transcript")}</span>{/if}</button>
  <button type="button" class="flex min-h-11 items-center gap-3 rounded-lg px-3 hover:bg-cursor-bg" aria-label={i18n.tk("app", "All Transcripts")} onclick={onall}><List size={20} aria-hidden="true" />{#if !collapsed}<span>{i18n.tk("app", "All Transcripts")}</span>{/if}</button>
  {#if !collapsed}
    <h2 class="px-3 pt-4 text-xs text-muted">{i18n.tk("app", "Recent transcripts")}</h2>
    <ul class="min-h-0 flex-1 space-y-1 overflow-y-auto">
      {#each vm.rows as row (row.id)}
        <li class={["flex items-center rounded-lg", row.selected && "bg-cursor-bg"]}>
          <button type="button" class="flex min-h-11 min-w-0 flex-1 items-center gap-2 rounded-lg px-3 text-left hover:bg-cursor-bg" aria-current={row.selected ? "page" : undefined} onclick={() => onopen(row.id)}>
            {#if row.spinner}<span class="size-4 shrink-0 animate-spin rounded-full border-2 border-progress border-t-transparent" aria-hidden="true"></span>{:else if row.is_translation}<Languages size={16} aria-hidden="true" />{:else}<Mic size={16} aria-hidden="true" />{/if}
            <span class="truncate" title={row.sidebar_name}>{row.sidebar_name}</span>
          </button>
          <Menu label={i18n.tk("app", "More options")} trigger={moreIcon} items={[...(row.sidebar_feedback ? [{ id: "feedback", label: i18n.tk("app", "Feedback") }] : []), { id: "delete", label: i18n.tk("app", "Delete") }]} onselect={(id) => id === "feedback" ? onfeedback(row.id) : ondelete(row.id)} />
        </li>
      {/each}
      {#if vm.can_load_more || vm.loading_more}<li class="min-h-8" use:sentinel={{ enabled: vm.can_load_more, onvisible: onloadmore }}>{#if vm.loading_more}<p role="status" class="p-2 text-sm text-muted">{i18n.tk("app", "Loading more...")}</p>{/if}</li>{/if}
    </ul>
  {/if}
</div>
<ConfirmDialog open={showConfirmation && vm.confirm_delete !== null} title={i18n.tk("common", "Are you sure you want to delete this file?")} description={i18n.tk("common", "Note that you can't access this file again.")} confirmLabel={i18n.tk("common", "Yes")} cancelLabel={i18n.tk("common", "No")} tone="danger" onconfirm={onconfirmdelete} oncancel={oncanceldelete} />
{#snippet moreIcon()}<Ellipsis size={18} aria-hidden="true" />{/snippet}
