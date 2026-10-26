<script lang="ts">
  // Fixed Rust fixtures exercise the real root shell, not the wasm effects/runtime.
  import type { AppIntent, AppVm, TransgateCore } from "$lib/core";
  import { createMediaPlatform } from "$lib/platform/media";
  import { InvoicePreview } from "$lib/platform/preview.svelte";
  import { Telemetry } from "$lib/platform/telemetry.svelte";
  import { ThemeStore } from "$lib/theme/theme.svelte";
  import App from "./App.svelte";

  let { vm, onintent }: { vm: AppVm; onintent: (intent: AppIntent) => void } = $props();
  const listeners = new Set<(view: AppVm) => void>();
  const core: TransgateCore = {
    current: () => vm,
    dispatch(intent) { onintent(intent); return vm; },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    dispose() { listeners.clear(); },
  };
  $effect(() => { for (const listener of listeners) listener(vm); });
  const media = createMediaPlatform();
  const theme = new ThemeStore();
  const telemetry = new Telemetry({ posthog: null, sentry: null }, () => vm.locale);
  const preview = new InvoicePreview();
</script>

<App {core} {media} {theme} {telemetry} {preview} captcha={null} copy={async () => false} />
