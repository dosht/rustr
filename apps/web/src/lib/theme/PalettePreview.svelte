<script lang="ts">
  import type { AppIntentDto } from "../core";
  import { signedIn, signIn } from "../core/fixtures";
  import DesktopFrame from "../app/views/desktop/DesktopFrame.svelte";
  import MobileFrame from "../app/views/mobile/MobileFrame.svelte";
  import SignInView from "../app/views/SignInView.svelte";
  import { palettes, defaultPalette, modes } from "./palettes";

  interface Props {
    palette?: (typeof palettes)[number]["id"];
    mode?: (typeof modes)[number];
    platform?: "desktop" | "phone";
    screen?: "tasks" | "sign-in" | "members";
    showError?: boolean;
    readOnly?: boolean;
    onIntent: (intent: AppIntentDto) => Promise<void> | void;
  }

  let {
    palette = defaultPalette,
    mode = "light",
    platform = "desktop",
    screen = "tasks",
    showError = false,
    readOnly = false,
    onIntent,
  }: Props = $props();

  const name = $derived(palettes.find((item) => item.id === palette)?.name ?? palette);
  const fixture = $derived(`${readOnly ? "viewer" : "owner"}-${screen === "members" ? "members" : "tasks"}`);
</script>

<div class="experiment" data-palette={palette} data-theme={mode}>
  <aside class="caption" aria-label="Palette experiment">
    <h1>{name} · {mode} · {platform} · {screen}</h1>
    <p>E3 palette candidate. Existing Taskr core fixtures, not Transgate screens. Intents are logged, not persisted.</p>
    <p>Compare the named stories or use palette/mode controls. Paper Moss remains the default until mu selects.</p>
  </aside>
  <!-- One live view per story keeps window shortcuts and fixed phone bars isolated. -->
  <div class="surface" class:phone={platform === "phone"}>
    {#if screen === "sign-in"}
      <SignInView view={signIn(showError ? "sign-in-error" : "sign-in")} devSignIn={false} {onIntent} />
    {:else if platform === "phone"}
      <MobileFrame view={signedIn(fixture)} {onIntent} />
    {:else}
      <DesktopFrame view={signedIn(fixture)} {onIntent} />
    {/if}
  </div>
</div>

<style>
  .experiment {
    box-sizing: border-box;
    min-height: 100vh;
    padding: 16px;
    background: var(--surface);
    color: var(--fg);
  }
  .caption {
    max-width: 960px;
    margin: 0 auto 16px;
  }
  .caption h1 {
    margin: 0;
    font: inherit;
    font-weight: 700;
  }
  .caption p {
    margin: 4px 0;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .surface {
    margin: 0 auto;
    max-width: 1100px;
    height: 800px;
    overflow: auto;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--fg);
    isolation: isolate;
    /* A containing block for the real mobile view's fixed add/tab bars. */
    transform: translateZ(0);
  }
  .surface.phone {
    max-width: 390px;
    height: 844px;
  }
  .surface :global(.frame) {
    min-height: 100%;
  }
</style>
