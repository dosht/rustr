<script lang="ts">
  import type { ConfirmMessages } from "$lib/core/types/ConfirmMessages";
  import { i18n } from "$lib/i18n/index.svelte";
  import Skeleton from "$lib/ui/Skeleton.svelte";

  let { messages }: { messages: ConfirmMessages | null } = $props();
</script>

<main class="flex min-h-dvh items-center justify-center p-6">
  <section class="flex w-full max-w-lg flex-col gap-4 rounded-lg border border-border bg-surface p-6 text-center">
    {#if messages}
      <h1 class="text-2xl font-bold">{i18n.t(messages.title)}</h1>
      {#if messages.status}<h2 class="text-xl font-semibold">{i18n.t(messages.status)}</h2>{/if}
      <p role="status">{i18n.t(messages.message)}</p>
      {#if messages.detail}<p>{i18n.t(messages.detail)}</p>{/if}
      {#if messages.redirect}<p class="text-sm text-muted">{i18n.t(messages.redirect)}</p>{/if}
    {:else}
      <Skeleton label={i18n.tk("common", "Loading...")} />
    {/if}
  </section>
</main>
