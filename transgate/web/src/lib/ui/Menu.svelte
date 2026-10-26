<script lang="ts" generics="Id extends string">
  import type { Snippet } from "svelte";

  interface Props {
    /** Accessible name of the disclosure button. */
    label: string;
    items: readonly { id: Id; label: string; disabled?: boolean }[];
    onselect: (id: Id) => void;
    /** Button content; defaults to the label text. */
    trigger?: Snippet;
    align?: "start" | "end";
    /** `above` for menus at the foot of a column, like the sidebar account menu. */
    placement?: "below" | "above";
    /** The trigger fills its container's width. */
    fill?: boolean;
  }

  let { label, items, onselect, trigger, align = "end", placement = "below", fill = false }: Props = $props();
  const id = $props.id();
  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();

  function close(returnFocus: boolean) {
    open = false;
    if (returnFocus) button?.focus();
  }

  $effect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (root && !root.contains(event.target as Node)) close(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  });
</script>

<div
  bind:this={root}
  class={["relative", fill ? "block w-full" : "inline-block"]}
  role="presentation"
  onkeydown={(event) => {
    if (event.key === "Escape" && open) {
      event.stopPropagation();
      close(true);
    }
  }}
  onfocusout={(event) => {
    if (open && root && !root.contains(event.relatedTarget as Node | null)) close(false);
  }}
>
  <button
    bind:this={button}
    type="button"
    aria-expanded={open}
    aria-controls="{id}-list"
    aria-label={trigger ? label : undefined}
    onclick={() => (open = !open)}
    class={["inline-flex min-h-11 items-center gap-2 rounded-md px-3 hover:bg-cursor-bg", fill && "w-full"]}
  >
    {#if trigger}{@render trigger()}{:else}{label}{/if}
  </button>
  {#if open}
    <ul
      id="{id}-list"
      class={[
        "absolute z-20 min-w-48 rounded-md border border-border bg-surface py-1 shadow-lg",
        placement === "above" ? "bottom-full mb-1" : "mt-1",
        align === "end" ? "right-0" : "left-0",
      ]}
    >
      {#each items as item (item.id)}
        <li>
          <button
            type="button"
            disabled={item.disabled}
            onclick={() => {
              close(true);
              onselect(item.id);
            }}
            class="block min-h-11 w-full px-4 text-left hover:bg-cursor-bg disabled:cursor-not-allowed disabled:opacity-50"
          >
            {item.label}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
