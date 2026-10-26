<script lang="ts">
  // Light / dark / system (source infrastructure/theming/components/ThemeToggle.tsx).
  // A shell mechanic: it reads and writes the shell's ThemeStore, not core.
  // The source's icon-only buttons get accessible names from their titles.
  import Monitor from "@lucide/svelte/icons/monitor";
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import IconButton from "$lib/ui/IconButton.svelte";
  import type { ThemeChoice } from "./theme.svelte";

  interface Props {
    /** Accessible name of the group, for example the translated "Theme". */
    label: string;
    choice: ThemeChoice;
    /** Translated names: "Light mode", "Dark mode", "System mode". */
    names: Record<ThemeChoice, string>;
    onchoose: (choice: ThemeChoice) => void;
  }

  let { label, choice, names, onchoose }: Props = $props();
  const options = [
    { value: "light", icon: Sun },
    { value: "dark", icon: Moon },
    { value: "system", icon: Monitor },
  ] as const;
</script>

<div role="group" aria-label={label} class="flex items-center gap-1">
  {#each options as option (option.value)}
    <IconButton
      size="sm"
      label={names[option.value]}
      icon={option.icon}
      pressed={choice === option.value}
      onclick={() => onchoose(option.value)}
      class="bg-cursor-bg"
    />
  {/each}
</div>
