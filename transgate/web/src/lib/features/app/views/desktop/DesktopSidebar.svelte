<script lang="ts">
  // Desktop sidebar (source SideBar.tsx:110-160 collapsed, :163-250 expanded;
  // width and toggle from home/[[...jobId]].tsx:400-438). The history list is
  // p1Z's, passed in; collapsing is a frame mechanic, not business state.
  import type { Snippet } from "svelte";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import Folder from "@lucide/svelte/icons/folder";
  import Plus from "@lucide/svelte/icons/plus";
  import type { Msg } from "$lib/core/types/Msg";
  import { i18n } from "$lib/i18n/index.svelte";
  import logo from "../../art/logo.png?url";
  import logoMark from "../../art/logo_0.png?url";
  import UserArea, { type AccountAction } from "./UserArea.svelte";

  interface Props {
    collapsed: boolean;
    displayName: string;
    balance: Msg;
    signingOut: boolean;
    ontoggle: () => void;
    onnew: () => void;
    onall: () => void;
    onbalance: () => void;
    onsupport: () => void;
    onupgrade: () => void;
    onaccount: (action: AccountAction) => void;
    history: Snippet;
  }

  let { collapsed, ontoggle, onnew, onall, history, ...user }: Props = $props();
  const toggleLabel = $derived(i18n.tk("app", collapsed ? "Expand Sidebar" : "Collapse Sidebar"));
  const newLabel = $derived(i18n.tk("common", "New Transcript"));
  const allLabel = $derived(i18n.tk("app", "All Transcripts"));
</script>

<nav
  aria-label={i18n.tk("app", "Recent transcripts")}
  class={["relative flex h-full shrink-0 flex-col bg-surface p-2 transition-[width] duration-300", collapsed ? "w-16" : "w-[260px] xl:w-[360px]"]}
>
  <button
    type="button"
    title={toggleLabel}
    aria-label={toggleLabel}
    aria-expanded={!collapsed}
    onclick={ontoggle}
    class={["absolute top-4 z-10 flex size-10 items-center justify-center rounded-lg bg-cursor-bg hover:bg-border", collapsed ? "left-3" : "right-4"]}
  >
    <ChevronsRight size={20} aria-hidden="true" class={["transition-transform duration-300", !collapsed && "rotate-180"]} />
  </button>

  {#if collapsed}
    <div class="mt-16 flex flex-col items-center gap-3">
      <img src={logoMark} alt="" class="size-8 object-contain" />
      <button type="button" title={newLabel} aria-label={newLabel} onclick={onnew} class="flex size-10 items-center justify-center rounded-md bg-accent text-on-accent hover:bg-accent-hover">
        <Plus size={16} aria-hidden="true" />
      </button>
      <button type="button" title={allLabel} aria-label={allLabel} onclick={onall} class="flex size-10 items-center justify-center rounded-md bg-cursor-bg hover:bg-border">
        <Folder size={16} aria-hidden="true" />
      </button>
    </div>
    <div class="mt-auto">
      <UserArea collapsed {...user} />
    </div>
  {:else}
    <div class="mb-6 flex items-center">
      <img src={logo} alt="Transgate" width="180" height="80" class="mt-2 mb-2 ml-4 h-auto w-[180px]" />
    </div>
    <div class="-mr-2 flex min-h-0 flex-1 flex-col py-2">
      {@render history()}
    </div>
    <UserArea collapsed={false} {...user} />
  {/if}
</nav>
