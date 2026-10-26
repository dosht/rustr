<script lang="ts">
  // Source AIConsentView (settings AI tab), one responsive view as the source
  // was. D22: the settings action turns AI off AND deletes saved AI data from
  // the account (the revoking DELETE); a pending purge is reported as pending,
  // never as done. The confirmation's open state is view mechanics; a failed
  // write keeps the confirmed state because core keeps it (D21).
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import type { ConsentPanelVm } from "$lib/core/types/ConsentPanelVm";
  import Button from "$lib/ui/Button.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import { localDate } from "../../format";
  import type { ConsentIntent } from "../../intents";

  interface Props {
    consent: ConsentPanelVm;
    showTitle?: boolean;
    onintent: (intent: ConsentIntent) => void;
  }

  let { consent, showTitle = true, onintent }: Props = $props();
  let confirming = $state(false);
  const id = $props.id();

  // The confirmation closes once the server has recorded the revocation.
  $effect(() => {
    if (!consent.has_consented) confirming = false;
  });
</script>

{#if consent.gate === "loading"}
  <Skeleton label={i18n.tk("common", "Loading...")} lines={2} />
{:else if consent.load_error}
  <p role="alert" class="text-xs text-error sm:text-sm">{i18n.t(consent.load_error)}</p>
{:else}
  <div data-testid="ai-consent-settings">
    {#if showTitle}
      <h3 class="mb-3 text-base font-medium text-fg sm:mb-4 sm:text-lg">{i18n.tk("ai", "AI Features")}</h3>
    {/if}
    <div class="space-y-3 sm:space-y-4">
      <div class="flex items-start gap-3 rounded-lg bg-cursor-bg p-3 sm:items-center sm:p-4">
        <span aria-hidden="true" class="mt-1 h-3 w-3 shrink-0 rounded-full sm:mt-0 {consent.has_consented ? 'bg-success' : 'bg-muted'}"></span>
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium text-fg sm:text-base">
            {i18n.tk("ai", consent.has_consented ? "AI Features Enabled" : "AI Features Disabled")}
          </p>
          <p class="text-xs leading-relaxed text-muted sm:text-sm">
            {i18n.tk("ai", consent.has_consented ? "Summaries, highlights, and AI chat are available" : "Enable AI features to access summaries, highlights, and chat")}
          </p>
        </div>
      </div>

      {#if consent.cleanup_pending}
        <p role="status" class="rounded-lg border border-border bg-notice-bg p-2 text-xs sm:p-3 sm:text-sm">{i18n.tk("app", "p5.consent.cleanup_pending")}</p>
      {/if}

      {#if consent.provider_name}
        <dl class="space-y-1 text-xs text-muted sm:space-y-2 sm:text-sm">
          {#if consent.consent_date != null}
            <div class="break-words"><dt class="inline font-medium">{i18n.tk("ai", "Consent given:")}</dt> <dd class="inline">{localDate(consent.consent_date, i18n.locale)}</dd></div>
          {/if}
          {#if consent.revoked_date != null}
            <div class="break-words"><dt class="inline font-medium">{i18n.tk("ai", "Consent revoked:")}</dt> <dd class="inline">{localDate(consent.revoked_date, i18n.locale)}</dd></div>
          {/if}
          <div class="break-words"><dt class="inline font-medium">{i18n.tk("ai", "AI Provider:")}</dt> <dd class="inline">{consent.provider_name}</dd></div>
          {#if consent.consent_version}
            <div class="break-words"><dt class="inline font-medium">{i18n.tk("ai", "Consent Version:")}</dt> <dd class="inline">{consent.consent_version}</dd></div>
          {/if}
        </dl>
      {/if}

      {#if !consent.has_consented}
        <Button variant="primary" class="w-full" busy={consent.busy === "enable"} onclick={() => onintent({ type: "enable" })}>
          {i18n.tk("ai", consent.busy === "enable" ? "Enabling..." : "Enable AI Features")}
        </Button>
      {:else if !confirming}
        <Button variant="danger" class="w-full" disabled={!consent.can_revoke || consent.busy !== null} onclick={() => (confirming = true)}>
          {i18n.tk("app", "p5.consent.turn_off_delete_action")}
        </Button>
      {:else}
        <div role="alertdialog" aria-labelledby={`${id}-title`} aria-describedby={`${id}-body`} class="rounded-lg border border-error bg-error-bg p-3 sm:p-4">
          <div class="flex items-start gap-2 sm:gap-3">
            <TriangleAlert size={18} aria-hidden="true" class="shrink-0 text-error" />
            <div class="min-w-0 flex-1">
              <h4 id={`${id}-title`} class="text-xs font-medium text-fg sm:text-sm">{i18n.tk("app", "p5.consent.turn_off_delete_title")}</h4>
              <p id={`${id}-body`} class="mt-1 text-xs leading-relaxed text-fg sm:text-sm">{i18n.tk("app", "p5.consent.turn_off_delete_confirm")}</p>
              <div class="mt-3 flex flex-col gap-2 sm:flex-row">
                <Button variant="danger" size="sm" busy={consent.busy === "disable"} onclick={() => onintent({ type: "disable" })}>
                  {i18n.tk("app", "p5.consent.turn_off_delete_button")}
                </Button>
                <Button variant="secondary" size="sm" disabled={consent.busy !== null} onclick={() => (confirming = false)}>{i18n.tk("ai", "Cancel")}</Button>
              </div>
            </div>
          </div>
        </div>
      {/if}

      {#if consent.action_error}
        <div role="alert" class="rounded-lg border border-error bg-error-bg p-2 sm:p-3">
          <p class="text-xs text-fg sm:text-sm">{i18n.t(consent.action_error)}</p>
        </div>
      {/if}

      <p class="text-xs leading-relaxed text-muted">
        {i18n.tk("ai", "For more information about how we handle your data, please review our")}
        <a href={consent.privacy_policy_url} target="_blank" rel="noopener" class="break-words text-accent" style:text-decoration-line="underline">{i18n.tk("ai", "Privacy Policy")}</a>.
      </p>
    </div>
  </div>
{/if}
