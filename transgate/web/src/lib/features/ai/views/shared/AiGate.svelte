<script lang="ts">
  // Source AIFeatureWrapper + AIIntroduction + AIConsentModal: a spinner until
  // consent is known, the introduction while it is required, else the panel.
  // The learn-more modal's open state is view mechanics; decisions are intents.
  import type { Snippet } from "svelte";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Info from "@lucide/svelte/icons/info";
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import type { ConsentPanelVm } from "$lib/core/types/ConsentPanelVm";
  import Button from "$lib/ui/Button.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { ConsentIntent } from "../../intents";

  interface Props {
    consent: ConsentPanelVm;
    feature: "summary" | "chat";
    /** The modal docks as a bottom sheet on phones. */
    phone?: boolean;
    onintent: (intent: ConsentIntent) => void;
    children: Snippet;
  }

  let { consent, feature, phone = false, onintent, children }: Props = $props();
  let learnMore = $state(false);

  const features = {
    summary: {
      icon: Sparkles,
      title: "AI Summary",
      description: "Get an intelligent summary of your transcript content",
      benefits: ["Automatically extracts key points", "Saves time reading long transcripts", "Highlights main discussion topics"],
      button: "Start Summarizing",
    },
    chat: {
      icon: MessageCircle,
      title: "AI Chat Assistant",
      description: "Ask questions about your transcript content",
      benefits: ["Interactive Q&A about content", "Get clarifications instantly", "Explore topics in depth"],
      button: "Start Chatting",
    },
  } as const;
  const config = $derived(features[feature]);
  const Icon = $derived(config.icon);
  const enabling = $derived(consent.busy === "enable");

  function decide(intent: ConsentIntent) {
    learnMore = false;
    onintent(intent);
  }
</script>

{#if consent.gate === "loading"}
  <div class="flex items-center justify-center p-8" role="status">
    <div aria-hidden="true" class="h-8 w-8 animate-spin rounded-full border-b-2 border-accent"></div>
    <span class="sr-only">{i18n.tk("common", "Loading...")}</span>
  </div>
{:else if consent.gate === "granted"}
  {@render children()}
{:else}
  <div class="bg-surface" data-testid="ai-introduction">
    <div class="flex flex-col items-center gap-3 px-4 pb-4 pt-6 sm:px-6 sm:pb-6 sm:pt-8">
      <Button variant="primary" busy={enabling} onclick={() => onintent({ type: "enable" })} class="w-full sm:w-auto">
        <Sparkles size={16} aria-hidden="true" />
        <span>{i18n.tk("ai", config.button)}</span>
        <ArrowRight size={16} aria-hidden="true" />
      </Button>
      <div class="flex flex-wrap items-center justify-center gap-2 text-xs text-muted">
        {#if consent.provider_name}
          <button type="button" class="flex items-center gap-1 font-medium text-accent hover:underline" onclick={() => (learnMore = true)}>
            <Info size={12} aria-hidden="true" />
            <span>{i18n.tk("ai", "Learn More")}</span>
          </button>
          <span aria-hidden="true">|</span>
        {/if}
        <span>{i18n.tk("ai", "AI features process your transcript content securely. You can disable them anytime in settings.")}</span>
      </div>
      {#if consent.action_error}
        <p role="alert" class="text-xs text-error">{i18n.t(consent.action_error)}</p>
      {/if}
    </div>
    <div class="flex flex-col items-center px-4 pb-6 text-center sm:px-6 sm:pb-8">
      <Icon size={40} aria-hidden="true" class="mb-3 text-progress" />
      <h3 class="mb-2 text-base font-bold text-fg sm:text-xl">{i18n.tk("ai", config.title)}</h3>
      <p class="mb-4 max-w-md text-xs leading-relaxed text-muted sm:text-sm">{i18n.tk("ai", config.description)}</p>
      <ul class="space-y-1 sm:space-y-2">
        {#each config.benefits as benefit (benefit)}
          <li class="flex items-center gap-2 text-xs text-fg sm:text-sm"><span aria-hidden="true" class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent"></span>{i18n.tk("ai", benefit)}</li>
        {/each}
      </ul>
    </div>
  </div>

  {#if consent.provider_name}
    <Dialog open={learnMore} title={i18n.tk("ai", "AI Features Consent")} kind={phone ? "sheet" : "dialog"} onclose={() => (learnMore = false)}>
      <div class="space-y-4 text-sm">
        <div class="rounded-md border border-border bg-notice-bg p-3">
          <h3 class="font-medium">{i18n.tk("ai", "AI Processing Information")}</h3>
          <p class="mt-1 text-muted">
            {i18n.tk("ai", "To provide AI-powered features like summaries, highlights, and chat assistance, we need to send your transcript content to our AI service provider")}
            ({consent.provider_name}).
          </p>
        </div>
        <div>
          <h4 class="font-medium">{i18n.tk("ai", "What this means:")}</h4>
          <!-- D22 disclosure (core.md 1.1a): these replace the source's processing,
               provider-permanence, browser-local and "revoke and delete at any time"
               bullets; never render them beside the replaced sentences. -->
          <ul class="mt-1 list-disc space-y-1 pl-5 text-muted">
            <li>{i18n.tk("app", "p5.consent.provider_processing", { provider: consent.provider_name })}</li>
            <li>{i18n.tk("ai", "We use secure HTTPS connections to protect your data in transit")}</li>
            <li>{i18n.tk("app", "p5.consent.provider_retention")}</li>
            <li>{i18n.tk("app", "p5.consent.account_results")}</li>
            <li>{i18n.tk("app", "p5.consent.decline_keeps")}</li>
            <li>{i18n.tk("app", "p5.consent.settings_delete")}</li>
          </ul>
        </div>
        <div>
          <h4 class="font-medium">{i18n.tk("ai", "AI Features include:")}</h4>
          <ul class="mt-1 list-disc space-y-1 pl-5 text-muted">
            <li>{i18n.tk("ai", "Automatic transcript summaries")}</li>
            <li>{i18n.tk("ai", "Key highlights extraction")}</li>
            <li>{i18n.tk("ai", "Interactive AI chat assistant")}</li>
          </ul>
        </div>
        <p class="text-xs text-muted">
          {i18n.tk("ai", "By accepting, you agree to the processing of your transcript data as described above. For more information, please review our")}
          <a href={consent.privacy_policy_url} target="_blank" rel="noopener" class="text-accent hover:underline">{i18n.tk("common", "Privacy Policy")}</a>.
        </p>
      </div>
      {#snippet actions()}
        <Button variant="secondary" onclick={() => decide({ type: "decline" })}>{i18n.tk("ai", "Decline")}</Button>
        <Button variant="primary" onclick={() => decide({ type: "enable" })}>{i18n.tk("ai", "Accept & Enable AI Features")}</Button>
      {/snippet}
    </Dialog>
  {/if}
{/if}
