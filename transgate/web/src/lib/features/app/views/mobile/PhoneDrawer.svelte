<script lang="ts">
  // Phone navigation drawer (source MobileDrawerView.tsx:111-305). A modal
  // dialog from the start edge; the recent list is p1Z's, passed in. Any
  // navigation closes it, as in the source.
  import type { Snippet } from "svelte";
  import Coins from "@lucide/svelte/icons/coins";
  import Folder from "@lucide/svelte/icons/folder";
  import Plus from "@lucide/svelte/icons/plus";
  import type { Msg } from "$lib/core/types/Msg";
  import { i18n } from "$lib/i18n/index.svelte";
  import logoMark from "../../art/logo_0.png?url";

  interface Props {
    open: boolean;
    displayName: string;
    balance: Msg;
    onclose: () => void;
    onnew: () => void;
    onall: () => void;
    onbalance: () => void;
    onaccount: () => void;
    recent: Snippet;
  }

  let { open, displayName, balance, onclose, onnew, onall, onbalance, onaccount, recent }: Props = $props();
  let element: HTMLDialogElement | undefined = $state();
  const recentId = $props.id();
  const t = (key: string) => i18n.tk("common", key);
  const initials = $derived(
    displayName
      .split(" ")
      .map((part) => part.charAt(0))
      .join("")
      .toUpperCase()
      .slice(0, 2),
  );

  $effect(() => {
    if (!element) return;
    if (open && !element.open) element.showModal();
    else if (!open && element.open) element.close();
  });

  function then(action: () => void) {
    return () => {
      action();
      onclose();
    };
  }
</script>

<dialog
  bind:this={element}
  aria-label={t("mobile.navigation.mainSection")}
  oncancel={(event) => {
    event.preventDefault();
    onclose();
  }}
  onclick={(event) => {
    if (event.target === element) onclose();
  }}
  class="m-0 h-dvh max-h-none w-[80%] max-w-[320px] border-0 border-e border-border bg-surface p-0 text-fg shadow-2xl"
>
  <div class="flex h-full flex-col">
    <div class="flex items-center gap-2 border-b border-border p-5">
      <img src={logoMark} alt="" class="h-6 w-auto" />
      <span class="text-lg font-bold tracking-wide">TRANSGATE</span>
    </div>
    <div class="flex flex-col gap-2 border-b border-border px-4 py-4">
      <button type="button" onclick={then(onnew)} class="flex h-11 w-full items-center gap-3 rounded-md bg-accent p-3 text-sm text-on-accent hover:bg-accent-hover">
        <Plus size={20} aria-hidden="true" />{t("mobile.navigation.newTranscript")}
      </button>
      <button type="button" onclick={then(onall)} class="flex h-11 w-full items-center gap-3 rounded-md border border-border bg-cursor-bg p-3 text-sm hover:bg-border">
        <Folder size={20} aria-hidden="true" />{t("mobile.navigation.allTranscripts")}
      </button>
    </div>
    <section class="flex min-h-0 flex-1 flex-col py-4" aria-labelledby={recentId}>
      <h2 id={recentId} class="px-4 pb-2 text-xs font-semibold tracking-wider text-muted uppercase">{t("mobile.navigation.recentSection")}</h2>
      <div class="min-h-0 flex-1 overflow-y-auto px-4">{@render recent()}</div>
    </section>
    <div class="flex flex-col gap-2 border-t border-border px-4 py-3">
      <button type="button" onclick={then(onbalance)} class="flex w-full items-center gap-3 rounded-full bg-cursor-bg px-3 py-2 hover:bg-border">
        <span class="flex size-10 shrink-0 items-center justify-center rounded-full bg-success text-bg"><Coins size={20} aria-hidden="true" /></span>
        <span class="flex flex-1 items-center justify-between gap-2 text-sm">
          <span class="text-muted">{t("Balance")}</span>
          <span class="font-semibold text-success">{i18n.t(balance)}</span>
        </span>
      </button>
      <button type="button" onclick={onaccount} class="flex w-full items-center gap-3 rounded-full bg-cursor-bg px-3 py-2 hover:bg-border">
        <span class="flex size-10 shrink-0 items-center justify-center rounded-full bg-accent font-semibold text-on-accent" aria-hidden="true">{initials}</span>
        <span class="min-w-0 flex-1 truncate text-start text-sm font-semibold">{displayName}</span>
      </button>
    </div>
  </div>
</dialog>
