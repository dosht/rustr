<script lang="ts">
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { FileRow } from "$lib/core/types/FileRow";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Progress from "$lib/ui/Progress.svelte";
  let { row, phone = false, onremove }: { row: FileRow; phone?: boolean; onremove: (id: number) => void } = $props();
  let start: number | null = $state(null), offset = $state(0), revealed = $state(false);
</script>
<li class="relative overflow-hidden rounded-lg border border-border">
  {#if phone && revealed}<button type="button" class="absolute inset-y-0 right-0 w-24 bg-error text-on-error" onclick={() => onremove(row.id)}>{i18n.tk("app", "Delete")}</button>{/if}
  <div class="relative space-y-2 bg-surface p-3" style:transform="translateX({offset}px)" style:touch-action="pan-y" role="group" aria-label={row.name}
    onpointerdown={(event) => { if (phone && event.pointerType === "touch") { start = event.clientX; event.currentTarget.setPointerCapture?.(event.pointerId); } }}
    onpointermove={(event) => { if (start !== null) offset = Math.max(-96, Math.min(0, event.clientX - start)); }}
    onpointerup={() => { if (start !== null) { revealed = offset <= -80; offset = revealed ? -96 : 0; start = null; } }}
    onpointercancel={() => { start = null; offset = 0; revealed = false; }}>
    <div class="flex items-center justify-between gap-3">
      <div class="min-w-0">
        <p class="truncate font-medium" title={row.name}>{row.name}</p>
        <p class="text-xs text-muted">{phone ? row.phone_size : row.desktop_size}{#if row.duration_text} · {i18n.t(row.duration_text)}{:else if !phone} · -{/if}</p>
      </div>
      <span class="text-sm text-muted">{i18n.t(phone ? row.phone_label : row.label)}</span>
      <IconButton icon={Trash2} label={i18n.tk("app", "Delete")} onclick={() => onremove(row.id)} />
    </div>
    {#if !phone || (row.chip.chip === "uploading" && row.progress < 100)}
      <Progress label={row.name} value={row.progress} showValue={phone} />
    {/if}
  </div>
</li>
