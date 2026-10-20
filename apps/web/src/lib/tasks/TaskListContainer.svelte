<script lang="ts">
  // Container for the signed-out app: the local task list, desktop or phone layout.
  import type { TaskListModel } from "./taskListModel.svelte";
  import TaskListDesktop from "./views/desktop/TaskListDesktop.svelte";
  import TaskListMobile from "./views/mobile/TaskListMobile.svelte";

  interface Props {
    model: TaskListModel;
    mobile: boolean;
  }

  let { model, mobile }: Props = $props();
</script>

{#if mobile}
  <TaskListMobile view={model.view} onIntent={model.send} heading="taskr" />
{:else}
  <TaskListDesktop view={model.view} onIntent={model.send} />
{/if}
