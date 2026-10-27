<script lang="ts">
  // Phone Ask AI view (source MobileChatView): pure, props in and intents out.
  // The draft is bindable so the container keeps it across tab switches;
  // follow-scroll and the scroll-to-latest pill are view mechanics.
  import { tick, untrack } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Send from "@lucide/svelte/icons/send";
  import type { ChatPanelVm } from "$lib/core/types/ChatPanelVm";
  import type { ConsentPanelVm } from "$lib/core/types/ConsentPanelVm";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { ChatIntent, ConsentIntent } from "../../intents";
  import AiGate from "../shared/AiGate.svelte";
  import ChatMessage from "../shared/ChatMessage.svelte";

  interface Props {
    consent: ConsentPanelVm;
    view: ChatPanelVm;
    draft?: string;
    /** Changing it scrolls back to the top (another job). */
    scrollResetKey?: number;
    /** Space taken by the phone's bottom navigation, in pixels. */
    bottomOffset?: number;
    onconsent: (intent: ConsentIntent) => void;
    onintent: (intent: ChatIntent) => void;
  }

  let { consent, view, draft = $bindable(""), scrollResetKey = 0, bottomOffset = 0, onconsent, onintent }: Props = $props();

  let scroller: HTMLDivElement | undefined = $state();
  let scrolledUp = $state(false);
  let pill = $state(false);
  const busy = $derived(!view.can_send);

  $effect(() => {
    void scrollResetKey;
    if (scroller) scroller.scrollTop = 0;
    scrolledUp = false;
    pill = false;
  });

  $effect(() => {
    void view.lines;
    if (untrack(() => scrolledUp)) pill = true;
    else tick().then(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  $effect(() => {
    if (!view.notice) return;
    const timer = setTimeout(() => onintent({ type: "dismiss_notice" }), 2000);
    return () => clearTimeout(timer);
  });

  function scrolled(event: Event & { currentTarget: HTMLDivElement }) {
    const el = event.currentTarget;
    const up = el.scrollHeight - el.scrollTop - el.clientHeight >= 100;
    if (up !== scrolledUp) pill = up;
    scrolledUp = up;
  }

  function toBottom() {
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
    scrolledUp = false;
    pill = false;
  }

  function send() {
    if (!draft.trim() || busy || view.input_locked) return;
    const text = draft;
    draft = "";
    onintent({ type: "send", text });
  }
</script>

<AiGate {consent} feature="chat" phone onintent={onconsent}>
  <div class="flex h-full flex-col" data-testid="chat-panel">
    <div class="flex items-center justify-between border-b border-border px-3 py-2">
      <div class="flex items-center gap-2">
        <span aria-hidden="true" class="flex h-7 w-7 items-center justify-center rounded-full bg-cursor-bg text-accent"><MessageSquare size={14} /></span>
        <h4 class="text-sm font-semibold text-fg">{i18n.tk("ai", "Ask AI")}</h4>
      </div>
      {#if view.lines.length > 0}
        <div class="flex items-center gap-1">
          <IconButton label={i18n.tk("ai", "Copy chat")} icon={Copy} onclick={() => onintent({ type: "copy_chat" })} />
          <IconButton label={i18n.tk("ai", "Download chat")} icon={Download} onclick={() => onintent({ type: "download" })} />
        </div>
      {/if}
    </div>

    <div class="relative flex-1 overflow-hidden">
      <div bind:this={scroller} class="h-full overflow-y-auto scroll-smooth px-3 py-3" onscroll={scrolled} role="log" aria-live="polite">
        <div class="space-y-4" style:padding-bottom={`${bottomOffset + 80}px`}>
          {#if view.loading_history}
            <Skeleton label={i18n.tk("common", "Loading...")} lines={3} />
          {:else if view.lines.length === 0 && !busy}
            <div class="flex flex-col items-center py-10 text-center">
              <span aria-hidden="true" class="mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-cursor-bg text-accent"><MessageSquare size={22} /></span>
              <h3 class="text-base font-semibold text-fg">{i18n.tk("ai", "Start a conversation")}</h3>
              <p class="mt-1 max-w-xs text-sm text-muted">{i18n.tk("ai", "Ask questions about your transcript and get AI-powered insights")}</p>
            </div>
          {/if}
          {#each view.lines as line, index (index)}
            <ChatMessage
              {line}
              phone
              oncopy={() => onintent({ type: "copy_message", index })}
              onretry={() => onintent({ type: "retry", index })}
              onrate={(rating) => onintent({ type: "rate", index, rating })}
            />
          {/each}
          {#if view.typing}
            <div role="status" class="flex items-center gap-1 pl-9 text-muted" aria-label={i18n.tk("ai", "AI is thinking...")}>
              <span class="h-2 w-2 animate-bounce rounded-full bg-muted"></span>
              <span class="h-2 w-2 animate-bounce rounded-full bg-muted [animation-delay:150ms]"></span>
              <span class="h-2 w-2 animate-bounce rounded-full bg-muted [animation-delay:300ms]"></span>
            </div>
          {/if}
          {#if view.error}
            <div role="alert" class="flex items-start gap-2 rounded-lg border border-error bg-error-bg p-3">
              <CircleAlert size={18} aria-hidden="true" class="shrink-0 text-error" />
              <div class="min-w-0 flex-1">
                <p class="text-sm font-medium text-fg">{i18n.tk("ai", "Failed to get response")}</p>
                {#if view.error.message}<p class="mt-1 text-xs text-muted">{i18n.t(view.error.message)}</p>{/if}
                {#if view.error.detail}<p class="mt-1 break-words text-xs text-muted">{view.error.detail}</p>{/if}
              </div>
              <IconButton label={i18n.tk("ai", "Dismiss")} icon={RefreshCw} onclick={() => onintent({ type: "dismiss_error" })} />
            </div>
          {/if}
          {#if view.sync_warning}
            <p role="status" class="rounded-lg border border-progress bg-notice-bg p-2 text-xs text-fg">{i18n.t(view.sync_warning)}</p>
          {/if}
        </div>
      </div>
      {#if pill}
        <button
          type="button"
          class="absolute right-4 flex h-10 w-10 items-center justify-center rounded-full bg-accent text-on-accent shadow"
          style:bottom={`${bottomOffset + 80}px`}
          aria-label={i18n.tk("app", "Scroll to latest message")}
          onclick={toBottom}
        >
          <ArrowDown size={18} aria-hidden="true" />
        </button>
      {/if}
    </div>

    {#if view.notice}
      <p role="status" class="mx-3 mb-2 text-center text-xs text-muted">{i18n.t(view.notice)}</p>
    {/if}

    <div class="flex items-end gap-2 border-t border-border bg-surface p-2" style:margin-bottom={`${bottomOffset}px`}>
      <textarea
        bind:value={draft}
        rows={1}
        class="max-h-32 min-h-11 flex-1 resize-none rounded-md border border-border bg-bg px-3 py-2 text-base text-fg"
        placeholder={i18n.tk("ai", "Ask about the transcript...")}
        aria-label={i18n.tk("ai", "Ask about the transcript...")}
        disabled={view.input_locked}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
      ></textarea>
      <IconButton label={i18n.tk("common", "Send message")} icon={Send} disabled={!draft.trim() || busy || view.input_locked} onclick={send} />
    </div>
  </div>
</AiGate>
