<script lang="ts">
  // The signed-in frame (source pages/home/[[...jobId]].tsx:370-540): exactly
  // one branch, chosen by core's layouts.frame. Desktop: collapsible sidebar +
  // content. Phone: header, drawer and account sheet + content. The dialogs
  // (feedback, support, settings) mount once here from core's view models.
  // Frame mechanics (sidebar collapse, drawer/sheet open) are local UI state.
  import type { AppIntent } from "$lib/core/types/AppIntent";
  import type { SignedInVm } from "$lib/core/types/SignedInVm";
  import type { Tab } from "$lib/core/types/Tab";
  import HomeContent from "$lib/features/workspace/containers/HomeContent.svelte";
  import SidebarHistory from "$lib/features/history/containers/SidebarHistory.svelte";
  import DesktopFeedback from "$lib/features/feedback/views/desktop/DesktopFeedback.svelte";
  import MobileFeedback from "$lib/features/feedback/views/mobile/MobileFeedback.svelte";
  import DesktopSupport from "$lib/features/support/views/desktop/DesktopSupport.svelte";
  import MobileSupport from "$lib/features/support/views/mobile/MobileSupport.svelte";
  import ManageDataPanel from "$lib/features/settings/views/ManageDataPanel.svelte";
  import ProfilePanel from "$lib/features/settings/views/ProfilePanel.svelte";
  import SettingsDialog from "$lib/features/settings/views/SettingsDialog.svelte";
  import AiConsentSettings from "$lib/features/ai/views/shared/AiConsentSettings.svelte";
  import type { ConsentIntent } from "$lib/features/ai/intents";
  import DesktopPurchase from "$lib/features/commerce/views/desktop/DesktopPurchase.svelte";
  import MobilePurchase from "$lib/features/commerce/views/mobile/MobilePurchase.svelte";
  import DesktopInvoices from "$lib/features/commerce/views/desktop/DesktopInvoices.svelte";
  import MobileInvoices from "$lib/features/commerce/views/mobile/MobileInvoices.svelte";
  import DesktopSubscription from "$lib/features/commerce/views/desktop/DesktopSubscription.svelte";
  import MobileSubscription from "$lib/features/commerce/views/mobile/MobileSubscription.svelte";
  import SpecialOffer from "$lib/features/commerce/views/SpecialOffer.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { MediaPlatform } from "$lib/platform/media";
  import type { ThemeStore } from "$lib/theme/theme.svelte";
  import DesktopSidebar from "../views/desktop/DesktopSidebar.svelte";
  import type { AccountAction } from "../views/desktop/UserArea.svelte";
  import AccountSheet, { type AccountChoice } from "../views/mobile/AccountSheet.svelte";
  import PhoneDrawer from "../views/mobile/PhoneDrawer.svelte";
  import PhoneHeader from "../views/mobile/PhoneHeader.svelte";

  interface Props {
    vm: SignedInVm;
    dispatch: (intent: AppIntent) => void;
    media: MediaPlatform;
    theme: ThemeStore;
    previewUrl: string | null;
    oncopy: (text: string) => Promise<boolean>;
  }

  let { vm, dispatch, media, theme, previewUrl, oncopy }: Props = $props();

  let collapsed = $state(false);
  let drawerOpen = $state(false);
  let sheetOpen = $state(false);
  let menuOpen = $state(false);
  let offerDismissed = $state(false);

  const phone = $derived(vm.layouts.frame === "phone");
  const kind = $derived(phone ? "sheet" : "dialog");
  const Purchase = $derived(phone ? MobilePurchase : DesktopPurchase);
  const Invoices = $derived(phone ? MobileInvoices : DesktopInvoices);
  const Subscription = $derived(phone ? MobileSubscription : DesktopSubscription);
  const offer = $derived(offerDismissed ? null : vm.special_client_alert);
  const displayName = $derived(vm.profile?.display_name ?? "");
  const title = $derived(vm.header_title_text ?? i18n.t(vm.header_title));

  const openNew = () => dispatch({ type: "navigate", intent: { route: vm.new_transcript_route } });
  const openAll = () => dispatch({ type: "navigate", intent: { route: { page: "home", view: { kind: "all_transcripts" } } } });
  const openPurchase = () => dispatch({ type: "purchase_opened" });
  const openSupport = () => dispatch({ type: "support", intent: { type: "opened" } });
  const openSettings = () => dispatch({ type: "settings", intent: { type: "opened" } });
  const signOut = () => dispatch({ type: "sign_out_requested" });

  function account(action: AccountAction) {
    if (action === "sign_out") signOut();
    else if (action === "change_plan") openPurchase();
    else openSettings();
  }

  function choose(choice: AccountChoice) {
    if (choice === "balance" || choice === "upgrade") openPurchase();
    else if (choice === "support") openSupport();
    else if (choice === "settings") openSettings();
    else signOut();
  }

  const feedback = {
    onselect: (rating: 0 | 5 | 10) => dispatch({ type: "feedback", intent: { type: "selected", rating } }),
    oncomment: (text: string) => dispatch({ type: "feedback", intent: { type: "comment_changed", text } }),
    onsubmit: () => dispatch({ type: "feedback", intent: { type: "submitted" } }),
    onclose: () => dispatch({ type: "feedback", intent: { type: "closed" } }),
  };
  const support = {
    onmessage: (text: string) => dispatch({ type: "support", intent: { type: "message_changed", text } }),
    onsubmit: () => dispatch({ type: "support", intent: { type: "submitted" } }),
    onclose: () => dispatch({ type: "support", intent: { type: "closed" } }),
  };
  const manageData = {
    oncleanup: (on: boolean) => dispatch({ type: "manage_data", intent: { type: "cleanup_toggled", on } }),
    ondeletemonthly: (on: boolean) => dispatch({ type: "manage_data", intent: { type: "delete_monthly_toggled", on } }),
    ondeleteall: () => dispatch({ type: "manage_data", intent: { type: "delete_all_pressed" } }),
    ondeleteaccount: () => dispatch({ type: "manage_data", intent: { type: "delete_account_pressed" } }),
    onconfirm: () => dispatch({ type: "manage_data", intent: { type: "confirmed" } }),
    oncancel: () => dispatch({ type: "manage_data", intent: { type: "canceled" } }),
  };
  // p21's view intents onto core's consent intents (core app.rs ConsentIntent).
  const consentIntents = {
    enable: "enable_requested",
    decline: "decline_requested",
    disable: "disable_confirmed",
    dismiss_error: "error_dismissed",
  } as const;
  const consent = (intent: ConsentIntent) => dispatch({ type: "consent", intent: { type: consentIntents[intent.type] } });
</script>

{#snippet history()}
  <SidebarHistory {vm} {dispatch} {media} {collapsed} />
{/snippet}

{#snippet settingsPanel(tab: Tab)}
  {#if tab === "profile"}
    <ProfilePanel
      email={vm.profile?.email ?? ""}
      joinedOnText={vm.joined_on_text}
      theme={theme.choice}
      locales={vm.locales}
      ontheme={(choice) => theme.set(choice)}
      onlocale={(code) => dispatch({ type: "locale_chosen", intent: { code } })}
    />
  {:else if tab === "ai_features"}
    <AiConsentSettings consent={vm.consent} onintent={consent} showTitle={false} />
  {:else if tab === "manage_data"}
    <ManageDataPanel vm={vm.manage_data} {kind} {...manageData} />
  {:else if tab === "subscription"}
    <Subscription vm={vm.subscription} messages={vm.billing_messages} onintent={(intent) => dispatch({ type: "subscription", intent })} />
  {/if}
{/snippet}

<div data-layout={phone ? "phone" : "desktop"} class={["relative z-0 flex h-dvh w-full overflow-hidden", phone ? "flex-col" : "flex-row"]}>
  {#if phone}
    <PhoneHeader
      {title}
      {drawerOpen}
      onmenu={() => (drawerOpen = true)}
      onmore={vm.transcript ? () => (menuOpen = true) : undefined}
    />
    <PhoneDrawer
      open={drawerOpen}
      {displayName}
      balance={vm.balance}
      onclose={() => (drawerOpen = false)}
      onnew={openNew}
      onall={openAll}
      onbalance={openPurchase}
      onaccount={() => {
        drawerOpen = false;
        sheetOpen = true;
      }}
      recent={history}
    />
    <AccountSheet
      open={sheetOpen}
      {displayName}
      email={vm.profile?.email ?? ""}
      balance={vm.balance}
      signingOut={vm.signing_out}
      onclose={() => (sheetOpen = false)}
      onchoose={choose}
    />
  {:else}
    <DesktopSidebar
      {collapsed}
      {displayName}
      balance={vm.balance}
      signingOut={vm.signing_out}
      ontoggle={() => (collapsed = !collapsed)}
      onnew={openNew}
      onall={openAll}
      onbalance={openPurchase}
      onsupport={openSupport}
      onupgrade={openPurchase}
      onaccount={account}
      {history}
    />
  {/if}
  <main class="relative flex min-w-0 flex-1 flex-col overflow-hidden">
    {#if vm.invoices}
      <Invoices vm={vm.invoices} messages={vm.billing_messages} {previewUrl}
        onintent={(intent) => dispatch({ type: "invoices", intent })} onback={openNew} />
    {:else}
      {#if offer}<div class="p-3"><SpecialOffer {offer} ondismiss={() => (offerDismissed = true)} {oncopy} /></div>{/if}
      <HomeContent {vm} {dispatch} {media} bind:menuOpen />
    {/if}
  </main>
</div>

{#if phone}
  <MobileFeedback vm={vm.feedback} {...feedback} />
  <MobileSupport vm={vm.support} {...support} />
{:else}
  <DesktopFeedback vm={vm.feedback} {...feedback} />
  <DesktopSupport vm={vm.support} {...support} />
{/if}
{#if vm.settings}
  <SettingsDialog
    vm={vm.settings}
    {kind}
    onselect={(tab) => dispatch({ type: "settings", intent: { type: "tab_selected", tab } })}
    onclose={() => dispatch({ type: "settings", intent: { type: "closed" } })}
    panel={settingsPanel}
  />
{/if}

{#if vm.purchase}
  <Purchase vm={vm.purchase} messages={vm.billing_messages} {offer} {oncopy}
    ondismissoffer={() => (offerDismissed = true)}
    onintent={(intent) => dispatch({ type: "purchase", intent })}
    onclose={() => dispatch({ type: "purchase_closed" })} />
{/if}
