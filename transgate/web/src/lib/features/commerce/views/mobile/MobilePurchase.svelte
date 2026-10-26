<script lang="ts">
  import type { ComponentProps } from "svelte";
  import PurchaseDialog from "../PurchaseDialog.svelte";
  let props: Omit<ComponentProps<typeof PurchaseDialog>, "phone"> = $props();
</script>
<PurchaseDialog {...props} phone />
