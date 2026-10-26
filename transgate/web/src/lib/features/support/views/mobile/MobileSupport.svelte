<script lang="ts">
  import type { ComponentProps } from "svelte";
  import SupportDialog from "../SupportDialog.svelte";

  let props: Omit<ComponentProps<typeof SupportDialog>, "kind"> = $props();
</script>

<SupportDialog {...props} kind="sheet" />
