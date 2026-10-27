<script lang="ts">
  import type { ComponentProps } from "svelte";
  import ApiKeysPage from "../ApiKeysPage.svelte";
  let props: Omit<ComponentProps<typeof ApiKeysPage>, "phone"> = $props();
</script>
<ApiKeysPage {...props} phone={true} />
