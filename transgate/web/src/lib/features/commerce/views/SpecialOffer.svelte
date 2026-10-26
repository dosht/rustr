<script lang="ts">
  import type { SpecialClientAlert } from "$lib/core/types/SpecialClientAlert";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";

  let { offer, ondismiss, oncopy }: {
    offer: SpecialClientAlert;
    ondismiss: () => void;
    oncopy: (text: string) => Promise<boolean>;
  } = $props();
  let copied = $state(false);
  const t = (key: string) => i18n.tk("common", key);
</script>

<Alert tone="info" {ondismiss} dismissLabel={t("Close")}>
  <div class="flex flex-col gap-2">
    <h3 class="font-semibold">{t("Special Offer for Valued Clients!")}</h3>
    <p>{offer.message}</p>
    <div class="flex flex-wrap items-center gap-2">
      <code class="select-all rounded border border-border px-3 py-2">{offer.code}</code>
      <Button variant="secondary" onclick={async () => (copied = await oncopy(offer.code))}>{t(copied ? "copied" : "copyCode")}</Button>
    </div>
    <p>{t("Use this code at checkout to get your discount!")}</p>
  </div>
</Alert>
