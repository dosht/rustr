<script lang="ts">
  import type { Snippet } from "svelte";

  /** Mirrors core's semantic `Tone` (transgate_core presentation); never a color. */
  export type Tone = "neutral" | "progress" | "accent" | "success" | "danger";

  interface Props {
    tone: Tone;
    /** A spinner for work in progress; decorative, the text carries the meaning. */
    spinner?: boolean;
    children: Snippet;
  }

  let { tone, spinner = false, children }: Props = $props();

  const tones: Record<Tone, string> = {
    neutral: "text-muted border-border",
    progress: "text-progress border-progress",
    accent: "text-accent border-accent",
    success: "text-success border-success",
    danger: "text-error border-error",
  };
</script>

<span class={["inline-flex items-center gap-1 rounded-full border bg-surface px-2 py-0.5 text-xs font-semibold", tones[tone]]}>
  {#if spinner}
    <span class="size-3 animate-spin rounded-full border-2 border-current border-t-transparent" aria-hidden="true"></span>
  {/if}
  {@render children()}
</span>
