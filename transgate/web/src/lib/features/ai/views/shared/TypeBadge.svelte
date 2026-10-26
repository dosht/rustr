<script lang="ts">
  // The detected-type chip beside "AI Analysis"; the source shows none for generic.
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Briefcase from "@lucide/svelte/icons/briefcase";
  import Headphones from "@lucide/svelte/icons/headphones";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Users from "@lucide/svelte/icons/users";
  import type { AnalysisType } from "$lib/core/types/AnalysisType";
  import { i18n } from "$lib/i18n/index.svelte";

  let { kind }: { kind: AnalysisType } = $props();

  const types = {
    lecture: { label: "Lecture", icon: BookOpen },
    meeting: { label: "Meeting", icon: Users },
    sales: { label: "Sales Call", icon: Briefcase },
    support: { label: "Support", icon: Headphones },
    interview: { label: "Interview", icon: MessageSquare },
  } as const;
  const config = $derived(kind === "generic" ? null : types[kind]);
</script>

{#if config}
  {@const Icon = config.icon}
  <span class="inline-flex items-center gap-1 rounded-full border border-accent px-2 py-0.5 text-xs font-medium text-accent" data-testid="analysis-type">
    <Icon size={12} aria-hidden="true" />
    {i18n.tk("ai", config.label)}
  </span>
{/if}
