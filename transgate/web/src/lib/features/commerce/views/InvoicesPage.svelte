<script lang="ts">
  import type { InvoicesVm } from "$lib/core/types/InvoicesVm";
  import type { InvoicesIntent } from "$lib/core/types/InvoicesIntent";
  import type { BillingMessages } from "$lib/core/types/BillingMessages";
  import type { InvoiceRow } from "$lib/core/types/InvoiceRow";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import TextField from "$lib/ui/TextField.svelte";

  let { vm, messages, phone = false, previewUrl = null, onintent, onback }: {
    vm: InvoicesVm;
    messages: BillingMessages;
    phone?: boolean;
    previewUrl?: string | null;
    onintent: (intent: InvoicesIntent) => void;
    onback: () => void;
  } = $props();
  const t = (key: string) => i18n.tk("common", key);
</script>

{#snippet actions(row: InvoiceRow)}
  <div class="flex flex-wrap gap-2">
    <Button onclick={() => onintent({ type: "preview", payment_id: row.payment_id })}>{t("Preview")}</Button>
    <Button onclick={() => onintent({ type: "download", payment_id: row.payment_id })}>{t("Download")}</Button>
  </div>
{/snippet}

<section class="flex min-h-0 flex-1 flex-col overflow-auto p-4" data-layout={phone ? "phone" : "desktop"}>
  <header class="mb-6 flex items-center gap-4">
    <Button onclick={onback}>{t("Back")}</Button><h1 class="text-2xl font-semibold">{t("Billing History")}</h1>
  </header>
  {#if vm.admin}
    <div class="mb-6 flex flex-col gap-3 rounded-lg border border-border bg-surface p-4">
      <form class="flex flex-wrap items-end gap-3" onsubmit={(event) => { event.preventDefault(); onintent({ type: "lookup_requested" }); }}>
        <TextField class="min-w-0 flex-1" type="email" label={t("Look up customer invoices by email")} value={vm.admin.email}
          oninput={(text) => onintent({ type: "lookup_email_typed", text })} />
        <Button type="submit" variant="primary" busy={vm.admin.looking_up}>{t("Load")}</Button>
        <Button onclick={() => onintent({ type: "lookup_cleared" })}>{t("Clear")}</Button>
      </form>
      {#if vm.admin.viewing_customer}<p>{t("Viewing invoices for")} <strong>{vm.admin.viewing_customer}</strong></p>{/if}
      {#if messages.customer_not_found}<Alert tone="error">{i18n.t(messages.customer_not_found)}</Alert>{/if}
      <TextField multiline rows={3} label={t("Custom Bill To (tax info, address, company)")}
        hint={t("One line per row. When filled, this replaces the invoice's Bill To details.")}
        placeholder={t("Leave empty to use the customer's saved billing info")}
        value={vm.admin.temporary_bill_to} oninput={(text) => onintent({ type: "temporary_bill_to_typed", text })} />
    </div>
  {/if}
  {#if messages.invoice_failure}<Alert tone="error">{i18n.t(messages.invoice_failure)}</Alert>{/if}
  {#if messages.document_failure && vm.preview === null}<Alert tone="error">{i18n.t(messages.document_failure)}</Alert>{/if}
  {#if vm.loading}
    <Skeleton label={t("Loading...")} />
  {:else if vm.rows.length === 0}
    <p class="py-8 text-center text-muted">{t("No completed payments found")}</p>
  {:else if phone}
    <div class="flex flex-col gap-3">
      {#each vm.rows as row (row.payment_id)}
        <article class="flex flex-col gap-3 rounded-lg border border-border bg-surface p-4" aria-label={row.invoice_id}>
          <h2 class="font-semibold">{row.invoice_id}</h2>
          <dl class="grid grid-cols-2 gap-2">
            <dt>{t("Date")}</dt><dd>{row.date}</dd>
            <dt>{t("Package")}</dt><dd>{row.package_name}</dd>
            <dt>{t("Amount")}</dt><dd>{row.amount_text}</dd>
            <dt>{t("Payment Gateway")}</dt><dd>{row.gateway}</dd>
          </dl>
          {@render actions(row)}
        </article>
      {/each}
    </div>
  {:else}
    <div class="overflow-x-auto">
      <table class="w-full text-start">
        <thead><tr class="border-b border-border">
          {#each ["Invoice ID", "Date", "Package", "Amount", "Payment Gateway"] as label}<th scope="col" class="p-3 text-start">{t(label)}</th>{/each}
          <th scope="col" class="p-3 text-start">{i18n.tk("app", "Actions")}</th>
        </tr></thead>
        <tbody>{#each vm.rows as row (row.payment_id)}
          <tr class="border-b border-border"><th scope="row" class="p-3 text-start font-normal">{row.invoice_id}</th>
            <td class="p-3">{row.date}</td><td class="p-3">{row.package_name}</td><td class="p-3">{row.amount_text}</td><td class="p-3">{row.gateway}</td><td class="p-3">{@render actions(row)}</td>
          </tr>
        {/each}</tbody>
      </table>
    </div>
  {/if}
</section>

<Dialog open={vm.preview !== null} title={t("Invoice Preview")} kind={phone ? "sheet" : "dialog"} wide onclose={() => onintent({ type: "close_preview" })}>
  {#if messages.document_failure}
    <Alert tone="error">{i18n.t(messages.document_failure)}</Alert>
  {:else if previewUrl}
    <iframe src={previewUrl} title={t("Invoice Preview")} class="h-[65dvh] w-full border-0" referrerpolicy="no-referrer"></iframe>
  {:else}
    <Skeleton label={t("Loading...")} />
  {/if}
  {#snippet actions()}
    <Button onclick={() => onintent({ type: "close_preview" })}>{t("Close")}</Button>
    {#if vm.preview !== null}<Button onclick={() => { if (vm.preview !== null) onintent({ type: "download", payment_id: vm.preview }); }}>{t("Download")}</Button>{/if}
  {/snippet}
</Dialog>
