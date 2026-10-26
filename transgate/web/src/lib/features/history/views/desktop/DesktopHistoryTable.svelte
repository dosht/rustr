<!--
  All-transcripts table (WEB-015; JobListTable.tsx). Pure: renders core's JobListVm and reports
  intents. Below md the same component shows the source's compact card rows inside the desktop
  frame (JobListTable.tsx:295-413); the frame/phone split itself is core's layout decision.
-->
<script lang="ts">
  import Play from "@lucide/svelte/icons/play";
  import Languages from "@lucide/svelte/icons/languages";
  import Download from "@lucide/svelte/icons/download";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { JobListVm } from "$lib/core/types/JobListVm";
  import type { JobRow } from "$lib/core/types/JobRow";
  import { i18n } from "$lib/i18n/index.svelte";
  import Badge from "$lib/ui/Badge.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import { sentinel } from "../shared/sentinel";

  interface Props {
    vm: JobListVm;
    onopen: (job: number) => void;
    ondownload: (job: number) => void;
    onfeedback: (job: number) => void;
    ondelete: (job: number) => void;
    onconfirmdelete: () => void;
    oncanceldelete: () => void;
    onloadmore: () => void;
  }

  let { vm, onopen, ondownload, onfeedback, ondelete, onconfirmdelete, oncanceldelete, onloadmore }: Props = $props();

  const name = (row: JobRow) => row.display_name ?? i18n.t(vm.untitled);
</script>

{#snippet rowIcon(row: JobRow, size: number)}
  {#if row.spinner}
    <span class="size-4 animate-spin rounded-full border-2 border-progress border-t-transparent" aria-hidden="true"></span>
  {:else if row.is_translation}
    <Languages {size} class="text-accent" aria-label={i18n.tk("app", "p5.history.translation_job")} role="img" />
  {:else}
    <Play {size} class="text-muted" aria-hidden="true" />
  {/if}
{/snippet}

{#snippet actions(row: JobRow, size: "sm" | "md")}
  <div class="flex items-center justify-end gap-1">
    <IconButton {size} icon={Play} label={i18n.tk("app", "View")} onclick={() => onopen(row.id)} />
    {#if row.can_download}
      <IconButton {size} icon={Download} label={i18n.tk("app", "Download")} disabled={row.downloading} aria-busy={row.downloading} onclick={() => ondownload(row.id)} />
    {/if}
    {#if row.can_feedback}
      <IconButton {size} icon={MessageSquare} label={i18n.tk("app", "Feedback")} onclick={() => onfeedback(row.id)} />
    {/if}
    <IconButton {size} icon={Trash2} label={i18n.tk("app", "Delete")} class="text-error" onclick={() => ondelete(row.id)} />
  </div>
{/snippet}

{#if vm.initial_loading}
  <div class="flex h-64 flex-col items-center justify-center gap-4" role="status">
    <span class="size-10 animate-spin rounded-full border-4 border-progress border-t-transparent" aria-hidden="true"></span>
    <p class="text-muted">{i18n.tk("app", "Loading jobs...")}</p>
  </div>
{:else if vm.error}
  <div class="flex h-64 flex-col items-center justify-center text-error" role="alert">
    <p class="mb-2 text-lg font-medium">{i18n.t(vm.error)}</p>
    {#if vm.error_detail}<p class="text-sm">{vm.error_detail}</p>{/if}
  </div>
{:else if vm.empty}
  <div class="flex h-64 flex-col items-center justify-center text-muted">
    <p class="mb-2 text-lg font-medium">{i18n.tk("app", "No transcripts found")}</p>
    <p class="text-sm">{i18n.tk("app", "Start by creating your first transcript")}</p>
  </div>
{:else}
  <div class="overflow-hidden rounded-lg border border-border bg-surface shadow-lg">
    <div class="hidden overflow-x-auto md:block">
      <table class="w-full divide-y divide-border">
        <thead class="bg-bg">
          <tr class="text-left text-xs font-bold tracking-wider text-muted uppercase">
            <th scope="col" class="min-w-[200px] px-4 py-3 lg:px-6">{i18n.tk("app", "Name")}</th>
            <th scope="col" class="min-w-[100px] px-4 py-3 lg:px-6">{i18n.tk("app", "Status")}</th>
            <th scope="col" class="hidden px-4 py-3 lg:table-cell lg:px-6">{i18n.tk("app", "Duration")}</th>
            <th scope="col" class="hidden px-4 py-3 xl:table-cell xl:px-6">{i18n.tk("app", "Date Created")}</th>
            <th scope="col" class="min-w-[120px] px-4 py-3 text-right lg:px-6">{i18n.tk("app", "Actions")}</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-border">
          {#each vm.rows as row (row.id)}
            <tr
              class={["cursor-pointer hover:bg-cursor-bg", row.selected && "bg-notice-bg"]}
              aria-current={row.selected ? "true" : undefined}
              onclick={() => onopen(row.id)}
            >
              <td class="px-4 py-4 lg:px-6">
                <div class="flex items-center gap-4">
                  <div class="flex size-10 shrink-0 items-center justify-center rounded-full bg-bg">{@render rowIcon(row, 20)}</div>
                  <div class="min-w-0">
                    <p class="max-w-[180px] truncate text-sm font-medium lg:max-w-[250px]" title={name(row)}>{name(row)}</p>
                    <p class="truncate text-sm text-muted">{row.language ?? i18n.t(vm.auto_detect)}</p>
                  </div>
                </div>
              </td>
              <td class="px-4 py-4 whitespace-nowrap lg:px-6">
                <Badge tone={row.tone} spinner={row.spinner}>{i18n.t(row.status_label)}</Badge>
              </td>
              <td class="hidden px-4 py-4 text-sm whitespace-nowrap lg:table-cell lg:px-6">{row.duration ?? i18n.t(vm.not_available)}</td>
              <td class="hidden px-4 py-4 text-sm whitespace-nowrap xl:table-cell xl:px-6">{row.date_text}</td>
              <!-- Actions never open the row (JobListTable.tsx:242). -->
              <td class="px-4 py-4 lg:px-6" onclick={(event) => event.stopPropagation()}>{@render actions(row, "md")}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <ul class="divide-y divide-border md:hidden">
      {#each vm.rows as row (row.id)}
        <li class={["flex items-start justify-between gap-2 p-4", row.selected && "bg-notice-bg"]} aria-current={row.selected ? "true" : undefined}>
          <button type="button" class="flex min-w-0 flex-1 items-start gap-3 text-left" onclick={() => onopen(row.id)}>
            <span class="mt-1 flex size-8 shrink-0 items-center justify-center rounded-full bg-bg">{@render rowIcon(row, 16)}</span>
            <span class="min-w-0">
              <span class="block max-w-[150px] truncate text-xs font-medium" title={name(row)}>{name(row)}</span>
              <span class="mt-1 flex gap-2 text-xs text-muted">
                <span>{row.duration ?? i18n.t(vm.not_available)}</span>
                <span>{new Date(row.created_at_ms).toLocaleDateString()}</span>
              </span>
              <span class="mt-1 block"><Badge tone={row.tone} spinner={row.spinner}>{i18n.t(row.status_label)}</Badge></span>
            </span>
          </button>
          {@render actions(row, "sm")}
        </li>
      {/each}
    </ul>

    {#if vm.can_load_more || vm.loading_more}
      <div class="flex items-center justify-center py-4" use:sentinel={{ enabled: vm.can_load_more, onvisible: onloadmore }}>
        {#if vm.loading_more}
          <span class="size-5 animate-spin rounded-full border-2 border-progress border-t-transparent" aria-hidden="true"></span>
          <span class="ml-2 text-sm text-muted" role="status">{i18n.tk("app", "Loading more...")}</span>
        {/if}
      </div>
    {/if}
    {#if vm.end_of_list}
      <p class="py-4 text-center text-sm text-muted">{i18n.t(vm.end_of_list)}</p>
    {/if}
  </div>
{/if}

<ConfirmDialog
  open={vm.confirm_delete !== null}
  title={i18n.tk("common", "Are you sure you want to delete this file?")}
  description={i18n.tk("common", "Note that you can't access this file again.")}
  confirmLabel={i18n.tk("common", "Yes")}
  cancelLabel={i18n.tk("common", "No")}
  tone="danger"
  onconfirm={onconfirmdelete}
  oncancel={oncanceldelete}
/>
