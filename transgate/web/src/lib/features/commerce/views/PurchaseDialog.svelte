<script lang="ts">
  import type { PurchaseVm } from "$lib/core/types/PurchaseVm";
  import type { PurchaseIntent } from "$lib/core/types/PurchaseIntent";
  import type { BillingMessages } from "$lib/core/types/BillingMessages";
  import type { SpecialClientAlert } from "$lib/core/types/SpecialClientAlert";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import SpecialOffer from "./SpecialOffer.svelte";

  let { vm, messages, phone = false, offer = null, onintent, onclose, ondismissoffer, oncopy }: {
    vm: PurchaseVm;
    messages: BillingMessages;
    phone?: boolean;
    offer?: SpecialClientAlert | null;
    onintent: (intent: PurchaseIntent) => void;
    onclose: () => void;
    ondismissoffer: () => void;
    oncopy: (text: string) => Promise<boolean>;
  } = $props();
  const t = (key: string) => i18n.tk("common", key);
  // Draft text is an input mechanic. Rust receives it unchanged on commit and
  // owns parsing, bounds, selection and the minimum warning.
  let hoursDraft = $state("");
  $effect(() => { hoursDraft = String(vm.hours); });
  const selectedPlan = $derived(vm.plans.find((plan) => plan.selected));
  const terms = $derived(t("agree-message").split(/(<a>.*?<\/a>)/s));
  const termsUrl = $derived(`/legal/terms-and-conditions-${i18n.locale === "tr" ? "tr" : "en"}`);
  const stripParagraphs = (text: string) => text.replace(/<\/?p>/g, "");
  const selectionClass = (selected: boolean) => selected ? "border-accent bg-notice-bg" : "border-border bg-bg";
</script>

<Dialog open title={t(phone ? "Add Balance" : "Choose your Plan")} kind={phone ? "sheet" : "dialog"} wide={!phone} locked={vm.processing} {onclose}>
  {#snippet actions()}<Button disabled={vm.processing} onclick={onclose}>{t("Close")}</Button>{/snippet}
  {#if offer}<SpecialOffer {offer} ondismiss={ondismissoffer} {oncopy} />{/if}
  <form onsubmit={(event) => { event.preventDefault(); onintent({ type: "pay_requested" }); }}>
    <fieldset disabled={vm.processing} class={phone ? "flex min-w-0 flex-col gap-4" : "grid min-w-0 gap-4 md:grid-cols-3"}>
      <legend class="sr-only">{t("Choose your Plan")}</legend>
      <section class={["min-w-0 rounded-lg border-2 p-4", selectionClass(vm.pay_as_you_go_selected)]}>
        <Button class="w-full" aria-pressed={vm.pay_as_you_go_selected} onclick={() => onintent({ type: "select_pay_as_you_go" })}>{t("Pay As You Go")}</Button>
        {#if !phone || vm.pay_as_you_go_selected}
          <div class="mt-4 flex flex-col gap-3">
            <TextField label={t("Credit Amount")} type="text" inputmode="numeric" value={hoursDraft}
              oninput={(text) => (hoursDraft = text)}
              onblur={() => onintent({ type: "hours_typed", text: hoursDraft })}
              onkeydown={(event: KeyboardEvent & { currentTarget: HTMLInputElement }) => { if (event.key === "Enter") { event.preventDefault(); event.currentTarget.blur(); } }} />
            <div class="flex flex-wrap gap-2" aria-label={t("Hours")}>
              <Button aria-label={`${t("Hours")} −1`} onclick={() => onintent({ type: "step_hours", delta: -1 })}>−</Button>
              {#each vm.hour_presets as hours}
                <Button aria-pressed={vm.hours === hours} onclick={() => onintent({ type: "preset_hours", hours })}>{hours}</Button>
              {/each}
              <Button aria-label={`${t("Hours")} +1`} onclick={() => onintent({ type: "step_hours", delta: 1 })}>+</Button>
            </div>
            {#if vm.minimum_warning}<Alert tone="info">{t("Minimum 5 hours")}</Alert>{/if}
            {#if vm.pay_as_you_go}
              <p class="text-accent">{vm.pay_as_you_go.price_text} <del class="text-muted">${vm.pay_as_you_go.struck_per_hour}</del></p>
            {/if}
            <p class="text-sm">{t("Minimum 5 hours")}</p>
            <p class="text-sm">{i18n.tk("common", "pay-as-you-go-message", { hours: vm.hours })}</p>
          </div>
        {/if}
      </section>
      <section class={["min-w-0 rounded-lg border-2 p-4", selectionClass(!vm.pay_as_you_go_selected)]}>
        <Button class="w-full" aria-pressed={!vm.pay_as_you_go_selected} onclick={() => onintent({ type: "plan_section_clicked" })}>{t("Subscription")}</Button>
        {#if !phone || !vm.pay_as_you_go_selected}
          <div class="mt-4 flex flex-col gap-3">
            <div class="flex gap-2">
              <Button aria-pressed={!vm.yearly} onclick={() => onintent({ type: "interval_chosen", yearly: false })}>{t("Monthly")}</Button>
              <Button aria-pressed={vm.yearly} onclick={() => onintent({ type: "interval_chosen", yearly: true })}>{t("Yearly")}</Button>
            </div>
            {#each vm.plans as plan (plan.package_id)}
              <button type="button" aria-pressed={plan.selected} class={["flex min-h-11 flex-col gap-2 rounded-lg border-2 p-3 text-start", selectionClass(plan.selected)]}
                onclick={() => onintent({ type: "select_plan", package_id: plan.package_id })}>
                <span class="font-semibold">{plan.card.name}</span>
                <span><del class="text-muted">${plan.card.struck_per_month}</del> <strong>${plan.card.usd_per_month}</strong> {t("/month")}</span>
                {#if plan.card.badge}<span class="text-sm text-success">{t(plan.card.badge)}</span>{/if}
                <span class="text-sm">{t("Package Expire: 1 month")}</span>
              </button>
            {/each}
          </div>
        {/if}
      </section>
      <section class={["flex min-w-0 flex-col gap-4 rounded-lg border border-border bg-surface p-4", phone && "sticky bottom-0"]}>
        <div><p class="text-sm text-muted">{t("You Choose")}</p>
          <p>{vm.pay_as_you_go_selected ? `${t("Pay as you go")} / ${vm.hours} ${t("Hours")}` : selectedPlan?.card.name ?? t("Subscription")}</p>
        </div>
        <TextField label={t("Coupon")} value={vm.coupon_code} disabled={vm.coupon_locked} placeholder={t("Enter code")}
          oninput={(text) => onintent({ type: "coupon_typed", text })}
          onkeydown={(event: KeyboardEvent) => { if (event.key === "Enter") { event.preventDefault(); onintent({ type: "coupon_applied" }); } }} />
        <Button disabled={vm.coupon_locked || !vm.coupon_code} onclick={() => onintent({ type: "coupon_applied" })}>{t("Apply")}</Button>
        {#if messages.coupon_notice}<Alert tone="error">{i18n.t(messages.coupon_notice)}</Alert>{/if}
        <div aria-busy={vm.quote_pending}>
          <p>{t("Total Payment")}</p>
          <p class="text-3xl font-bold text-accent">{vm.display_total === null ? "--" : `$${vm.display_total}`}</p>
          {#if vm.try_display !== null}<p class="text-success">= ₺{vm.try_display}</p>{/if}
          {#if vm.quote_pending}<p role="status">{t("Loading...")}</p>{/if}
        </div>
        <label class="flex items-start gap-3">
          <input type="checkbox" class="mt-1 size-5 shrink-0" checked={vm.terms_accepted} onchange={(event) => onintent({ type: "terms_accepted", accepted: event.currentTarget.checked })} />
          <span>{#each terms as part}{#if part.startsWith("<a>")}<a class="underline" href={termsUrl} target="_blank" rel="noreferrer">{stripParagraphs(part.slice(3, -4))}</a>{:else}{stripParagraphs(part)}{/if}{/each}</span>
        </label>
        <Button type="submit" variant="primary" disabled={!vm.can_pay} busy={vm.processing}>{t("Pay with Card")}</Button>
        {#if messages.renewal}<p class="text-sm text-muted">{i18n.t(messages.renewal)}{#if messages.renewal_at_ms !== null}{new Date(messages.renewal_at_ms).toLocaleDateString(i18n.locale)}{/if}</p>{/if}
        {#if messages.purchase_failure}<Alert tone="error">{i18n.t(messages.purchase_failure)}</Alert>{/if}
      </section>
    </fieldset>
  </form>
</Dialog>
