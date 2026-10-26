<script lang="ts">
  // The support dialog (source SupportModal.tsx:56-87). A failed send keeps
  // the message and shows core's notice (UI-04); success closes it in core.
  // The host acknowledges durable acceptance only, so no "sent" claim is made.
  import type { SupportVm } from "$lib/core/types/SupportVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import TextField from "$lib/ui/TextField.svelte";

  interface Props {
    vm: SupportVm;
    kind: "dialog" | "sheet";
    onmessage: (text: string) => void;
    onsubmit: () => void;
    onclose: () => void;
  }

  let { vm, kind, onmessage, onsubmit, onclose }: Props = $props();
  const t = (key: string) => i18n.tk("common", key);
</script>

<Dialog open={vm.open} {kind} title={t("Support")} locked={vm.sending} {onclose}>
  <form
    class="flex flex-col gap-3"
    onsubmit={(event) => {
      event.preventDefault();
      onsubmit();
    }}
  >
    <TextField
      multiline
      rows={6}
      label={t("Message")}
      placeholder={t("Enter your message here")}
      value={vm.message}
      disabled={vm.sending}
      oninput={onmessage}
    />
    {#if vm.notice}
      <Alert tone="error">{i18n.t(vm.notice)}</Alert>
    {/if}
    <Button variant="primary" type="submit" busy={vm.sending} class="w-full">{t("Send")}</Button>
  </form>
</Dialog>
