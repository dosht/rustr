<script lang="ts" generics="Id extends string">
  import type { Snippet } from "svelte";

  interface Props {
    /** Accessible name of the tab list. */
    label: string;
    tabs: readonly { id: Id; label: string }[];
    selected: Id;
    onselect: (id: Id) => void;
    panel: Snippet<[Id]>;
  }

  let { label, tabs, selected, onselect, panel }: Props = $props();
  const base = $props.id();
  let buttons: HTMLButtonElement[] = $state([]);

  // Arrow keys, Home and End move between tabs and select them (automatic activation).
  function onkeydown(event: KeyboardEvent, index: number) {
    const last = tabs.length - 1;
    const next =
      event.key === "ArrowRight" ? (index === last ? 0 : index + 1)
      : event.key === "ArrowLeft" ? (index === 0 ? last : index - 1)
      : event.key === "Home" ? 0
      : event.key === "End" ? last
      : null;
    if (next === null) return;
    event.preventDefault();
    onselect(tabs[next].id);
    buttons[next]?.focus();
  }
</script>

<div>
  <div role="tablist" aria-label={label} class="flex gap-1 overflow-x-auto border-b border-border">
    {#each tabs as tab, index (tab.id)}
      <button
        bind:this={buttons[index]}
        type="button"
        role="tab"
        id="{base}-tab-{tab.id}"
        aria-selected={tab.id === selected}
        aria-controls="{base}-panel"
        tabindex={tab.id === selected ? 0 : -1}
        onclick={() => onselect(tab.id)}
        onkeydown={(event) => onkeydown(event, index)}
        class={[
          "min-h-11 whitespace-nowrap border-b-2 px-3 font-medium",
          tab.id === selected ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg",
        ]}
      >
        {tab.label}
      </button>
    {/each}
  </div>
  <div role="tabpanel" id="{base}-panel" aria-labelledby="{base}-tab-{selected}" tabindex="0" class="pt-4">
    {@render panel(selected)}
  </div>
</div>
