<script lang="ts">
  // Pure view, desktop layout of a signed-in screen: a sidebar with
  // workspaces, sections and the signed-in person, the open screen on the
  // right. Which workspace and section are selected comes from Rust.
  import type { AppIntentDto, SignedInView } from "../../../core";
  import MembersDesktop from "../../../members/views/desktop/MembersDesktop.svelte";
  import TaskListDesktop from "../../../tasks/views/desktop/TaskListDesktop.svelte";

  interface Props {
    view: SignedInView;
    onIntent: (intent: AppIntentDto) => Promise<void> | void;
  }

  let { view, onIntent }: Props = $props();

  const workspace = $derived(view.workspaces.find((w) => w.selected)?.name ?? "Tasks");
</script>

<div class="frame">
  <aside>
    <div class="brand">taskr</div>

    <nav aria-label="Workspaces">
      <h2>Workspaces</h2>
      {#each view.workspaces as w (w.id)}
        <button
          class="item"
          class:selected={w.selected}
          aria-current={w.selected ? "page" : undefined}
          onclick={() => onIntent({ type: "WorkspaceSelected", id: w.id })}
        >
          <span class="name">{w.name}</span>
          <span class="badge">{w.roleLabel}</span>
        </button>
      {/each}
    </nav>

    {#if view.screens.length}
      <nav aria-label="Sections">
        <h2>Sections</h2>
        {#each view.screens as screen (screen.screen)}
          <button
            class="item"
            class:selected={screen.selected}
            aria-current={screen.selected ? "page" : undefined}
            onclick={() => onIntent({ type: "ScreenSelected", screen: screen.screen })}
          >
            {screen.label}
          </button>
        {/each}
      </nav>
    {/if}

    <div class="me">
      <span class="email">{view.email}</span>
      <button class="link" onclick={() => onIntent({ type: "SignOutRequested" })}>Sign out</button>
    </div>
  </aside>

  <section class="content">
    {#if view.error}
      <div class="banner" role="alert">
        <span>{view.error}</span>
        <button class="link" onclick={() => onIntent({ type: "MessageDismissed" })} aria-label="Dismiss">×</button>
      </div>
    {/if}
    {#if view.screen.kind === "tasks" && view.workspace}
      {@const open = view.workspace}
      <TaskListDesktop
        view={view.screen.view}
        heading={workspace}
        onIntent={(intent) => onIntent({ type: "Tasks", workspace: open, intent })}
      />
    {:else if view.screen.kind === "members" && view.workspace}
      {@const open = view.workspace}
      <MembersDesktop
        view={view.screen.view}
        heading="Members"
        onIntent={(intent) => onIntent({ type: "Members", workspace: open, intent })}
      />
    {:else}
      <p class="loading">Loading…</p>
    {/if}
  </section>
</div>

<style>
  .frame {
    display: grid;
    grid-template-columns: 240px 1fr;
    min-height: 100vh;
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px 12px;
    border-right: 1px solid var(--border);
  }
  .brand {
    font-weight: 700;
    color: var(--accent);
    padding: 0 8px;
  }
  h2 {
    margin: 0 0 4px;
    padding: 0 8px;
    font-size: 0.7rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  nav {
    display: grid;
    gap: 2px;
  }
  .item {
    box-sizing: border-box;
    min-width: 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    color: var(--fg);
  }
  .item.selected {
    background: var(--cursor-bg);
    border-color: var(--border);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    font-size: 0.7rem;
    color: var(--muted);
  }
  .me {
    margin-top: auto;
    display: grid;
    gap: 4px;
    padding: 0 8px;
    font-size: 0.85rem;
  }
  .email {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    text-align: left;
  }
  .content {
    min-width: 0;
  }
  .banner {
    display: flex;
    justify-content: space-between;
    margin: 16px;
    padding: 8px 12px;
    border-radius: 6px;
    background: var(--error-bg);
    color: var(--error);
  }
  .loading {
    text-align: center;
    padding: 48px 16px;
    color: var(--muted);
  }
</style>
