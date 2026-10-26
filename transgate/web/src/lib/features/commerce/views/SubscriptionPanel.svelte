<script lang="ts">
  import type { SubscriptionVm } from "$lib/core/types/SubscriptionVm";
  import type { SubscriptionIntent } from "$lib/core/types/SubscriptionIntent";
  import type { BillingMessages } from "$lib/core/types/BillingMessages";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";

  let { vm, messages, phone = false, onintent }: {
    vm: SubscriptionVm;
    messages: BillingMessages;
    phone?: boolean;
    onintent: (intent: SubscriptionIntent) => void;
  } = $props();
  const t = (key: string) => i18n.tk("common", key);
  // Micros -> Date is unit conversion only; core supplies both instants.
  const date = (micros: number) => new Date(micros / 1000).toLocaleDateString(i18n.locale);
</script>

<section class="flex flex-col gap-4">
  {#if vm.loading}
    <Skeleton label={t("Loading subscription information...")} />
  {:else if vm.plan}
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-3">
      <dt>{t("Current Plan:")}</dt><dd>{vm.plan.name}</dd>
      <dt>{t("Total Minutes:")}</dt><dd>{vm.plan.minutes_included}</dd>
      <dt>{t("Plan Valid Until:")}</dt><dd>{date(vm.plan.valid_until)}</dd>
      <dt>{t("Next Payment:")}</dt><dd>{vm.next_payment === null ? "--" : date(vm.next_payment)}</dd>
    </dl>
    {#if vm.show_cancel}
      <div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
        <p>{t("For cancellation")}</p>
        <Button variant="danger" busy={vm.cancelling} onclick={() => onintent({ type: "cancel_clicked" })}>{t("Cancel Subscription")}</Button>
      </div>
    {/if}
    {#if vm.canceled}<p role="status">{t("Your subscription is canceled, but you can still use your minutes.")}</p>{/if}
  {:else}
    <p>{t(" No Package")}</p>
    <div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
      <p>{t("To upgrade your package")}</p>
      <Button variant="primary" onclick={() => onintent({ type: "upgrade_clicked" })}>{t("Upgrade Now")}</Button>
    </div>
  {/if}
  {#if messages.subscription_failure}<Alert tone="error">{i18n.t(messages.subscription_failure)}</Alert>{/if}
</section>
<ConfirmDialog open={vm.confirming} kind={phone ? "sheet" : "dialog"} title={t("Are you sure you want to cancel your subscription?")}
  description={t("This action cannot be undone. Are you sure you want to proceed?")}
  confirmLabel={t("Yes")} cancelLabel={t("No")} tone="danger" busy={vm.cancelling}
  onconfirm={() => onintent({ type: "cancel_confirmed" })} oncancel={() => onintent({ type: "cancel_dismissed" })} />
