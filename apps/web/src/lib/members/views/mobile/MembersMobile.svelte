<script lang="ts">
  // Pure view, phone layout: one card per member with a role picker and a
  // remove button for owners, and the add form stacked at the top.
  import type { MemberRole, MembersIntentDto, MembersView } from "../../../core";

  interface Props {
    view: MembersView;
    onIntent: (intent: MembersIntentDto) => Promise<void> | void;
  }

  let { view, onIntent }: Props = $props();

  let email = $state("");
  let role = $state<MemberRole>("editor");

  async function add(event: SubmitEvent) {
    event.preventDefault();
    await onIntent({ type: "AddSubmitted", email, role });
    if (!view.error) email = "";
  }
</script>

<div class="screen">
  {#if view.canManage}
    <form class="add" onsubmit={add}>
      <input type="email" bind:value={email} placeholder="Email to add" aria-label="Member email" required />
      <div class="row">
        <select bind:value={role} aria-label="Role">
          {#each view.roleOptions as option (option.role)}
            <option value={option.role}>{option.label}</option>
          {/each}
        </select>
        <button type="submit">Add member</button>
      </div>
    </form>
  {/if}

  {#if view.error || view.notice}
    <div class="message" class:error={!!view.error} role={view.error ? "alert" : "status"}>
      <span>{view.error ?? view.notice}</span>
      <button class="link" onclick={() => onIntent({ type: "MessageDismissed" })} aria-label="Dismiss">×</button>
    </div>
  {/if}

  {#if view.isLoading && view.rows.length === 0}
    <p class="muted">Loading members…</p>
  {:else}
    <ul>
      {#each view.rows as row (row.email)}
        <li class="card">
          <div class="who">
            <span class="email">{row.email}</span>
            {#if row.isYou}<span class="muted">You</span>{/if}
          </div>
          {#if view.canManage}
            <div class="row">
              <select
                value={row.role}
                aria-label={`Role of ${row.email}`}
                onchange={(e) =>
                  onIntent({ type: "RoleChanged", email: row.email, role: e.currentTarget.value as MemberRole })}
              >
                {#each view.roleOptions as option (option.role)}
                  <option value={option.role}>{option.label}</option>
                {/each}
              </select>
              <button class="danger" onclick={() => onIntent({ type: "RemoveRequested", email: row.email })}>
                Remove
              </button>
            </div>
          {:else}
            <span class="muted">{row.roleLabel}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .screen {
    display: grid;
    gap: 12px;
    padding: 12px 16px 24px;
  }
  .add {
    display: grid;
    gap: 8px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row select {
    flex: 1;
    min-height: 44px;
  }
  input,
  button {
    min-height: 44px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .card {
    display: grid;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  .who {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .email {
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .danger {
    background: none;
    border: 1px solid var(--error);
    color: var(--error);
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
    min-height: auto;
  }
</style>
