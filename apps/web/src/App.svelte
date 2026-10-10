<script lang="ts">
  // Composition root: load the wasm core, build the model, hand it to the view.
  import { loadCore } from "./lib/core";
  import { createTaskListModel } from "./lib/tasks/taskListModel.svelte";
  import TaskListScreen from "./lib/tasks/TaskListScreen.svelte";

  const model = loadCore().then(createTaskListModel);
</script>

{#await model}
  <p class="boot">Loading…</p>
{:then model}
  <TaskListScreen view={model.view} onIntent={model.send} />
{:catch error}
  <p class="boot">Could not start: {error.message}</p>
{/await}

<style>
  .boot {
    text-align: center;
    padding: 48px 16px;
    color: var(--muted);
  }
</style>
