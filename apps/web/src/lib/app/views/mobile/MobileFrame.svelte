<script lang="ts">
  // Pure view, phone layout of a signed-in screen: a title bar, a drawer for
  // workspaces and the account, and a bottom tab bar for sections. Only
  // whether the drawer is open is local.
  import type { AppIntentDto, SignedInView } from "../../../core";
  import MembersMobile from "../../../members/views/mobile/MembersMobile.svelte";
  import TaskListMobile from "../../../tasks/views/mobile/TaskListMobile.svelte";

  interface Props {
    view: SignedInView;
    onIntent: (intent: AppIntentDto) => Promise<void> | void;
  }

  let { view, onIntent }: Props = $props();

  let drawerOpen = $state(false);

  function go(intent: AppIntentDto) {
    drawerOpen = false;
    onIntent(intent);
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && (drawerOpen = false)} />

<div class="frame">
  <header class="topbar">
    <button class="menu" aria-label="Open workspaces" aria-expanded={drawerOpen} onclick={() => (drawerOpen = true)}>
      ☰
    </button>
    <span class="title">{view.title}</span>
  </header>

  {#if view.error}
    <div class="banner" role="alert">
      <span>{view.error}</span>
      <button class="link" onclick={() => onIntent({ type: "MessageDismissed" })} aria-label="Dismiss">×</button>
    </div>
  {/if}

  <main class:with-tabs={view.screens.length > 0}>
    {#if view.screen.kind === "tasks" && view.workspace}
      {@const open = view.workspace}
      <TaskListMobile
        view={view.screen.view}
        onIntent={(intent) => onIntent({ type: "Tasks", workspace: open, intent })}
      />
    {:else if view.screen.kind === "members" && view.workspace}
      {@const open = view.workspace}
      <MembersMobile
        view={view.screen.view}
        onIntent={(intent) => onIntent({ type: "Members", workspace: open, intent })}
      />
    {:else}
      <p class="loading">Loading…</p>
    {/if}
  </main>

  {#if view.screens.length}
    <nav class="tabs" aria-label="Sections">
      {#each view.screens as screen (screen.screen)}
        <button
          class:selected={screen.selected}
          aria-current={screen.selected ? "page" : undefined}
          onclick={() => go({ type: "ScreenSelected", screen: screen.screen })}
        >
          {screen.label}
        </button>
      {/each}
    </nav>
  {/if}

  {#if drawerOpen}
    <button class="scrim" aria-hidden="true" tabindex="-1" onclick={() => (drawerOpen = false)}></button>
    <aside class="drawer" aria-label="Workspaces and account">
      <div class="drawer-head">
        <h2>Workspaces</h2>
        <button class="close" aria-label="Close workspaces" onclick={() => (drawerOpen = false)}>×</button>
      </div>
      {#each view.workspaces as w (w.id)}
        <button
          class="item"
          class:selected={w.selected}
          aria-current={w.selected ? "page" : undefined}
          onclick={() => go({ type: "WorkspaceSelected", id: w.id })}
        >
          <span class="name">{w.name}</span>
          <span class="badge">{w.roleLabel}</span>
        </button>
      {/each}
      <h2>Sections</h2>
      {#each view.screens as screen (screen.screen)}
        <button
          class="item"
          class:selected={screen.selected}
          onclick={() => go({ type: "ScreenSelected", screen: screen.screen })}
        >
          {screen.label}
        </button>
      {/each}
      <div class="me">
        <span class="email">{view.email}</span>
        <button class="link" onclick={() => go({ type: "SignOutRequested" })}>Sign out</button>
      </div>
    </aside>
  {/if}
</div>

<style>
  .frame {
    min-height: 100vh;
  }
  .topbar {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }
  .menu {
    min-width: 44px;
    min-height: 44px;
    background: none;
    border: 1px solid var(--border);
    color: var(--fg);
  }
  .title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Bottom bars inside the screen (the add bar) sit above the tab bar. */
  main.with-tabs {
    --bottom-inset: 52px;
    padding-bottom: 52px;
  }
  .tabs {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    padding-bottom: env(safe-area-inset-bottom);
    background: var(--bg);
    border-top: 1px solid var(--border);
  }
  .tabs button {
    min-height: 52px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--muted);
  }
  .tabs button.selected {
    color: var(--accent);
    font-weight: 600;
  }
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 2;
    border: none;
    background: rgb(0 0 0 / 0.3);
  }
  .drawer {
    position: fixed;
    inset: 0 auto 0 0;
    z-index: 3;
    width: min(300px, 85vw);
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 16px 12px;
    background: var(--bg);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  .drawer-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .close {
    min-width: 44px;
    min-height: 44px;
    background: none;
    border: none;
    color: var(--fg);
    font-size: 1.4rem;
  }
  h2 {
    margin: 12px 0 4px;
    padding: 0 8px;
    font-size: 0.7rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .item {
    box-sizing: border-box;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 44px;
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
    gap: 8px;
    padding: 12px 8px 0;
    font-size: 0.9rem;
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
  .banner {
    display: flex;
    justify-content: space-between;
    margin: 12px 16px 0;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--error-bg);
    color: var(--error);
  }
  .loading {
    text-align: center;
    padding: 48px 16px;
    color: var(--muted);
  }
</style>
