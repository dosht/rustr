<script lang="ts">
  // Desktop Ask AI panel (source DesktopChatView): pure, props in and intents
  // out. The draft and the scroll offset are bindable so the container keeps
  // them across panel collapse; follow-scroll and the pill are view mechanics.
  import { untrack } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Download from "@lucide/svelte/icons/download";
  import Copy from "@lucide/svelte/icons/copy";
  import MessageCircle from "@lucide/svelte/icons/message-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Send from "@lucide/svelte/icons/send";
  import type { ChatPanelVm } from "$lib/core/types/ChatPanelVm";
  import type { ConsentPanelVm } from "$lib/core/types/ConsentPanelVm";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { ChatIntent, ConsentIntent } from "../../intents";
  import AiGate from "../shared/AiGate.svelte";
  import ChatLimits from "../shared/ChatLimits.svelte";
  import ChatMessage from "../shared/ChatMessage.svelte";

  interface Props {
    consent: ConsentPanelVm;
    view: ChatPanelVm;
    draft?: string;
    scrollTop?: number;
    onconsent: (intent: ConsentIntent) => void;
    onintent: (intent: ChatIntent) => void;
  }

  let { consent, view, draft = $bindable(""), scrollTop = $bindable(0), onconsent, onintent }: Props = $props();

  const THRESHOLD = 80;
  let scroller: HTMLDivElement | undefined = $state();
  let input: HTMLTextAreaElement | undefined = $state();
  let follow = $state(false);
  let pill = $state(false);
  const busy = $derived(!view.can_send);

  // Restore the persisted offset once, when the list mounts.
  $effect(() => {
    if (scroller) scroller.scrollTop = untrack(() => scrollTop);
  });

  // Follow the streaming answer while the reader stays at the bottom.
  $effect(() => {
    void view.lines;
    void busy;
    if (follow && scroller) scroller.scrollTop = scroller.scrollHeight;
  });

  $effect(() => {
    if (!view.notice) return;
    const timer = setTimeout(() => onintent({ type: "dismiss_notice" }), 2000);
    return () => clearTimeout(timer);
  });

  function wheel(event: WheelEvent) {
    if (!scroller) return;
    if (event.deltaY < 0) {
      follow = false;
      pill = true;
    } else if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - THRESHOLD) {
      follow = true;
      pill = false;
    }
  }

  function toBottom() {
    follow = true;
    pill = false;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }

  function send() {
    if (!draft.trim() || busy || !view.enabled) return;
    const text = draft;
    draft = "";
    input?.focus();
    toBottom();
    onintent({ type: "send", text });
  }
</script>

<AiGate {consent} feature="chat" onintent={onconsent}>
  <div class="flex h-full flex-col" data-testid="chat-panel">
    {#if view.show_history_header}
      <div class="flex items-center justify-between border-b border-border px-4 py-2">
        <h3 class="text-sm font-semibold text-fg">{i18n.tk("ai", "Chat History")}</h3>
        <div class="flex items-center gap-1">
          <IconButton label={i18n.tk("ai", "Copy chat")} icon={Copy} size="sm" onclick={() => onintent({ type: "copy_chat" })} />
          <IconButton label={i18n.tk("ai", "Download chat")} icon={Download} size="sm" onclick={() => onintent({ type: "download" })} />
        </div>
      </div>
    {/if}

    {#if view.show_welcome}
      <div class="m-4 rounded-lg border border-border bg-notice-bg p-4">
        <div class="mb-2 flex items-center gap-2">
          <span aria-hidden="true" class="flex h-7 w-7 items-center justify-center rounded-full bg-accent text-on-accent"><MessageCircle size={14} /></span>
          <h3 class="text-sm font-semibold text-fg">{i18n.tk("ai", "Ask AI Assistant")}</h3>
        </div>
        <p class="text-sm text-muted">{i18n.tk("ai", "Hello! I can help analyze this transcript. Ask me anything about the content.")}</p>
      </div>
    {/if}

    {#if view.limits}
      <div class="mx-4 mb-2"><ChatLimits limits={view.limits} /></div>
    {/if}

    <div class="relative flex-1 overflow-hidden">
      <div
        bind:this={scroller}
        class="h-full overflow-y-auto px-4 py-3"
        onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
        onwheel={wheel}
        role="log"
        aria-live="polite"
      >
        <div class="space-y-4">
          {#if view.loading_history}
            <Skeleton label={i18n.tk("common", "Loading...")} lines={3} />
          {/if}
          {#each view.lines as line, index (index)}
            <ChatMessage
              {line}
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
              <IconButton label={i18n.tk("ai", "Dismiss")} icon={RefreshCw} size="sm" onclick={() => onintent({ type: "dismiss_error" })} />
            </div>
          {/if}
          {#if view.sync_warning}
            <p role="status" class="rounded-lg border border-progress bg-notice-bg p-2 text-xs text-fg">{i18n.t(view.sync_warning)}</p>
          {/if}
        </div>
      </div>
      {#if busy && pill}
        <button type="button" class="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-1 rounded-full bg-accent px-3 py-1 text-xs text-on-accent shadow" onclick={toBottom}>
          <ArrowDown size={12} aria-hidden="true" />
          {i18n.tk("ai", "New message")}
        </button>
      {/if}
    </div>

    {#if view.notice}
      <p role="status" class="mx-4 mb-2 text-center text-xs text-muted">{i18n.t(view.notice)}</p>
    {/if}

    <div class="flex items-end gap-2 border-t border-border p-3">
      <textarea
        bind:this={input}
        bind:value={draft}
        rows={2}
        class="min-h-[60px] flex-1 resize-none rounded-md border border-border bg-bg px-3 py-2 text-sm text-fg"
        placeholder={i18n.tk("ai", "Ask about the transcript...")}
        aria-label={i18n.tk("ai", "Ask about the transcript...")}
        disabled={!view.enabled}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
      ></textarea>
      <IconButton label={i18n.tk("common", "Send message")} icon={Send} disabled={!draft.trim() || busy || !view.enabled} onclick={send} />
    </div>
  </div>
</AiGate>
