<script lang="ts">
  interface Props {
    /** Announced once while loading, for example "Loading...". */
    label: string;
    lines?: number;
  }

  let { label, lines = 3 }: Props = $props();
</script>

<div role="status" aria-busy="true" class="flex flex-col gap-2">
  <span class="sr-only">{label}</span>
  {#each { length: lines }, index (index)}
    <span aria-hidden="true" class={["h-4 animate-pulse rounded bg-cursor-bg", index === lines - 1 ? "w-2/3" : "w-full"]}></span>
  {/each}
</div>
