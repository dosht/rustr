<script lang="ts">
  // Phone header (source MobileNavigationContainer MobileHeader, home
  // [[...jobId]].tsx:374-396): menu button, core's title, and "More options"
  // for the open job's action sheet.
  import Menu from "@lucide/svelte/icons/menu";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";

  interface Props {
    title: string;
    drawerOpen: boolean;
    onmenu: () => void;
    /** Present only with an open job. */
    onmore?: () => void;
  }

  let { title, drawerOpen, onmenu, onmore }: Props = $props();
</script>

<header class="flex h-14 shrink-0 items-center gap-2 border-b border-border bg-surface px-2">
  <IconButton label={i18n.tk("app", "Main navigation menu")} icon={Menu} aria-expanded={drawerOpen} onclick={onmenu} />
  <h1 class="min-w-0 flex-1 truncate font-semibold">{title}</h1>
  {#if onmore}
    <IconButton label={i18n.tk("app", "More options")} icon={EllipsisVertical} onclick={onmore} />
  {/if}
</header>
