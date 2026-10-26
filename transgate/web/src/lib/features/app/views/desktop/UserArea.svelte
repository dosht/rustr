<script lang="ts">
  // Sidebar foot (source UserArea.tsx:130-267, collapsed :66-124): balance,
  // support, upgrade and the account menu. The source "API Keys" entry is not
  // rendered until P7 delivers the key screen (P5 D13); the KenzNote promo is
  // not carried into the app (screens ledger open question, recorded).
  import Coins from "@lucide/svelte/icons/coins";
  import LifeBuoy from "@lucide/svelte/icons/life-buoy";
  import Package from "@lucide/svelte/icons/package";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import type { Msg } from "$lib/core/types/Msg";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Menu from "$lib/ui/Menu.svelte";

  export type AccountAction = "sign_out" | "change_plan" | "settings";

  interface Props {
    collapsed: boolean;
    displayName: string;
    balance: Msg;
    signingOut: boolean;
    onbalance: () => void;
    onsupport: () => void;
    onupgrade: () => void;
    onaccount: (action: AccountAction) => void;
  }

  let { collapsed, displayName, balance, signingOut, onbalance, onsupport, onupgrade, onaccount }: Props = $props();
  const t = (key: string) => i18n.tk("common", key);
  const items = $derived([
    { id: "sign_out" as const, label: t("Sign Out"), disabled: signingOut },
    { id: "change_plan" as const, label: t("Change Plan") },
    { id: "settings" as const, label: t("Settings") },
  ]);
  const initial = $derived(displayName.charAt(0).toUpperCase() || "U");
  const row = "flex w-full items-center gap-2 rounded-md px-2 py-2 text-sm text-muted hover:bg-cursor-bg hover:text-fg";
</script>

<div class="border-t border-border pt-1">
  {#if collapsed}
    <div class="flex flex-col items-center gap-2">
      <IconButton label={t("Balance")} icon={Coins} onclick={onbalance} />
      <IconButton label={t("Support")} icon={LifeBuoy} onclick={onsupport} />
      <IconButton label={t("Upgrade Now")} icon={Package} onclick={onupgrade} />
      <Menu label={displayName} {items} onselect={onaccount} align="start" placement="above">
        {#snippet trigger()}
          <span class="flex size-6 items-center justify-center rounded-full bg-muted text-xs font-medium text-bg" aria-hidden="true">{initial}</span>
        {/snippet}
      </Menu>
    </div>
  {:else}
    <button type="button" class={row} onclick={onbalance}>
      <Coins size={20} aria-hidden="true" />
      <span>{t("Balance")}: {i18n.t(balance)}</span>
    </button>
    <button type="button" class={row} onclick={onsupport}>
      <LifeBuoy size={20} aria-hidden="true" />
      <span>{t("Support")}</span>
    </button>
    <button type="button" class={row} onclick={onupgrade}>
      <Package size={20} aria-hidden="true" />
      <span>{t("Upgrade Now")}</span>
    </button>
    <Menu label={displayName} {items} onselect={onaccount} align="start" placement="above" fill>
      {#snippet trigger()}
        <span class="flex min-w-0 grow items-center justify-between gap-2">
          <span class="truncate text-fg">{displayName}</span>
          <Ellipsis size={20} aria-hidden="true" />
        </span>
      {/snippet}
    </Menu>
  {/if}
</div>
