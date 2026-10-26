<script lang="ts">
  // Cookie banner (source CookieConsent.tsx:52-74), shown until the visitor
  // answers. Accept/Reject drive the consent-gated Telemetry (P5 D12); GA is
  // not carried over. A landmark region, so it is reachable without hunting.
  import { i18n } from "$lib/i18n/index.svelte";
  import Button from "$lib/ui/Button.svelte";

  interface Props {
    /** Show while consent is undecided. */
    open: boolean;
    onaccept: () => void;
    onreject: () => void;
  }

  let { open, onaccept, onreject }: Props = $props();
  const t = (key: string) => i18n.tk("common", key);
  const id = $props.id();
</script>

{#if open}
  <section
    aria-labelledby={id}
    class="fixed inset-x-0 bottom-0 z-50 flex flex-col items-center justify-between gap-2 border-t border-border bg-surface p-4 shadow-xl md:flex-row"
  >
    <p {id} class="text-sm">
      {t("We use cookies to ensure you get the best experience on our website")}.
      <a href="/legal/privacy-policy-en" class="text-accent underline-offset-2 hover:underline">{t("Learn more")}</a>.
    </p>
    <div class="flex gap-4">
      <Button variant="primary" onclick={onaccept}>{t("Accept")}</Button>
      <Button variant="secondary" onclick={onreject}>{t("Reject")}</Button>
    </div>
  </section>
{/if}
