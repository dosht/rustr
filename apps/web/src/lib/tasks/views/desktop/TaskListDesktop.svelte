<script lang="ts">
  // Pure view: renders a TaskListView and reports intents. Only UI mechanics
  // (focus, the add draft, the keyboard cursor) are local state.
  // Desktop: a table with vim-style keys, like the TUI.
  import type { TaskIntent, TaskListView } from "../../../core";

  interface Props {
    view: TaskListView;
    onIntent: (intent: TaskIntent) => Promise<void> | void;
    /** The page heading; the signed-in frame passes the workspace name. */
    heading?: string;
  }

  let { view, onIntent, heading = "taskr" }: Props = $props();

  let draft = $state("");
  let cursor = $state(0);
  let addInput: HTMLInputElement | undefined = $state();
  let filterInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (cursor > view.rows.length - 1) cursor = Math.max(0, view.rows.length - 1);
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    await onIntent({ type: "AddSubmitted", title: draft });
    if (!view.error) draft = "";
  }

  function isTyping(target: EventTarget | null): boolean {
    return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      onIntent({ type: "FilterChanged", text: "" });
      onIntent({ type: "MessageDismissed" });
      (event.target as HTMLElement | null)?.blur?.();
      return;
    }
    if (isTyping(event.target) || event.ctrlKey || event.metaKey || event.altKey) return;
    const row = view.rows[cursor];
    const keys: Record<string, () => void> = {
      j: () => (cursor = Math.min(cursor + 1, view.rows.length - 1)),
      k: () => (cursor = Math.max(cursor - 1, 0)),
      g: () => (cursor = 0),
      G: () => (cursor = view.rows.length - 1),
      x: () => row && onIntent({ type: "ToggleRequested", id: row.id }),
      a: () => addInput?.focus(),
      "/": () => filterInput?.focus(),
      "1": () => onIntent({ type: "ScopeChanged", scope: "open" }),
      "2": () => onIntent({ type: "ScopeChanged", scope: "done" }),
      "3": () => onIntent({ type: "ScopeChanged", scope: "all" }),
    };
    const action = keys[event.key];
    if (action) {
      event.preventDefault();
      action();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<main>
  <header>
    <h1>{heading}</h1>
    <div class="tabs" role="tablist" aria-label="Task scope">
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
  </header>

  {#if view.canChange}
    <form class="add" onsubmit={submit}>
      <input
        bind:this={addInput}
        bind:value={draft}
        placeholder="Add a task (a)"
        aria-label="New task title"
      />
      <button type="submit">Add</button>
    </form>
  {:else}
    <p class="read-only">You can view this list. Ask an owner for editor access to change it.</p>
  {/if}

  <input
    bind:this={filterInput}
    class="filter"
    type="search"
    value={view.filter}
    oninput={(e) => onIntent({ type: "FilterChanged", text: e.currentTarget.value })}
    placeholder="Filter (/)"
    aria-label="Filter tasks"
  />

  {#if view.error || view.notice}
    <div class="message" class:error={!!view.error} role={view.error ? "alert" : "status"}>
      <span>{view.error ?? view.notice}</span>
      <button class="link" onclick={() => onIntent({ type: "MessageDismissed" })} aria-label="Dismiss">
        ×
      </button>
    </div>
  {/if}

  {#if view.emptyMessage}
    <p class="empty">{view.emptyMessage}</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th aria-label="Done"></th>
          {#each view.columns as column (column.title)}
            <th class={column.align}>{column.title}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each view.rows as row, i (row.id)}
          <tr class:done={row.done} class:cursor={i === cursor} onclick={() => (cursor = i)}>
            <td>
              <input
                type="checkbox"
                checked={row.done}
                disabled={!view.canChange}
                onchange={() => onIntent({ type: "ToggleRequested", id: row.id })}
                aria-label={`Mark "${row.title}" ${row.done ? "open" : "done"}`}
              />
            </td>
            {#each row.cells as cell, c (c)}
              <td class={view.columns[c].align}>{cell}</td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  <footer>
    <kbd>j</kbd>/<kbd>k</kbd> move ·
    {#if view.canChange}<kbd>x</kbd> toggle · <kbd>a</kbd> add ·{/if}
    <kbd>/</kbd> filter · <kbd>1</kbd><kbd>2</kbd><kbd>3</kbd> tabs · <kbd>esc</kbd> clear
  </footer>
</main>

<style>
  main {
    max-width: 720px;
    margin: 0 auto;
    padding: 24px 16px;
    display: grid;
    gap: 12px;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  h1 {
    margin: 0;
    font-size: 1.4rem;
    color: var(--accent);
  }
  .tabs {
    display: flex;
    gap: 4px;
  }
  .tabs button {
    background: none;
    border: 1px solid var(--border);
    color: var(--muted);
  }
  .tabs button.selected {
    color: var(--fg);
    border-color: var(--accent);
  }
  .add {
    display: flex;
    gap: 8px;
  }
  .add input {
    flex: 1;
    min-width: 0;
  }
  .read-only {
    margin: 0;
    padding: 8px 12px;
    border: 1px dashed var(--border);
    border-radius: 6px;
    color: var(--muted);
  }
  .message {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-radius: 6px;
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
  .empty {
    color: var(--muted);
    text-align: center;
    padding: 32px 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.05em;
    text-align: left;
  }
  th,
  td {
    padding: 8px 6px;
    border-bottom: 1px solid var(--border);
  }
  .right {
    width: 1%;
    text-align: right;
    white-space: nowrap;
  }
  tr.cursor {
    background: var(--cursor-bg);
  }
  tr.done td:not(:first-child) {
    color: var(--muted);
    text-decoration: line-through;
  }
  footer {
    color: var(--muted);
    font-size: 0.8rem;
  }
</style>
