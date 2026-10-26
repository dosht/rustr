<script lang="ts">
  import type { Snippet } from "svelte";
  import X from "@lucide/svelte/icons/x";
  import IconButton from "./IconButton.svelte";

  interface Props {
    /** `error` is announced assertively (`role=alert`); the rest politely (`role=status`). */
    tone: "info" | "success" | "error";
    title?: string;
    children: Snippet;
    /** Shown only with `dismissLabel`, its accessible name. */
    ondismiss?: () => void;
    dismissLabel?: string;
    actions?: Snippet;
  }

  let { tone, title, children, ondismiss, dismissLabel, actions }: Props = $props();

  const tones = {
    info: "bg-notice-bg border-border",
    success: "bg-notice-bg border-success",
    error: "bg-error-bg border-error",
  } as const;
</script>

<div role={tone === "error" ? "alert" : "status"} class={["flex items-start gap-3 rounded-md border p-3 text-fg", tones[tone]]}>
  <div class="flex flex-1 flex-col gap-1">
    {#if title}<p class="font-semibold">{title}</p>{/if}
    <div>{@render children()}</div>
    {#if actions}<div class="mt-1 flex flex-wrap gap-2">{@render actions()}</div>{/if}
  </div>
  {#if ondismiss && dismissLabel}
    <IconButton label={dismissLabel} icon={X} size="sm" onclick={ondismiss} />
  {/if}
</div>
