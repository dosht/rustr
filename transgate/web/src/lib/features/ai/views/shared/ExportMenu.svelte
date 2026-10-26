<script lang="ts">
  // Source ExportDropdown: four formats, a busy trigger while one runs and the
  // export error with Dismiss. Rendering and saving happen behind the intent.
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import type { DocumentFormat } from "$lib/core/types/DocumentFormat";
  import type { Msg } from "$lib/core/types/Msg";
  import Button from "$lib/ui/Button.svelte";
  import { i18n } from "$lib/i18n/index.svelte";

  interface Props {
    busy: DocumentFormat | null;
    error: Msg | null;
    onexport: (format: DocumentFormat) => void;
    ondismiss: () => void;
  }

  let { busy, error, onexport, ondismiss }: Props = $props();
  const id = $props.id();
  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();

  const options: { format: DocumentFormat; label: string; description: string }[] = [
    { format: "docx", label: "Export as Word (.docx)", description: "Microsoft Word format" },
    { format: "pdf", label: "Export as PDF", description: "Portable Document Format" },
    { format: "txt", label: "Export as TXT", description: "Plain text format" },
    { format: "md", label: "Export as Markdown", description: "Markdown format" },
  ];

  $effect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (root && !root.contains(event.target as Node)) open = false;
    };
    // Escape closes and returns focus to the disclosure button.
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      open = false;
      trigger?.focus();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  });

  function choose(format: DocumentFormat) {
    if (error) ondismiss();
    onexport(format);
    open = false;
  }
</script>

<div bind:this={root} class="relative">
  <button
    bind:this={trigger}
    type="button"
    class="inline-flex min-h-11 min-w-11 items-center justify-center rounded-md text-muted hover:bg-cursor-bg hover:text-fg disabled:opacity-50 sm:min-h-8 sm:min-w-8"
    aria-label={i18n.tk("ai", "Export")}
    aria-expanded={open}
    aria-controls={`${id}-menu`}
    aria-busy={busy !== null}
    disabled={busy !== null}
    onclick={() => (open = !open)}
  >
    {#if busy}<LoaderCircle size={16} aria-hidden="true" class="animate-spin" />{:else}<Download size={16} aria-hidden="true" />{/if}
  </button>
  {#if open}
    <div id={`${id}-menu`} class="absolute right-0 z-20 mt-1 w-64 rounded-md border border-border bg-surface p-1 shadow-lg">
      <!-- web-shell.md: a disclosure button over a list of buttons, not an ARIA menu. -->
      <ul>
        {#each options as option (option.format)}
          <li>
            <button
              type="button"
              class="flex w-full flex-col items-start rounded px-3 py-2 text-left hover:bg-cursor-bg disabled:opacity-50"
              aria-describedby={`${id}-${option.format}`}
              disabled={busy !== null}
              onclick={() => choose(option.format)}
            >
              <span class="text-sm font-medium text-fg">{i18n.tk("ai", option.label)}</span>
              <span id={`${id}-${option.format}`} class="text-xs text-muted">{i18n.tk("ai", option.description)}</span>
            </button>
          </li>
        {/each}
      </ul>
      <div class="my-1 border-t border-border"></div>
      <p class="px-3 py-1 text-xs text-muted">{i18n.tk("ai", "Choose a format to download your analysis")}</p>
    </div>
  {/if}
  {#if error}
    <div role="alert" class="absolute right-0 z-20 mt-1 w-64 rounded-md border border-error bg-error-bg p-3 text-sm">
      <p class="text-fg">{i18n.t(error)}</p>
      <Button variant="ghost" size="sm" onclick={ondismiss}>{i18n.tk("ai", "Dismiss")}</Button>
    </div>
  {/if}
</div>
