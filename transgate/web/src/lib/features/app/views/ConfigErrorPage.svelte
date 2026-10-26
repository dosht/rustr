<script lang="ts">
  // Shown when the host supplied no valid tg-config: a deployment fault, so
  // nothing else starts (web-shell.md section 7). New state, new `app` key.
  import { i18n } from "$lib/i18n/index.svelte";
  import Alert from "$lib/ui/Alert.svelte";
</script>

<main class="flex min-h-dvh items-center justify-center p-4">
  <div class="w-full max-w-md">
    <Alert tone="error">{i18n.tk("app", "p5.web.config_unavailable")}</Alert>
  </div>
</main>
