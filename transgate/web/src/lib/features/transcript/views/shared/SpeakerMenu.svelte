<!--
  The per-line speaker control (WEB-019/020; SpeakerLabel.tsx, EditSpeaker.tsx). Choosing an option
  changes this line; "Edit speaker..." renames the current value across the job (byJobId). Which
  options exist and which one is checked come from core (speaker_options, speaker_value).
-->
<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";
  import type { SpeakerLabel } from "$lib/core/types/SpeakerLabel";
  import type { SpeakerOption } from "$lib/core/types/SpeakerOption";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import { speakerText } from "./speaker";

  interface Props {
    label: SpeakerLabel;
    /** The stored value: the checked option and the rename source. */
    value: string | null;
    options: readonly SpeakerOption[];
    rtl: boolean;
    onchoose: (speaker: string) => void;
    onrename: (from: string, to: string) => void;
  }

  let { label, value, options, rtl, onchoose, onrename }: Props = $props();
  const id = $props.id();
  let open = $state(false);
  let renaming = $state(false);
  let draft = $state("");
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  function restoreFocus() { void tick().then(() => trigger?.focus()); }

  $effect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (root && !root.contains(event.target as Node)) open = false;
    };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); open = false; restoreFocus(); } };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", escape); };
  });

  function startRename() {
    draft = value ?? "";
    renaming = true;
    open = false;
  }

  function submit() {
    renaming = false;
    onrename(value ?? "", draft);
    restoreFocus();
  }

  function keydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      submit();
    } else if (event.key === "Escape") {
      event.preventDefault();
      renaming = false;
      restoreFocus();
    }
  }
</script>

<div bind:this={root} class={["relative flex shrink-0 flex-col pt-2 text-sm text-muted", rtl ? "items-start" : "items-end"]}>
  {#if renaming}
    <div class="flex items-center gap-1">
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="min-w-[60px] rounded border border-border bg-surface px-2 py-1 text-fg"
        aria-label={i18n.tk("app", "p5.transcript.edit_speaker_label")}
        spellcheck="false"
        autofocus
        bind:value={draft}
        onkeydown={keydown}
      />
      <IconButton size="sm" icon={Check} label={i18n.tk("app", "p5.transcript.save_line")} onclick={submit} />
      <IconButton size="sm" icon={X} label={i18n.tk("common", "Cancel")} onclick={() => { renaming = false; restoreFocus(); }} />
    </div>
  {:else}
    <button
      bind:this={trigger}
      type="button"
      class="flex min-h-11 w-[120px] items-center gap-1 truncate rounded px-2 hover:bg-cursor-bg"
      class:justify-end={rtl}
      aria-expanded={open}
      aria-controls="{id}-speakers"
      title={speakerText(label)}
      onclick={() => (open = !open)}
    >
      <span class="truncate">{speakerText(label)}</span>
      <ChevronDown size={12} class={["shrink-0 opacity-60 transition-transform", open && "rotate-180"]} aria-hidden="true" />
    </button>
    {#if open}
      <ul id="{id}-speakers" class="absolute top-full right-0 z-30 mt-1 w-[160px] overflow-hidden rounded-lg border border-border bg-surface shadow-lg">
        <li class="max-h-60 overflow-y-auto">
          <ul>
            {#each options as option (option.value)}
              <li>
                <button
                  type="button"
                  class="flex w-full items-center justify-end gap-2 px-4 py-2 text-right hover:bg-cursor-bg"
                  aria-pressed={option.value === value}
                  onclick={() => {
                    open = false;
                    onchoose(option.value);
                    restoreFocus();
                  }}
                >
                  <span class="truncate">{speakerText(option.label)}</span>
                  {#if option.value === value}<Check size={16} aria-hidden="true" />{/if}
                </button>
              </li>
            {/each}
          </ul>
        </li>
        <li class="border-t border-border px-3 py-2">
          <button type="button" class="w-full text-right text-sm text-accent hover:text-accent-hover" onclick={startRename}>
            {i18n.tk("common", "Edit speaker")}...
          </button>
        </li>
      </ul>
    {/if}
  {/if}
</div>
