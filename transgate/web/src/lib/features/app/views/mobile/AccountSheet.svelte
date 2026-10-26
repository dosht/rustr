<script lang="ts">
  // Phone account sheet (source MobileBottomSheetView.tsx:85-182): who is
  // signed in, then balance, support, upgrade, settings and logout. Each
  // choice closes the sheet first, as in the source.
  import Coins from "@lucide/svelte/icons/coins";
  import LifeBuoy from "@lucide/svelte/icons/life-buoy";
  import LogOut from "@lucide/svelte/icons/log-out";
  import Package from "@lucide/svelte/icons/package";
  import Settings from "@lucide/svelte/icons/settings";
  import type { Msg } from "$lib/core/types/Msg";
  import { i18n } from "$lib/i18n/index.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";

  export type AccountChoice = "balance" | "support" | "upgrade" | "settings" | "logout";

  interface Props {
    open: boolean;
    displayName: string;
    email: string;
    balance: Msg;
    signingOut: boolean;
    onclose: () => void;
    onchoose: (choice: AccountChoice) => void;
  }

  let { open, displayName, email, balance, signingOut, onclose, onchoose }: Props = $props();
  const t = (key: string) => i18n.tk("common", `mobile.navigation.bottomSheet.${key}`);
  const initials = $derived(
    displayName
      .split(" ")
      .map((part) => part.charAt(0))
      .join("")
      .toUpperCase()
      .slice(0, 2),
  );
  const row = "flex w-full items-center gap-3 px-4 py-3 text-start text-muted hover:bg-cursor-bg hover:text-fg disabled:opacity-50";

  function choose(choice: AccountChoice) {
    onclose();
    onchoose(choice);
  }
</script>

<Dialog {open} kind="sheet" title={t("account")} {onclose}>
  <div class="flex items-center gap-3 rounded-full bg-cursor-bg px-4 py-3">
    <span class="flex size-12 shrink-0 items-center justify-center rounded-full bg-accent text-lg font-semibold text-on-accent" aria-hidden="true">{initials}</span>
    <span class="min-w-0 flex-1">
      <span class="block truncate font-semibold">{displayName}</span>
      <span class="block truncate text-sm text-muted">{email}</span>
    </span>
  </div>
  <div class="-mx-5 flex flex-col">
    <button type="button" class={row} onclick={() => choose("balance")}>
      <Coins size={20} aria-hidden="true" /><span class="flex-1 font-medium">{t("balance")}</span><span class="text-sm">{i18n.t(balance)}</span>
    </button>
    <button type="button" class={row} onclick={() => choose("support")}>
      <LifeBuoy size={20} aria-hidden="true" /><span class="font-medium">{t("support")}</span>
    </button>
    <button type="button" class={row} onclick={() => choose("upgrade")}>
      <Package size={20} aria-hidden="true" /><span class="font-medium">{t("upgradeNow")}</span>
    </button>
    <button type="button" class={row} onclick={() => choose("settings")}>
      <Settings size={20} aria-hidden="true" /><span class="font-medium">{t("title")}</span>
    </button>
    <hr class="my-2 border-border" />
    <button type="button" class={row} disabled={signingOut} onclick={() => choose("logout")}>
      <LogOut size={20} aria-hidden="true" /><span class="font-medium">{t("logout")}</span>
    </button>
  </div>
</Dialog>
