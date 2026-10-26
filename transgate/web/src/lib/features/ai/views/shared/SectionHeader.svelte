<script lang="ts">
  // The source's SectionHeader: a translated title, an optional count and the
  // section's own copy button. Core formats the copied text; the check mark
  // for two seconds is view mechanics.
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import type { SectionId } from "$lib/core/types/SectionId";
  import IconButton from "$lib/ui/IconButton.svelte";
  import { i18n } from "$lib/i18n/index.svelte";

  interface Props {
    /** An `ai` catalog key. */
    title: string;
    count?: number;
    /** Sections with a copy button in the source name their id. */
    section?: SectionId;
    oncopy?: (section: SectionId) => void;
  }

  let { title, count, section, oncopy }: Props = $props();
  let copied = $state(false);

  $effect(() => {
    if (!copied) return;
    const timer = setTimeout(() => (copied = false), 2000);
    return () => clearTimeout(timer);
  });

  function copy() {
    if (!section || !oncopy) return;
    oncopy(section);
    copied = true;
  }
</script>

<div class="mb-3 flex items-center justify-between">
  <h3 class="text-sm font-semibold text-fg">
    {i18n.tk("ai", title)}{count !== undefined ? ` (${count})` : ""}
  </h3>
  {#if section && oncopy}
    <IconButton label={i18n.tk("ai", "Copy to clipboard")} icon={copied ? Check : Copy} size="sm" onclick={copy} />
  {/if}
</div>
