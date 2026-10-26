<script lang="ts">
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import type { PlayerSnapshot } from "$lib/platform/media/player";
  import { clockTime } from "$lib/platform/media/player";
  import { i18n } from "$lib/i18n/index.svelte";
  import IconButton from "$lib/ui/IconButton.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Select from "$lib/ui/Select.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  interface Props { state: PlayerSnapshot; title: string; disabled?: boolean; phone?: boolean; expandedEnabled?: boolean; rate?: number; ontoggle: () => void; onseek: (seconds: number) => void; onskip: (seconds: number) => void; onrate: (rate: number) => void; }
  let { state: playback, title, disabled = false, phone = false, expandedEnabled = false, rate = 1, ontoggle, onseek, onskip, onrate }: Props = $props();
  let expanded = $state(false), dragY: number | null = $state(null);
  const speeds = [0.5, 0.75, 1, 1.25, 1.5, 2].map((rate) => ({ value: String(rate), label: `${rate}×` }));
</script>
{#if !expanded || !phone}{@render controls(false)}{/if}
{#if phone && expandedEnabled}<Button variant="ghost" disabled={disabled} onclick={() => (expanded = true)}>{i18n.tk("app", "Expand player")}</Button>{/if}
<Dialog open={expanded && phone} kind="sheet" title={title} onclose={() => (expanded = false)}>
  {#if expanded}
    <button type="button" class="mb-3 flex min-h-11 w-full touch-none justify-center rounded-lg" aria-label={i18n.tk("app", "Close player")} onclick={() => (expanded = false)}
      onpointerdown={(event) => { dragY = event.clientY; event.currentTarget.setPointerCapture(event.pointerId); }}
      onpointerup={(event) => { if (dragY !== null && event.clientY - dragY > 80) expanded = false; dragY = null; }} onpointercancel={() => (dragY = null)}><span class="mt-2 h-1 w-10 rounded-full bg-border"></span></button>
    {@render controls(true)}
  {/if}
</Dialog>
{#snippet controls(full: boolean)}
  <div class="space-y-3">
    <div class="flex items-center gap-3">
      {#if full}<IconButton icon={SkipBack} label={i18n.tk("app", "p5.player.skip_back")} {disabled} onclick={() => onskip(-10)} />{/if}
      <IconButton data-testid="player-toggle" icon={playback.playing ? Pause : Play} label={i18n.tk("app", playback.playing ? "p5.player.pause" : "p5.player.play")} {disabled} onclick={ontoggle} />
      {#if full}<IconButton icon={SkipForward} label={i18n.tk("app", "p5.player.skip_forward")} {disabled} onclick={() => onskip(10)} />{/if}
      <div class="min-w-0 flex-1">
        <span data-testid="player-time" class="block text-xs tabular-nums text-muted">{clockTime(playback.time)} / {clockTime(playback.duration ?? 0)}</span>
        <input type="range" min="0" max={playback.duration ?? 0} step="0.1" value={playback.time} disabled={disabled || playback.duration === null} aria-label={i18n.tk("app", "Show Time")} class="min-h-11 w-full accent-accent" oninput={(event) => onseek(Number(event.currentTarget.value))} />
      </div>
    </div>
    {#if full}<Select label={i18n.tk("app", "p5.player.speed")} options={speeds} value={String(rate)} {disabled} onchange={(value) => onrate(Number(value))} />{/if}
  </div>
{/snippet}
