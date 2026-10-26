<script lang="ts">
  // "Please rotate your device" (source LandscapeBlocker.tsx:27-52). A modal
  // dialog, so the app beneath is inert while it shows; the source overlay was
  // aria-hidden and left the app focusable underneath (D9). It cannot be
  // dismissed: rotating back closes it.
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import { i18n } from "$lib/i18n/index.svelte";

  let { open }: { open: boolean } = $props();
  let element: HTMLDialogElement | undefined = $state();
  const id = $props.id();

  $effect(() => {
    if (!element) return;
    if (open && !element.open) element.showModal();
    else if (!open && element.open) element.close();
  });
</script>

<dialog
  bind:this={element}
  aria-labelledby="{id}-title"
  aria-describedby="{id}-body"
  oncancel={(event) => event.preventDefault()}
  class="m-0 h-dvh max-h-none w-dvw max-w-none border-0 bg-fg p-8 text-bg"
>
  <div class="flex h-full flex-col items-center justify-center gap-6 text-center">
    <div class="flex size-20 items-center justify-center rounded-full bg-accent/20 motion-safe:animate-pulse">
      <RotateCcw size={40} aria-hidden="true" class="text-accent" />
    </div>
    <div>
      <h2 id="{id}-title" class="mb-2 text-xl font-bold">{i18n.tk("common", "Please rotate your device")}</h2>
      <p id="{id}-body" class="text-sm opacity-80">{i18n.tk("common", "This app works best in portrait mode")}</p>
    </div>
  </div>
</dialog>
