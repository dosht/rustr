<script lang="ts">
  // Pure view: renders a MembersView and reports intents. Whether the add
  // form, role pickers and remove buttons exist comes from `canManage`.
  import type { MemberRole, MembersIntentDto, MembersView } from "../../../core";

  interface Props {
    view: MembersView;
    onIntent: (intent: MembersIntentDto) => Promise<void> | void;
    heading: string;
  }

  let { view, onIntent, heading }: Props = $props();

  let email = $state("");
  let role = $state<MemberRole>("editor");

  async function add(event: SubmitEvent) {
    event.preventDefault();
    await onIntent({ type: "AddSubmitted", email, role });
    if (!view.error) email = "";
  }
</script>

<main>
  <header>
    <h1>{heading}</h1>
  </header>

  {#if view.canManage}
    <form class="add" onsubmit={add}>
      <input type="email" bind:value={email} placeholder="Email to add" aria-label="Member email" required />
      <select bind:value={role} aria-label="Role">
        {#each view.roleOptions as option (option.role)}
          <option value={option.role}>{option.label}</option>
        {/each}
      </select>
      <button type="submit">Add</button>
    </form>
  {/if}

  {#if view.error || view.notice}
    <div class="message" class:error={!!view.error} role={view.error ? "alert" : "status"}>
      <span>{view.error ?? view.notice}</span>
      <button class="link" onclick={() => onIntent({ type: "MessageDismissed" })} aria-label="Dismiss">×</button>
    </div>
  {/if}

  {#if view.isLoading && view.rows.length === 0}
    <p class="empty">Loading members…</p>
  {:else}
    <ul>
      {#each view.rows as row (row.email)}
        <li>
          <span class="email">{row.email}{#if row.isYou}<span class="you">(you)</span>{/if}</span>
          {#if view.canManage}
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
            <button class="link" onclick={() => onIntent({ type: "RemoveRequested", email: row.email })}>
              Remove
            </button>
          {:else}
            <span class="role">{row.roleLabel}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  main {
    max-width: 720px;
    margin: 0 auto;
    padding: 24px 16px;
    display: grid;
    gap: 12px;
  }
  h1 {
    margin: 0;
    font-size: 1.4rem;
    color: var(--accent);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .add input {
    flex: 1;
    min-width: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 6px;
    border-bottom: 1px solid var(--border);
  }
  .email {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .you {
    margin-left: 0.4em;
  }
  .you,
  .role,
  .empty {
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
  }
</style>
