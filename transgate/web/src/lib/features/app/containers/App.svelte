<script lang="ts">
  // Root container: renders core's AppVm and forwards intents. It also runs the
  // shell mechanics around it: the UI locale's catalogs, viewport changes,
  // the phone landscape notice, cookie consent and manual pageviews (D12).
  import { onDestroy, untrack } from "svelte";
  import ConfirmPage from "$lib/features/commerce/views/ConfirmPage.svelte";
  import type { InvoicePreview } from "$lib/platform/preview.svelte";
  import type { AppIntent, AppVm, TransgateCore } from "$lib/core";
  import AuthView from "$lib/features/auth/views/AuthView.svelte";
  import NewPasswordView from "$lib/features/auth/views/NewPasswordView.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { RuntimeConfig } from "$lib/platform/config";
  import { watchPhoneLandscape } from "$lib/platform/landscape";
  import type { MediaPlatform } from "$lib/platform/media";
  import type { Telemetry } from "$lib/platform/telemetry.svelte";
  import { watchViewport } from "$lib/platform/viewport.svelte";
  import type { ThemeStore } from "$lib/theme/theme.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import CookieConsent from "../views/CookieConsent.svelte";
  import LandscapeNotice from "../views/LandscapeNotice.svelte";
  import SignedInFrame from "./SignedInFrame.svelte";

  interface Props {
    core: TransgateCore;
    media: MediaPlatform;
    theme: ThemeStore;
    telemetry: Telemetry;
    captcha: RuntimeConfig["captcha"];
    preview: InvoicePreview;
    copy: (text: string, html: string | null) => Promise<boolean>;
  }

  let { core, media, theme, telemetry, captcha, preview, copy }: Props = $props();

  // Seed from the owned presenter once; subsequent views arrive through subscribe.
  let vm: AppVm = $state(untrack(() => core.current()));
  const locale = $derived(vm.locale);
  /** The locale whose catalogs are loaded; nothing renders copy before that. */
  let loaded: string | null = $state(null);
  let landscape = $state(false);

  function receive(next: AppVm): void {
    // Release obsolete document bytes synchronously, before another delivery.
    preview.sync(next);
    vm = next;
  }

  function dispatch(intent: AppIntent): void {
    receive(core.dispatch(intent));
  }

  $effect(() => core.subscribe(receive));
  onDestroy(() => {
    core.dispose();
    preview.dispose();
  });

  $effect(() => {
    const wanted = locale;
    let current = true;
    void i18n.use(wanted).then(() => {
      if (current && wanted === locale) loaded = wanted;
    });
    document.documentElement.lang = wanted;
    return () => { current = false; };
  });

  $effect(() => watchViewport((viewport) => dispatch({ type: "viewport_changed", intent: { viewport } })));
  $effect(() => watchPhoneLandscape((value) => (landscape = value)));

  // A manual pageview per route, as the source captured them (PostHogProvider.tsx:71-83).
  const routeKey = $derived(vm.state === "signed_in" || vm.state === "signed_out" ? JSON.stringify(vm.route) : vm.state);
  $effect(() => {
    void routeKey;
    telemetry.pageview();
  });
</script>

{#if loaded === locale}
  <div inert={landscape}>
  {#if vm.state === "restoring"}
    <main class="flex min-h-dvh items-center justify-center p-4">
      <div class="w-full max-w-xs"><Skeleton label={i18n.tk("common", "Loading...")} lines={2} /></div>
    </main>
  {:else if vm.state === "signed_out"}
    {#if vm.route.page === "new_password"}
      <NewPasswordView vm={vm.new_password} onintent={(intent) => dispatch({ type: "new_password", intent })} />
    {:else}
      <AuthView vm={vm.auth} captchaSiteKey={captcha?.siteKey ?? null} onintent={(intent) => dispatch({ type: "auth", intent })} />
    {/if}
  {:else if vm.state === "signed_in"}
    {#if vm.session_error}
      <div class="fixed inset-x-0 top-0 z-40 p-2">
        <Alert tone="error">
          {i18n.t(vm.session_error)}
          {#snippet actions()}
            <button type="button" class="underline" onclick={() => dispatch({ type: "session_retried" })}>{i18n.tk("app", "Try Again")}</button>
          {/snippet}
        </Alert>
      </div>
    {/if}
    {#if vm.route.page === "new_password"}
      <!-- Recovery link: core signs the user in and routes here; the source page is standalone, without the app frame. -->
      <NewPasswordView vm={vm.new_password} onintent={(intent) => dispatch({ type: "new_password", intent })} />
    {:else}
      {#key vm.profile?.account}
        <SignedInFrame {vm} {dispatch} {media} {theme} previewUrl={preview.url} oncopy={(text) => copy(text, null)} />
      {/key}
    {/if}
  {:else if vm.state === "checkout_confirm"}
    <ConfirmPage messages={vm.messages} />
  {:else}
    <main class="flex min-h-dvh items-center justify-center p-4">
      <div class="w-full max-w-md"><Alert tone="info">{i18n.t(vm.message)}</Alert></div>
    </main>
  {/if}

  <CookieConsent open={telemetry.undecided} onaccept={() => telemetry.accept()} onreject={() => telemetry.reject()} />
  </div>
  <LandscapeNotice open={landscape} />
{/if}
