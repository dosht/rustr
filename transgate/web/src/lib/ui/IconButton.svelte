<script lang="ts">
  import type { Component } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";

  interface Props extends Omit<HTMLButtonAttributes, "children" | "aria-label"> {
    /** The accessible name; required because the button shows only an icon. */
    label: string;
    icon: Component<{ size?: number; "aria-hidden"?: boolean | "true" }>;
    /** Toggle state for toggle buttons; omit for plain actions. */
    pressed?: boolean;
    size?: "sm" | "md";
  }

  let { label, icon: Icon, pressed, size = "md", type = "button", class: extra, ...rest }: Props = $props();
</script>

<button
  {...rest}
  {type}
  aria-label={label}
  title={label}
  aria-pressed={pressed}
  class={[
    "inline-flex items-center justify-center rounded-md text-muted hover:bg-cursor-bg hover:text-fg",
    "disabled:cursor-not-allowed disabled:opacity-50",
    "aria-pressed:bg-accent aria-pressed:text-on-accent aria-pressed:hover:bg-accent-hover aria-pressed:hover:text-on-accent",
    size === "sm" ? "size-9" : "size-11",
    extra,
  ]}
>
  <Icon size={size === "sm" ? 16 : 20} aria-hidden="true" />
</button>
