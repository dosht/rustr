<script lang="ts">
  // The settings dialog (source SettingsView.tsx:170-230): a titled dialog with
  // the tabs core says exist (the AI tab only with its capability). Each tab's
  // body is composed by the container: profile and Manage Data here, the AI
  // tab from p21, the subscription tab from p1R.
  import type { Snippet } from "svelte";
  import X from "@lucide/svelte/icons/x";
  import type { SettingsVm } from "$lib/core/types/SettingsVm";
  import type { Tab } from "$lib/core/types/Tab";
  import { i18n } from "$lib/i18n/index.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Tabs from "$lib/ui/Tabs.svelte";

  interface Props {
    vm: SettingsVm;
    kind: "dialog" | "sheet";
    onselect: (tab: Tab) => void;
    onclose: () => void;
    panel: Snippet<[Tab]>;
  }

  let { vm, kind, onselect, onclose, panel }: Props = $props();
  const title = $derived(i18n.t(vm.title));
  const tabs = $derived(vm.tabs.map((tab) => ({ id: tab.tab, label: i18n.t(tab.label) })));
</script>

<Dialog open={vm.open} {kind} {title} {onclose}>
  <div class="absolute top-3 right-3">
    <IconButton label={i18n.tk("common", "Close")} icon={X} size="sm" onclick={onclose} />
  </div>
  <Tabs label={title} {tabs} selected={vm.active} {onselect} {panel} />
</Dialog>
