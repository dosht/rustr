<script lang="ts">
  // WEB-045/046: the key screen (source pages/dashboard/api-keys.tsx). Core's
  // ApiKeysVm decides every fact and text: what the owner may see (the shown-once
  // key, the signing secret), which rows can be revoked, validity, labels and
  // tones. This view renders it and forwards typed intents; the desktop table and
  // phone cards are the only layout difference.
  import type { ApiKeysIntent } from "$lib/core/types/ApiKeysIntent";
  import type { ApiKeysVm } from "$lib/core/types/ApiKeysVm";
  import type { KeyRowVm } from "$lib/core/types/KeyRowVm";
  import type { OptionVm } from "$lib/core/types/OptionVm";
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
  import Badge from "$lib/ui/Badge.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import Select from "$lib/ui/Select.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import TextField from "$lib/ui/TextField.svelte";

  interface Props {
    vm: ApiKeysVm;
    phone?: boolean;
    onintent: (intent: ApiKeysIntent) => void;
  }

  let { vm, phone = false, onintent }: Props = $props();
  const t = (key: string) => i18n.tk("common", key);

  /** The option core marked selected; a native select needs its value. */
  function chosen<V extends string>(options: OptionVm<V>[]): V {
    return (options.find((option) => option.selected) ?? options[0]).value;
  }
  const choices = <V extends string>(options: OptionVm<V>[]) => options.map((option) => ({ value: option.value, label: i18n.t(option.label) }));
  const revoking = $derived(vm.rows.find((row) => row.id === vm.confirm_revoke)?.revoking ?? false);
</script>

{#snippet scopes(row: KeyRowVm)}
  <div class="flex flex-wrap gap-1">
    {#each row.scopes as scope}
      <Badge tone="accent">{scope.kind === "known" ? i18n.t(scope.label) : scope.value}</Badge>
    {:else}
      <Badge tone="neutral">{t("No Permissions")}</Badge>
    {/each}
  </div>
{/snippet}

{#snippet expiry(row: KeyRowVm)}
  <Badge tone={row.expiry_tone}>
    {#if row.expiry.kind === "never"}{t("Never")}{:else if row.expiry.kind === "expired"}{t("Expired")}{:else}{row.expiry.days} {t("Days")}{/if}
  </Badge>
{/snippet}

{#snippet revoke(row: KeyRowVm)}
  {#if row.can_revoke}
    <Button variant="danger" size="sm" busy={row.revoking} onclick={() => onintent({ type: "revoke_requested", id: row.id })}>{t("Revoke")}</Button>
  {/if}
{/snippet}

<section class="flex min-h-0 flex-1 flex-col gap-4 overflow-auto p-4" data-layout={phone ? "phone" : "desktop"}>
  <header>
    <h1 class="text-2xl font-semibold">{t("API Keys")}</h1>
    <p class="text-muted">{t("Manage your API keys for accessing the transcription service")}</p>
  </header>

  {#if vm.error}<Alert tone="error">{i18n.t(vm.error)}</Alert>{/if}
  {#if vm.notice}<Alert tone="success" ondismiss={() => onintent({ type: "notice_dismissed" })} dismissLabel={t("Dismiss")}>{i18n.t(vm.notice)}</Alert>{/if}

  {#if vm.created_key !== null}
    <section aria-labelledby="new-key-title" class="flex flex-col gap-3 rounded-lg border border-progress bg-notice-bg p-4">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h2 id="new-key-title" class="font-semibold">{t("Your new API key")}:</h2>
        <Button size="sm" onclick={() => onintent({ type: "copy_requested", what: "key" })}>{vm.key_copied ? t("Copied!") : t("Copy")}</Button>
      </div>
      <code class="select-all break-all rounded border border-border bg-bg p-3 text-sm">{vm.created_key}</code>
      <p class="text-sm text-muted">{t("This key will only be shown once. Please copy and store it securely.")}</p>
    </section>
  {/if}

  {#if vm.signing_secret !== null || vm.secret_error}
    <section aria-labelledby="secret-title" class="flex flex-col gap-3 rounded-lg border border-border bg-surface p-4">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h2 id="secret-title" class="font-semibold">{i18n.t(vm.secret_title)}</h2>
        {#if vm.signing_secret !== null}
          <Button size="sm" onclick={() => onintent({ type: "copy_requested", what: "secret" })}>{vm.secret_copied ? t("Copied!") : i18n.t(vm.secret_copy_label)}</Button>
        {/if}
      </div>
      {#if vm.secret_error}<Alert tone="error">{i18n.t(vm.secret_error)}</Alert>{/if}
      {#if vm.signing_secret !== null}
        <code class="select-all break-all rounded border border-border bg-bg p-3 text-sm">{vm.signing_secret}</code>
        <p class="text-sm text-muted">{i18n.t(vm.secret_description)}</p>
      {/if}
    </section>
  {/if}

  <section aria-labelledby="generate-title" class="flex flex-col gap-4 rounded-lg border border-border bg-surface p-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 id="generate-title" class="text-xl font-semibold">{t("Generate New Key")}</h2>
      <Button variant="primary" onclick={() => onintent({ type: "form_toggled" })}>{i18n.t(vm.form_toggle)}</Button>
    </div>
    {#if vm.form}
      {@const form = vm.form}
      <form class="flex flex-col gap-4" onsubmit={(event) => { event.preventDefault(); onintent({ type: "create_requested" }); }}>
        <TextField label={t("Key Name")} required placeholder={t("e.g. My App Key")} value={form.name} disabled={form.creating}
          oninput={(text) => onintent({ type: "name_changed", text })} />
        <Select label={t("Permissions")} value={chosen(form.scopes)} options={choices(form.scopes)} disabled={form.creating}
          onchange={(scope) => onintent({ type: "scope_chosen", scope })} />
        <Select label={t("Expiration Policy")} value={chosen(form.policies)} options={choices(form.policies)} disabled={form.creating}
          onchange={(policy) => onintent({ type: "policy_chosen", policy })} />
        {#if form.custom_days !== null}
          <TextField label={t("Custom Days")} inputmode="numeric" value={form.custom_days} disabled={form.creating}
            hint={form.custom_days_invalid ? undefined : t("Enter the number of days until the key expires (1-3650)")}
            error={form.custom_days_invalid ? t("Enter the number of days until the key expires (1-3650)") : null}
            oninput={(text) => onintent({ type: "custom_days_changed", text })} />
        {/if}
        <div><Button type="submit" variant="primary" busy={form.creating} disabled={!form.can_create && !form.creating}>{form.creating ? t("Loading...") : t("Generate Key")}</Button></div>
      </form>
    {/if}
  </section>

  <section aria-labelledby="keys-title" class="flex flex-col gap-3 rounded-lg border border-border bg-surface p-4">
    <h2 id="keys-title" class="text-xl font-semibold">{t("Your API Keys")}</h2>
    {#if vm.loading}
      <Skeleton label={t("Loading...")} />
    {:else if vm.empty}
      <p class="py-6 text-center text-muted">{t("No API keys found.")}</p>
    {:else if phone}
      <div class="flex flex-col gap-3">
        {#each vm.rows as row (row.id)}
          <article class={["flex flex-col gap-3 rounded-lg border border-border p-4", !row.active && "opacity-60"]} aria-label={row.name}>
            <h3 class="break-all font-mono text-sm font-semibold">{row.name}</h3>
            <dl class="grid grid-cols-2 items-center gap-2 text-sm">
              <dt>{t("Scopes")}</dt><dd>{@render scopes(row)}</dd>
              <dt>{t("Created")}</dt><dd>{new Date(row.created_at_ms).toLocaleString()}</dd>
              <dt>{t("Expires")}</dt><dd>{@render expiry(row)}</dd>
              <dt>{t("Status")}</dt><dd><Badge tone={row.status_tone}>{i18n.t(row.status)}</Badge></dd>
            </dl>
            {@render revoke(row)}
          </article>
        {/each}
      </div>
    {:else}
      <div class="overflow-x-auto">
        <table class="w-full text-start">
          <thead><tr class="border-b border-border">
            <th scope="col" class="p-3 text-start">{t("Name")}</th>
            <th scope="col" class="p-3 text-start">{t("Scopes")}</th>
            <th scope="col" class="p-3 text-start">{t("Created")}</th>
            <th scope="col" class="p-3 text-start">{t("Expires")}</th>
            <th scope="col" class="p-3 text-start">{t("Status")}</th>
            <th scope="col" class="p-3 text-start"><span class="sr-only">{i18n.tk("app", "Actions")}</span></th>
          </tr></thead>
          <tbody>
            {#each vm.rows as row (row.id)}
              <tr class={["border-b border-border", !row.active && "opacity-60"]}>
                <th scope="row" class="p-3 text-start font-mono text-sm font-normal">{row.name}</th>
                <td class="p-3">{@render scopes(row)}</td>
                <td class="p-3 text-sm text-muted">{new Date(row.created_at_ms).toLocaleString()}</td>
                <td class="p-3">{@render expiry(row)}</td>
                <td class="p-3"><Badge tone={row.status_tone}>{i18n.t(row.status)}</Badge></td>
                <td class="p-3">{@render revoke(row)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <p class="text-sm text-muted">
    {t("For more information on how to use API keys, visit our ")}<a class="underline" href="/api-docs">{t("API Documentation")}</a>.
  </p>
</section>

<ConfirmDialog
  open={vm.confirm_revoke !== null}
  title={t("Revoke API key")}
  description={t("Are you sure you want to revoke this API key?")}
  confirmLabel={t("Revoke")}
  cancelLabel={t("Cancel")}
  tone="danger"
  busy={revoking}
  kind={phone ? "sheet" : "dialog"}
  onconfirm={() => onintent({ type: "revoke_confirmed" })}
  oncancel={() => onintent({ type: "revoke_canceled" })}
/>
