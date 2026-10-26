<script lang="ts">
  // Source shared ChatMessage: user bubble, assistant bubble (streaming text
  // with a cursor, finished text as escaped simple markdown) and the actions
  // toolbar, hover-revealed on desktop and always visible on phones.
  import Bot from "@lucide/svelte/icons/bot";
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ThumbsDown from "@lucide/svelte/icons/thumbs-down";
  import ThumbsUp from "@lucide/svelte/icons/thumbs-up";
  import User from "@lucide/svelte/icons/user";
  import type { ChatLineVm } from "$lib/core/types/ChatLineVm";
  import type { Rating } from "$lib/core/types/Rating";
  import { i18n } from "$lib/i18n/index.svelte";
  import { simpleMarkdown } from "../../markdown";

  interface Props {
    line: ChatLineVm;
    phone?: boolean;
    oncopy: () => void;
    onretry: () => void;
    onrate: (rating: Rating) => void;
  }

  let { line, phone = false, oncopy, onretry, onrate }: Props = $props();
  let copied = $state(false);

  $effect(() => {
    if (!copied) return;
    const timer = setTimeout(() => (copied = false), 2000);
    return () => clearTimeout(timer);
  });

  function copy() {
    oncopy();
    copied = true;
  }

  const action = "inline-flex min-h-8 items-center gap-1 rounded px-1.5 text-xs text-muted hover:bg-cursor-bg hover:text-fg";
</script>

{#if line.role === "user"}
  <div class="flex items-start justify-end gap-2" data-testid="chat-message-user">
    <div class="rounded-lg bg-accent px-3 py-2 text-sm text-on-accent {phone ? 'max-w-[90%]' : 'max-w-[85%]'}">
      <p class="whitespace-pre-wrap break-words">{line.content}</p>
    </div>
    <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-cursor-bg text-muted" role="img" aria-label={i18n.tk("ai", "User avatar")}>
      <User size={14} aria-hidden="true" />
    </span>
  </div>
{:else}
  <div class="group" data-testid="chat-message-assistant">
    <div class="flex items-start gap-2">
      <span aria-hidden="true" class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-cursor-bg text-accent"><Bot size={14} /></span>
      <div class="max-w-[90%] rounded-lg border border-border bg-surface px-3 py-2 text-sm text-fg">
        {#if line.streaming}
          <div class="whitespace-pre-wrap break-words">
            <span>{line.content}</span>
            <span class="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-accent align-middle" role="img" aria-label={i18n.tk("ai", "AI is typing")}></span>
          </div>
        {:else}
          <!-- simpleMarkdown escapes the answer before adding its own tags. -->
          <div class="ai-markdown whitespace-pre-wrap break-words">{@html simpleMarkdown(line.content)}</div>
        {/if}
      </div>
    </div>
    {#if line.show_actions && !line.streaming}
      <div
        role="toolbar"
        aria-label={i18n.tk("ai", "Message actions")}
        class="ml-9 mt-1 flex items-center gap-1 {phone ? '' : 'opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100'}"
      >
        <button type="button" class={action} title={i18n.tk("ai", "Copy message")} onclick={copy}>
          {#if copied}<Check size={12} aria-hidden="true" />{:else}<Copy size={12} aria-hidden="true" />{/if}
          {i18n.tk("ai", "Copy")}
        </button>
        <button type="button" class={action} onclick={onretry}>
          <RotateCcw size={12} aria-hidden="true" />
          {i18n.tk("ai", "Retry")}
        </button>
        <button type="button" class={action} aria-label={i18n.tk("ai", "Helpful")} onclick={() => onrate("like")}>
          <ThumbsUp size={12} aria-hidden="true" />
        </button>
        <button type="button" class={action} aria-label={i18n.tk("ai", "Not helpful")} onclick={() => onrate("dislike")}>
          <ThumbsDown size={12} aria-hidden="true" />
        </button>
      </div>
    {/if}
  </div>
{/if}
