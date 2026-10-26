<script lang="ts">
  // Two horizontal panes with a keyboard-operable separator (paneforge renders
  // role=separator and handles arrows/Home/End). The secondary pane may be
  // collapsible, like the source desktop transcript/chat layout
  // (Transcript.tsx:653-884: 60/30 left, 35/20 right, collapsed 3).
  // Sizes are percentages chosen by the owning view; this primitive decides
  // nothing about which panes exist.
  import type { Snippet } from "svelte";
  import { Pane, PaneGroup, PaneResizer } from "paneforge";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";

  interface PaneSize {
    defaultSize: number;
    minSize: number;
    maxSize?: number;
  }

  interface Props {
    /** Accessible name of the separator, for example "Resize chat panel". */
    separatorLabel: string;
    primary: PaneSize;
    secondary: PaneSize & { collapsedSize?: number };
    /** Collapsed state of the secondary pane; omit for a pane that cannot collapse. */
    collapsed?: boolean;
    /** Reports a collapse or expand caused by dragging past the minimum. */
    oncollapsedchange?: (collapsed: boolean) => void;
    first: Snippet;
    second: Snippet;
  }

  let { separatorLabel, primary, secondary, collapsed, oncollapsedchange, first, second }: Props = $props();

  const secondaryId = $props.id();
  const collapsible = $derived(collapsed !== undefined);
  let secondaryPane: ReturnType<typeof Pane> | undefined = $state();
  // paneforge's separator carries no aria-valuenow; a focusable separator needs one.
  let measured: number | null = $state(null);
  const valueNow = $derived(Math.round(measured ?? primary.defaultSize));

  // The owner's `collapsed` is authoritative; drive the pane imperatively.
  $effect(() => {
    if (!secondaryPane || collapsed === undefined) return;
    if (collapsed && !secondaryPane.isCollapsed()) secondaryPane.collapse();
    else if (!collapsed && secondaryPane.isCollapsed()) secondaryPane.expand();
  });
</script>

<PaneGroup direction="horizontal" class="h-full" onLayoutChange={(layout) => { measured = layout[0] ?? measured; }}>
  <Pane defaultSize={primary.defaultSize} minSize={primary.minSize} maxSize={primary.maxSize} order={1} class="h-full">
    {@render first()}
  </Pane>
  <PaneResizer
    aria-label={separatorLabel}
    aria-orientation="vertical"
    aria-controls={secondaryId}
    aria-valuemin={primary.minSize}
    aria-valuemax={primary.maxSize ?? 100}
    aria-valuenow={valueNow}
    disabled={collapsed === true}
    class="group flex w-2 items-center justify-center data-[enabled=false]:hidden"
  >
    <span class="flex h-8 w-1 items-center justify-center rounded-full bg-border group-hover:bg-accent group-data-[active]:bg-accent">
      <GripVertical size={12} aria-hidden="true" class="text-muted group-hover:text-on-accent" />
    </span>
  </PaneResizer>
  <Pane
    bind:this={secondaryPane}
    id={secondaryId}
    defaultSize={secondary.defaultSize}
    minSize={secondary.minSize}
    maxSize={secondary.maxSize}
    {collapsible}
    collapsedSize={secondary.collapsedSize}
    order={2}
    onCollapse={() => {
      if (collapsed !== true) oncollapsedchange?.(true);
    }}
    onExpand={() => {
      if (collapsed !== false) oncollapsedchange?.(false);
    }}
    class="h-full"
  >
    {@render second()}
  </Pane>
</PaneGroup>
