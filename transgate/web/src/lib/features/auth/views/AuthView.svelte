<script lang="ts">
  // /signin (alias /new-signin): the source Supabase Auth UI (signin.tsx:144-178)
  // with its four views. Core's AuthVm decides mode, busy, errors and notices;
  // the labels are the source's `auth` catalog keys for the shown mode.
  import type { AuthIntent } from "$lib/core/types/AuthIntent";
  import type { AuthMode } from "$lib/core/types/AuthMode";
  import type { AuthVm } from "$lib/core/types/AuthVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import logo from "../../app/art/logo.png?url";
  import Turnstile from "./Turnstile.svelte";

  interface Props {
    vm: AuthVm;
    /** Turnstile site key from tg-config; shown only when core requires it. */
    captchaSiteKey: string | null;
    onintent: (intent: AuthIntent) => void;
  }

  let { vm, captchaSiteKey, onintent }: Props = $props();
  const a = (key: string, args?: Record<string, string>) => i18n.tk("auth", key, args);

  /** The mode's wire value is the source catalog section (`sign_in.*`, `magic_link.*`, ...). */
  const s = $derived(vm.mode);
  const social = $derived(vm.mode === "sign_in" || vm.mode === "sign_up");
  const withPassword = $derived(vm.mode === "sign_in" || vm.mode === "sign_up");
  const emailLabel = $derived(a(vm.mode === "magic_link" ? "magic_link.email_input_label" : `${s}.email_label`));
  /** The source's links under the form, per view. */
  const links = $derived<{ mode: AuthMode; key: string }[]>(
    vm.mode === "sign_in"
      ? [
          { mode: "magic_link", key: "magic_link.link_text" },
          { mode: "forgotten_password", key: "forgotten_password.link_text" },
          { mode: "sign_up", key: "sign_up.link_text" },
        ]
      : vm.mode === "sign_up"
        ? [
            { mode: "magic_link", key: "magic_link.link_text" },
            { mode: "sign_in", key: "sign_in.link_text" },
          ]
        : [{ mode: "sign_in", key: "sign_in.link_text" }],
  );
</script>

<main class="flex min-h-dvh justify-center bg-bg">
  <div class="m-auto flex w-80 max-w-lg flex-col gap-8 p-3">
    <a href="/" class="flex justify-center p-8">
      <img src={logo} alt="Transgate" class="h-16 w-auto" />
    </a>
    <div class="flex flex-col gap-4">
      {#if social}
        <Button variant="secondary" disabled={vm.busy} onclick={() => onintent({ type: "google_chosen" })}>
          {a(`${s}.social_provider_text`, { provider: "Google" })}
        </Button>
        <hr class="border-border" />
      {/if}
      <form
        class="flex flex-col gap-4"
        onsubmit={(event) => {
          event.preventDefault();
          onintent({ type: "submitted" });
        }}
      >
        <TextField
          label={emailLabel}
          type="email"
          autocomplete="email"
          required
          placeholder={a(`${s}.email_input_placeholder`)}
          value={vm.email}
          disabled={vm.busy}
          oninput={(text) => onintent({ type: "email_changed", text })}
        />
        {#if withPassword}
          <TextField
            label={a(`${s}.password_label`)}
            type="password"
            autocomplete={vm.mode === "sign_up" ? "new-password" : "current-password"}
            required
            placeholder={a(`${s}.password_input_placeholder`)}
            value={vm.password}
            disabled={vm.busy}
            oninput={(text) => onintent({ type: "password_changed", text })}
          />
        {/if}
        {#if vm.captcha_required && captchaSiteKey}
          <Turnstile
            siteKey={captchaSiteKey}
            language={i18n.locale}
            onsolved={(token) => onintent({ type: "captcha_solved", token })}
            onexpired={() => onintent({ type: "captcha_expired" })}
          />
        {/if}
        <Button variant="primary" type="submit" busy={vm.busy} disabled={!vm.can_submit && !vm.busy}>
          {vm.busy ? a(`${s}.loading_button_label`) : a(`${s}.button_label`)}
        </Button>
      </form>
      {#if vm.error}
        <Alert tone="error">{i18n.t(vm.error)}</Alert>
      {/if}
      {#if vm.notice}
        <Alert tone="success">{i18n.t(vm.notice)}</Alert>
      {/if}
      <nav class="flex flex-col items-center gap-2 text-sm">
        {#each links as link (link.mode)}
          <button
            type="button"
            class="text-muted underline-offset-2 hover:text-fg hover:underline"
            onclick={() => onintent({ type: "mode_chosen", mode: link.mode })}
          >
            {a(link.key)}
          </button>
        {/each}
      </nav>
    </div>
  </div>
</main>
