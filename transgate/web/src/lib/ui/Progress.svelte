<script lang="ts">
  interface Props {
    /** Accessible name of the progress bar. */
    label: string;
    /** 0 to 100; omit for indeterminate progress. */
    value?: number | null;
    /** Shows the rounded percentage beside the bar. */
    showValue?: boolean;
  }

  let { label, value = null, showValue = false }: Props = $props();
  const clamped = $derived(value === null ? null : Math.min(100, Math.max(0, value)));
</script>

<div class="flex items-center gap-2">
  <div
    role="progressbar"
    aria-label={label}
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={clamped === null ? undefined : Math.round(clamped)}
    class="relative h-2 flex-1 overflow-hidden rounded-full bg-cursor-bg"
  >
    {#if clamped === null}
      <span class="absolute inset-y-0 w-1/3 animate-pulse rounded-full bg-progress"></span>
    {:else}
      <span class="absolute inset-y-0 left-0 rounded-full bg-progress" style:width="{clamped}%"></span>
    {/if}
  </div>
  {#if showValue && clamped !== null}
    <span class="text-sm tabular-nums text-muted" aria-hidden="true">{Math.round(clamped)}%</span>
  {/if}
</div>
