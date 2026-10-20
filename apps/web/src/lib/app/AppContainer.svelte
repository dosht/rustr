<script lang="ts">
  // Container: picks the view for the current state and the screen size, and
  // wires intents to the model. No product logic; that is in Rust.
  import type { AppModel } from "./appModel.svelte";
  import DesktopFrame from "./views/desktop/DesktopFrame.svelte";
  import MobileFrame from "./views/mobile/MobileFrame.svelte";
  import SignInView from "./views/SignInView.svelte";

  interface Props {
    model: AppModel;
    mobile: boolean;
    devSignIn: boolean;
  }

  let { model, mobile, devSignIn }: Props = $props();

  const view = $derived(model.view);

  $effect(() => {
    document.title = view.state === "signedIn" ? `${view.view.title} · taskr` : "taskr";
  });
</script>

{#if view.state === "starting"}
  <p class="boot">Loading…</p>
{:else if view.state === "signedOut"}
  <SignInView view={view.view} onIntent={model.send} {devSignIn} />
{:else if mobile}
  <MobileFrame view={view.view} onIntent={model.send} />
{:else}
  <DesktopFrame view={view.view} onIntent={model.send} />
{/if}

<style>
  .boot {
    text-align: center;
    padding: 48px 16px;
    color: var(--muted);
  }
</style>
