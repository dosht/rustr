<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";

  export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

  interface Props extends Omit<HTMLButtonAttributes, "children"> {
    variant?: ButtonVariant;
    size?: "sm" | "md";
    /** Pending work: the button stays focusable, reports busy and ignores clicks. */
    busy?: boolean;
    children: Snippet;
  }

  let { variant = "secondary", size = "md", busy = false, type = "button", disabled, onclick, children, class: extra, ...rest }: Props = $props();

  const variants: Record<ButtonVariant, string> = {
    primary: "bg-accent text-on-accent hover:bg-accent-hover border-transparent",
    secondary: "bg-surface text-fg border-border hover:bg-cursor-bg",
    ghost: "bg-transparent text-fg border-transparent hover:bg-cursor-bg",
    danger: "bg-error text-on-error border-transparent hover:opacity-90",
  };
</script>

<button
  {...rest}
  {type}
  {disabled}
  aria-busy={busy || undefined}
  aria-disabled={busy || undefined}
  class={[
    "inline-flex items-center justify-center gap-2 rounded-md border font-medium",
    "disabled:cursor-not-allowed disabled:opacity-50",
    size === "sm" ? "min-h-9 px-3 text-sm" : "min-h-11 px-4",
    variants[variant],
    extra,
  ]}
  onclick={(event) => {
    if (busy) {
      event.preventDefault();
      return;
    }
    onclick?.(event);
  }}
>
  {#if busy}
    <span class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent" aria-hidden="true"></span>
  {/if}
  {@render children()}
</button>
