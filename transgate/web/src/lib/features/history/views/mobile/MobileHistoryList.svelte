<!--
  Phone all-transcripts list (WEB-016; MobileJobListView.tsx, MobileJobCard.tsx, SwipeableJobRow.tsx).
  D9 baseline: no filter sheet and no pull-to-refresh on this route. Swipe left past 80 px reveals
  Delete (UI mechanics only); Delete asks core, which shows the confirmation (confirm_delete).
  Keyboard users reach the same Delete button when the card's row has focus.
-->
<script lang="ts">
  import Languages from "@lucide/svelte/icons/languages";
  import Mic from "@lucide/svelte/icons/mic";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { JobListVm } from "$lib/core/types/JobListVm";
  import type { JobRow } from "$lib/core/types/JobRow";
  import { i18n } from "$lib/i18n/index.svelte";
  import Badge from "$lib/ui/Badge.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import { sentinel } from "../shared/sentinel";

  interface Props {
    vm: JobListVm;
    onopen: (job: number) => void;
    ondelete: (job: number) => void;
    onconfirmdelete: () => void;
    oncanceldelete: () => void;
    onloadmore: () => void;
    onrefresh: () => void;
    onnew: () => void;
  }

  let { vm, onopen, ondelete, onconfirmdelete, oncanceldelete, onloadmore, onrefresh, onnew }: Props = $props();

  const THRESHOLD = 80;
  const MAX = 120;
  /** Only one row is swiped open at a time; local gesture state, never business data. */
  let swipe = $state<{ id: number; startX: number; offset: number; revealed: boolean } | null>(null);

  const name = (row: JobRow) => row.display_name ?? i18n.t(vm.untitled);
  const offset = (row: JobRow) => (swipe?.id === row.id ? swipe.offset : 0);

  function down(row: JobRow, event: PointerEvent) {
    if (event.pointerType !== "touch") return;
    swipe = { id: row.id, startX: event.clientX, offset: swipe?.id === row.id && swipe.revealed ? -MAX : 0, revealed: false };
  }

  function move(row: JobRow, event: PointerEvent) {
    if (swipe?.id !== row.id) return;
    swipe.offset = Math.max(-MAX, Math.min(0, event.clientX - swipe.startX));
  }

  function up(row: JobRow) {
    if (swipe?.id !== row.id) return;
    if (swipe.offset <= -THRESHOLD) {
      swipe.offset = -MAX;
      swipe.revealed = true;
    } else {
      swipe = null;
    }
  }

  function tap(row: JobRow) {
    if (swipe?.id === row.id && swipe.revealed) {
      swipe = null;
      return;
    }
    onopen(row.id);
  }

  function remove(row: JobRow) {
    swipe = null;
    ondelete(row.id);
  }
</script>

{#if vm.initial_loading && vm.rows.length === 0}
  <div class="flex h-full flex-col items-center justify-center gap-4 bg-surface" role="status">
    <span class="size-10 animate-spin rounded-full border-4 border-progress border-t-transparent" aria-hidden="true"></span>
    <p class="text-muted">{i18n.tk("app", "Loading jobs...")}</p>
  </div>
{:else if vm.error}
  <div class="flex h-full flex-col items-center justify-center bg-surface px-4">
    <div class="text-center text-error" role="alert">
      <p class="mb-2 text-lg font-medium">{i18n.t(vm.error)}</p>
      {#if vm.error_detail}<p class="text-sm">{vm.error_detail}</p>{/if}
    </div>
    <div class="mt-4"><Button onclick={onrefresh}>{i18n.tk("app", "Try Again")}</Button></div>
  </div>
{:else if vm.rows.length === 0}
  <div class="flex h-full flex-col items-center justify-center bg-surface px-4 text-center">
    <p class="mb-2 text-lg font-medium">{i18n.tk("app", "No transcripts found")}</p>
    <p class="mb-4 text-sm text-muted">{i18n.tk("app", "Start by creating your first transcript")}</p>
    <Button onclick={onnew}><Plus size={20} aria-hidden="true" />{i18n.tk("app", "New Transcription")}</Button>
  </div>
{:else}
  <div class="relative flex h-full flex-col bg-surface">
    <ul class="flex-1 divide-y divide-border overflow-y-auto">
      {#each vm.rows as row (row.id)}
        <li class="group relative overflow-hidden">
          <div class="absolute inset-y-0 right-0 flex w-24 items-center justify-center bg-error">
            <button
              type="button"
              class={[
                "flex min-h-12 min-w-12 flex-col items-center justify-center gap-1 text-on-error",
                offset(row) > -THRESHOLD && "sr-only group-focus-within:not-sr-only",
              ]}
              onclick={() => remove(row)}
            >
              <Trash2 size={20} aria-hidden="true" />
              <span class="text-xs font-medium">{i18n.tk("app", "Delete")}</span>
            </button>
          </div>
          <button
            type="button"
            class={[
              "relative block min-h-12 w-full bg-surface p-3 text-left hover:bg-cursor-bg sm:p-4",
              row.selected && "border-l-4 border-accent bg-notice-bg",
            ]}
            style:transform="translateX({offset(row)}px)"
            style:transition={swipe?.id === row.id && !swipe.revealed ? "none" : "transform 0.2s ease-out"}
            style:touch-action="pan-y"
            aria-current={row.selected ? "true" : undefined}
            onpointerdown={(event) => down(row, event)}
            onpointermove={(event) => move(row, event)}
            onpointerup={() => up(row)}
            onpointercancel={() => (swipe = null)}
            onclick={() => tap(row)}
          >
            <span class="flex items-start gap-3">
              <span class="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-full bg-bg">
                {#if row.spinner}
                  <span class="size-4 animate-spin rounded-full border-2 border-progress border-t-transparent" aria-hidden="true"></span>
                {:else if row.is_translation}
                  <Languages size={20} class="text-accent" aria-label={i18n.tk("app", "p5.history.translation_job")} role="img" />
                {:else}
                  <Mic size={20} class="text-muted" aria-hidden="true" />
                {/if}
              </span>
              <span class="min-w-0 flex-1">
                <span class="mb-1 block truncate text-sm font-medium" title={name(row)}>{name(row)}</span>
                <span class="mb-2 flex items-center gap-2 text-xs text-muted">
                  {#if row.duration}<span>{row.duration}</span><span aria-hidden="true">•</span>{/if}
                  {#if row.speakers}<span>{i18n.t(row.speakers)}</span><span aria-hidden="true">•</span>{/if}
                  <span class="truncate">{row.date_text}</span>
                </span>
                <Badge tone={row.tone} spinner={row.spinner}>{i18n.t(row.status_label)}</Badge>
              </span>
            </span>
          </button>
        </li>
      {/each}
      {#if vm.can_load_more || vm.loading_more}
        <li class="flex items-center justify-center py-4" use:sentinel={{ enabled: vm.can_load_more, onvisible: onloadmore }}>
          {#if vm.loading_more}
            <span class="size-5 animate-spin rounded-full border-2 border-progress border-t-transparent" aria-hidden="true"></span>
            <span class="ml-2 text-sm text-muted" role="status">{i18n.tk("app", "Loading more...")}</span>
          {/if}
        </li>
      {/if}
    </ul>
    <button
      type="button"
      class="fixed right-4 bottom-[calc(1.5rem+env(safe-area-inset-bottom))] flex size-14 items-center justify-center rounded-full bg-accent text-on-accent shadow-lg hover:bg-accent-hover"
      aria-label={i18n.tk("app", "New transcription")}
      onclick={onnew}
    >
      <Plus size={24} aria-hidden="true" />
    </button>
  </div>
{/if}

<ConfirmDialog
  open={vm.confirm_delete !== null}
  kind="sheet"
  title={i18n.tk("common", "Are you sure you want to delete this file?")}
  description={i18n.tk("common", "Note that you can't access this file again.")}
  confirmLabel={i18n.tk("common", "Yes")}
  cancelLabel={i18n.tk("common", "No")}
  tone="danger"
  onconfirm={onconfirmdelete}
  oncancel={oncanceldelete}
/>
