<script lang="ts">
  // Pure view: the sign-in form. Only the typed email and password are local.
  import type { AppIntentDto, SignInView } from "../../core";

  interface Props {
    view: SignInView;
    onIntent: (intent: AppIntentDto) => Promise<void> | void;
    /** Dev sign-in accepts any email and ignores the password. */
    devSignIn: boolean;
  }

  let { view, onIntent, devSignIn }: Props = $props();

  let email = $state("");
  let password = $state("");

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const signUp = (event.submitter as HTMLButtonElement | null)?.value === "sign-up";
    onIntent({ type: signUp ? "SignUpSubmitted" : "SignInSubmitted", email, password });
  }
</script>

<main>
  <h1>taskr</h1>
  <form onsubmit={submit} aria-busy={view.busy}>
    <label>
      Email
      <input type="email" autocomplete="email" bind:value={email} required />
    </label>
    {#if !devSignIn}
      <label>
        Password
        <input type="password" autocomplete="current-password" bind:value={password} required />
      </label>
    {/if}
    {#if view.error || view.notice}
      <p class="message" class:error={!!view.error} role={view.error ? "alert" : "status"}>
        {view.error ?? view.notice}
      </p>
    {/if}
    <div class="actions">
      <button type="submit" value="sign-in" disabled={view.busy}>Sign in</button>
      {#if !devSignIn}
        <button type="submit" value="sign-up" class="secondary" disabled={view.busy}>Create account</button>
      {/if}
    </div>
    {#if devSignIn}
      <p class="hint">Dev sign-in: any email works, no password. The server must run with TASKR_DEV_AUTH=1.</p>
    {/if}
  </form>
</main>

<style>
  main {
    max-width: 360px;
    margin: 0 auto;
    padding: 64px 16px;
    display: grid;
    gap: 16px;
  }
  h1 {
    margin: 0;
    color: var(--accent);
  }
  form {
    display: grid;
    gap: 12px;
  }
  label {
    display: grid;
    gap: 4px;
    color: var(--muted);
    font-size: 0.9rem;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .secondary {
    background: none;
    border: 1px solid var(--border);
    color: var(--fg);
  }
  .message {
    margin: 0;
    padding: 8px 12px;
    border-radius: 6px;
    background: var(--notice-bg);
  }
  .message.error {
    background: var(--error-bg);
    color: var(--error);
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
</style>
