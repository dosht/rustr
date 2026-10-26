<script lang="ts">
  // The recovery link's update-password screen (source new-password.tsx:43-80).
  // Core decides busy, the error text and the success notice, and returns home
  // after it (the source's 2 s timer).
  import type { NewPasswordIntent } from "$lib/core/types/NewPasswordIntent";
  import type { NewPasswordVm } from "$lib/core/types/NewPasswordVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import logo from "../../app/art/logo.png?url";

  interface Props {
    vm: NewPasswordVm;
    onintent: (intent: NewPasswordIntent) => void;
  }

  let { vm, onintent }: Props = $props();
  const t = (key: string) => i18n.tk("common", key);
</script>

<main class="flex min-h-dvh justify-center bg-bg">
  <div class="mx-auto w-full max-w-md p-4">
    <a href="/" class="flex justify-center p-8">
      <img src={logo} alt="Transgate" class="h-16 w-auto" />
    </a>
    <form
      class="flex flex-col gap-4 rounded-lg border border-border bg-surface px-8 pt-6 pb-8"
      onsubmit={(event) => {
        event.preventDefault();
        onintent({ type: "submitted" });
      }}
    >
      <TextField
        label={t("New Password")}
        type="password"
        autocomplete="new-password"
        required
        placeholder={t("New Password")}
        value={vm.password}
        disabled={vm.busy}
        oninput={(text) => onintent({ type: "password_changed", text })}
      />
      {#if vm.error}
        <Alert tone="error">{i18n.t(vm.error)}</Alert>
      {/if}
      {#if vm.success}
        <Alert tone="success">{i18n.t(vm.success)}</Alert>
      {/if}
      <div>
        <Button variant="primary" type="submit" busy={vm.busy}>{t("Change Password")}</Button>
      </div>
    </form>
  </div>
</main>
