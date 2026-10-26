<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    open: boolean;
    title: string;
    /** Escape, the backdrop or a close action; the owner decides whether to set `open` false. */
    onclose: () => void;
    /** `sheet` docks to the bottom edge on phones; `dialog` is centered. */
    kind?: "dialog" | "sheet";
    wide?: boolean;
    /** Blocks dismissal while work is pending (for example a destructive confirm in flight). */
    locked?: boolean;
    children: Snippet;
    actions?: Snippet;
  }

  let { open, title, onclose, kind = "dialog", wide = false, locked = false, children, actions }: Props = $props();

  let element: HTMLDialogElement | undefined = $state();
  const titleId = $props.id();

  $effect(() => {
    if (!element) return;
    if (open && !element.open) element.showModal();
    else if (!open && element.open) element.close();
  });

  function dismiss(event: Event) {
    event.preventDefault();
    if (!locked) onclose();
  }
</script>

<dialog
  bind:this={element}
  aria-labelledby={titleId}
  oncancel={dismiss}
  onclick={(event) => {
    if (event.target === element) dismiss(event);
  }}
  class={[
    "max-h-[90dvh] overflow-hidden border border-border bg-surface p-0 text-fg",
    kind === "sheet"
      ? "mx-0 mt-auto mb-0 w-full max-w-none rounded-t-xl"
      : wide ? "m-auto w-[min(72rem,calc(100vw-2rem))] rounded-lg" : "m-auto w-[min(32rem,calc(100vw-2rem))] rounded-lg",
  ]}
>
  <!-- Keyboard scrolling of overflow: WCAG 2.1.1 / axe scrollable-region-focusable. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div role="region" aria-labelledby={titleId} tabindex="0" class="flex max-h-[calc(90dvh-2px)] flex-col gap-4 overflow-auto p-5">
    <h2 id={titleId} class="text-lg font-semibold">{title}</h2>
    {@render children()}
    {#if actions}
      <div class="flex flex-wrap justify-end gap-2">{@render actions()}</div>
    {/if}
  </div>
</dialog>
