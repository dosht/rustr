<script lang="ts">
  import type { ComponentProps } from "svelte";
  import FeedbackDialog from "../FeedbackDialog.svelte";

  let props: Omit<ComponentProps<typeof FeedbackDialog>, "kind"> = $props();
</script>

<FeedbackDialog {...props} kind="dialog" />
