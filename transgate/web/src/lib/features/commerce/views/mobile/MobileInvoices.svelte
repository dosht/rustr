<script lang="ts">
  import type { ComponentProps } from "svelte";
  import InvoicesPage from "../InvoicesPage.svelte";
  let props: Omit<ComponentProps<typeof InvoicesPage>, "phone"> = $props();
</script>
<InvoicesPage {...props} phone />
