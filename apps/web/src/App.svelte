<script lang="ts">
  // Composition root. With VITE_TASKR_API set, the app signs in to a taskr
  // server; without it, tasks stay in this browser (the original local mode).
  import { loadApp, loadCore, type AppConfig } from "./lib/core";
  import AppContainer from "./lib/app/AppContainer.svelte";
  import { createAppModel } from "./lib/app/appModel.svelte";
  import { createViewport } from "./lib/platform/viewport.svelte";
  import TaskListContainer from "./lib/tasks/TaskListContainer.svelte";
  import { createTaskListModel } from "./lib/tasks/taskListModel.svelte";

  const env = import.meta.env;
  const absolute = (url: string) => new URL(url, window.location.origin).href.replace(/\/$/, "");

  const config: AppConfig | null = env.VITE_TASKR_API
    ? {
        apiUrl: absolute(env.VITE_TASKR_API),
        authUrl: env.VITE_TASKR_AUTH_URL ? absolute(env.VITE_TASKR_AUTH_URL) : undefined,
        authKey: env.VITE_TASKR_AUTH_KEY || undefined,
      }
    : null;

  const viewport = createViewport();
  const app = config ? loadApp(config).then(createAppModel) : null;
  const local = config ? null : loadCore().then(createTaskListModel);
</script>

{#if app}
  {#await app}
    <p class="boot">Loading…</p>
  {:then model}
    <AppContainer {model} mobile={viewport.mobile} devSignIn={!config?.authUrl} />
  {:catch error}
    <p class="boot">Could not start: {error.message}</p>
  {/await}
{:else if local}
  {#await local}
    <p class="boot">Loading…</p>
  {:then model}
    <TaskListContainer {model} mobile={viewport.mobile} />
  {:catch error}
    <p class="boot">Could not start: {error.message}</p>
  {/await}
{/if}

<style>
  .boot {
    text-align: center;
    padding: 48px 16px;
    color: var(--muted);
  }
</style>
