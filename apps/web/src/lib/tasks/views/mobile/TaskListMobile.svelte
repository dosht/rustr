<script lang="ts">
  // Pure view, phone layout: segmented scopes, tappable cards and an add bar
  // fixed to the bottom, within thumb reach. Same TaskListView and intents as
  // the desktop table; only the add draft is local.
  import type { TaskIntent, TaskListView } from "../../../core";

  interface Props {
    view: TaskListView;
    onIntent: (intent: TaskIntent) => Promise<void> | void;
    /** Shown when the view stands alone; the signed-in frame has its own title bar. */
    heading?: string;
  }

  let { view, onIntent, heading }: Props = $props();

  let draft = $state("");
  const ageColumn = $derived(view.columns.findIndex((c) => c.title === "AGE"));

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    await onIntent({ type: "AddSubmitted", title: draft });
    if (!view.error) draft = "";
  }
</script>

<div class="screen">
  {#if heading}<h1>{heading}</h1>{/if}

  <div class="segments" role="tablist" aria-label="Task scope">
    {#each view.tabs as tab (tab.scope)}
      <button
        role="tab"
        aria-selected={tab.selected}
        class:selected={tab.selected}
        onclick={() => onIntent({ type: "ScopeChanged", scope: tab.scope })}
      >
        {tab.label}
      </button>
    {/each}
  </div>

  <input
    class="filter"
    type="search"
    value={view.filter}
    oninput={(e) => onIntent({ type: "FilterChanged", text: e.currentTarget.value })}
    placeholder="Filter"
    aria-label="Filter tasks"
  />

  {#if view.error || view.notice}
    <div class="message" class:error={!!view.error} role={view.error ? "alert" : "status"}>
      <span>{view.error ?? view.notice}</span>
      <button class="link" onclick={() => onIntent({ type: "MessageDismissed" })} aria-label="Dismiss">×</button>
    </div>
  {/if}

  {#if view.emptyMessage}
    <p class="empty">{view.emptyMessage}</p>
  {:else}
    <ul class="cards">
      {#each view.rows as row (row.id)}
        <li>
          <button
            class="card"
            class:done={row.done}
            disabled={!view.canChange}
            aria-pressed={row.done}
            aria-label={`${row.title}, ${row.done ? "done" : "open"}`}
            onclick={() => onIntent({ type: "ToggleRequested", id: row.id })}
          >
            <span class="check" aria-hidden="true">{row.done ? "✓" : ""}</span>
            <span class="title">{row.title}</span>
            {#if ageColumn >= 0}<span class="age">{row.cells[ageColumn]}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if view.canChange}
    <form class="add" onsubmit={submit}>
      <input bind:value={draft} placeholder="Add a task" aria-label="New task title" enterkeyhint="done" />
      <button type="submit">Add</button>
    </form>
  {:else}
    <p class="read-only">You can view this list. Ask an owner for editor access to change it.</p>
  {/if}
</div>

<style>
  .screen {
    display: grid;
    gap: 12px;
    padding: 12px 16px calc(80px + var(--bottom-inset, 0px));
  }
  h1 {
    margin: 0;
    font-size: 1.3rem;
    color: var(--accent);
  }
  .segments {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .segments button {
    border: none;
    border-radius: 0;
    background: none;
    color: var(--muted);
    min-height: 40px;
  }
  .segments button.selected {
    background: var(--cursor-bg);
    color: var(--fg);
    font-weight: 600;
  }
  .filter {
    min-height: 40px;
  }
  .cards {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .card {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 52px;
    text-align: left;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--fg);
  }
  .card:disabled {
    opacity: 1;
    cursor: default;
  }
  .check {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 2px solid var(--border);
    border-radius: 50%;
    color: var(--bg);
  }
  .card.done .check {
    background: var(--accent);
    border-color: var(--accent);
  }
  .card.done .title {
    color: var(--muted);
    text-decoration: line-through;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .age,
  .empty,
  .read-only {
    color: var(--muted);
  }
  .empty {
    text-align: center;
    padding: 32px 0;
  }
  .add {
    position: fixed;
    left: 0;
    right: 0;
    bottom: var(--bottom-inset, 0px);
    display: flex;
    gap: 8px;
    padding: 12px 16px;
    background: var(--bg);
    border-top: 1px solid var(--border);
  }
  .add input {
    flex: 1;
    min-width: 0;
    min-height: 44px;
  }
  .add button {
    min-height: 44px;
  }
  .message {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--notice-bg);
  }
  .message.error {
    background: var(--error-bg);
    color: var(--error);
  }
  .link {
    background: none;
    border: none;
    color: inherit;
    font-size: 1.2rem;
  }
</style>
