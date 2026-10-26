<script lang="ts">
  import type { ComponentProps } from "svelte";
  import SubscriptionPanel from "../SubscriptionPanel.svelte";
  let props: Omit<ComponentProps<typeof SubscriptionPanel>, "phone"> = $props();
</script>
<SubscriptionPanel {...props} phone />
