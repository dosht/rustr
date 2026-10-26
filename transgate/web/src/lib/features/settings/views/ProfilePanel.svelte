<script lang="ts">
  // Settings "Profile" tab (source ProfileView.tsx:42-80, LanguageSwitcher.tsx:28-90):
  // email and "Joined on" (core's local d-m-yyyy), the shell's theme toggle and
  // core's 16-locale switcher, which reloads the page under the new prefix.
  import type { LocaleOption } from "$lib/core/types/LocaleOption";
  import { i18n } from "$lib/i18n/index.svelte";
  import ThemeToggle from "$lib/theme/ThemeToggle.svelte";
  import type { ThemeChoice } from "$lib/theme/theme.svelte";
  import Select from "$lib/ui/Select.svelte";

  interface Props {
    email: string;
    joinedOnText: string | null;
    theme: ThemeChoice;
    locales: readonly LocaleOption[];
    ontheme: (choice: ThemeChoice) => void;
    onlocale: (code: string) => void;
  }

  let { email, joinedOnText, theme, locales, ontheme, onlocale }: Props = $props();
  const c = (key: string) => i18n.tk("common", key);
  const app = (key: string) => i18n.tk("app", key);
  const current = $derived(locales.find((option) => option.current)?.code ?? i18n.locale);
  const options = $derived(locales.map((option) => ({ value: option.code, label: `${option.flag} ${i18n.t(option.name)}` })));
</script>

<div class="flex flex-col gap-6 font-medium">
  <dl class="flex flex-col gap-3">
    <div class="flex gap-1"><dt>{c("Email: ")}</dt><dd class="break-all">{email}</dd></div>
    <div class="flex gap-1"><dt>{c("Joined on: ")}</dt><dd>{joinedOnText ?? ""}</dd></div>
  </dl>

  <section class="flex flex-col gap-3">
    <h3 class="text-lg font-semibold">{app("Appearance")}</h3>
    <div class="flex items-center justify-between gap-4 rounded-lg bg-cursor-bg p-4">
      <div>
        <p>{app("Theme")}</p>
        <p class="text-sm font-normal text-muted">{app("Choose your preferred theme")}</p>
      </div>
      <ThemeToggle
        label={app("Theme")}
        choice={theme}
        names={{ light: app("Light mode"), dark: app("Dark mode"), system: app("System mode") }}
        onchoose={ontheme}
      />
    </div>
  </section>

  <Select label={c("Change Language")} value={current} {options} onchange={onlocale} />
</div>
