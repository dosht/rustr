<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import Button from "$lib/ui/Button.svelte";
  interface Props { touch: boolean; videoId: string; player: Snippet; controls: Snippet; }
  let { touch, videoId, player, controls }: Props = $props();
  type Mode = "video" | "strip" | "float";
  type Corner = "top-left" | "top-right" | "bottom-left" | "bottom-right";
  let desktopMode: Mode = $state("video"), mobileMode: "video" | "strip" = $state("video");
  let desktopHeight = $state(220), mobileHeight = $state(180), corner: Corner = $state("bottom-right"), restored = $state(false);
  let drag: { y: number; height: number } | null = $state(null);
  const mode = $derived.by((): Mode => touch ? mobileMode : desktopMode);
  const height = $derived(touch ? mobileHeight : desktopHeight);
  const modes: readonly Mode[] = ["video", "strip", "float"];
  const touchModes: readonly Mode[] = ["video", "strip"];
  const corners: Corner[] = ["top-left", "top-right", "bottom-left", "bottom-right"];
  function stored(key: string, fallback: string) { try { return localStorage.getItem(key) ?? fallback; } catch { return fallback; } }
  function persist(key: string, value: string | number) { try { localStorage.setItem(key, String(value)); } catch { /* Storage is optional for layout preferences. */ } }
  function setMode(value: Mode) { if (touch) mobileMode = value === "float" ? "video" : value; else desktopMode = value; }
  function resize(next: number) {
    if (touch) { mobileMode = next < 80 ? "strip" : "video"; mobileHeight = Math.max(96, Math.min(240, next)); }
    else desktopHeight = Math.max(120, Math.min(520, next));
  }
  onMount(() => {
    const desktop = stored("yt-layout-mode", "video"), mobile = stored("yt-layout-mobile-mode", "video"), position = stored("yt-layout-corner", "bottom-right");
    if (desktop === "video" || desktop === "strip" || desktop === "float") desktopMode = desktop;
    if (mobile === "video" || mobile === "strip") mobileMode = mobile;
    const restoredCorner = corners.find((value) => value === position);
    if (restoredCorner) corner = restoredCorner;
    const dh = Number(stored("yt-layout-height", "220")), mh = Number(stored("yt-layout-mobile-height", "180"));
    desktopHeight = Number.isFinite(dh) ? Math.max(120, Math.min(520, dh)) : 220;
    mobileHeight = Number.isFinite(mh) ? Math.max(96, Math.min(240, mh)) : 180;
    restored = true;
  });
  $effect(() => { if (!restored) return; persist("yt-layout-mode", desktopMode); persist("yt-layout-mobile-mode", mobileMode); persist("yt-layout-height", desktopHeight); persist("yt-layout-mobile-height", mobileHeight); persist("yt-layout-corner", corner); });
</script>
<div class="relative" data-testid="youtube-layout" data-mode={mode}>
  <div role="group" aria-label={i18n.tk("app", "Player mode")} class="flex gap-1">
    {#each (touch ? touchModes : modes) as value}
      <Button size="sm" variant={mode === value ? "primary" : "ghost"} aria-pressed={mode === value} onclick={() => setMode(value)}>{i18n.tk("app", `p5.youtube.mode_${value}`)}</Button>
    {/each}
  </div>
  <div style:height={mode === "video" ? `${height}px` : "0px"}>
    <div class="video-stage" class:strip={mode === "strip"} class:floating={mode === "float"}
      style:height={mode === "float" || mode === "strip" ? "180px" : `${height}px`}
      style:top={mode === "float" && corner.startsWith("top") ? "16px" : undefined}
      style:bottom={mode === "float" && corner.startsWith("bottom") ? "16px" : undefined}
      style:left={mode === "float" && corner.endsWith("left") ? "16px" : undefined}
      style:right={mode === "float" && corner.endsWith("right") ? "16px" : undefined}
      inert={mode === "strip"} aria-hidden={mode === "strip" ? "true" : undefined}>
      {@render player()}
      {#if mode === "float"}<div class="absolute right-1 top-1 grid grid-cols-2 bg-surface">{#each corners as value}<button type="button" class="min-h-11 min-w-11" aria-label={i18n.tk("app", `Move to ${value}`)} aria-pressed={corner === value} onclick={() => (corner = value)}>{value.startsWith("top") ? value.endsWith("left") ? "↖" : "↗" : value.endsWith("left") ? "↙" : "↘"}</button>{/each}</div>{/if}
    </div>
  </div>
  {#if mode === "video" || touch}
    <div role="slider" tabindex="0" aria-label={i18n.tk("app", "Resize video player (drag up or down)")} aria-orientation="vertical" aria-valuemin={touch ? 0 : 120} aria-valuemax={touch ? 240 : 520} aria-valuenow={mode === "strip" ? 0 : height}
      class="flex h-5 touch-none cursor-ns-resize items-center justify-center"
      onkeydown={(event) => {
        if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault();
          resize(mode === "strip" ? event.key === "ArrowUp" ? 96 : 0 : height + (event.key === "ArrowUp" ? 20 : -20));
        } else if (event.key === "Home" || event.key === "End") {
          event.preventDefault();
          resize(event.key === "Home" ? touch ? 0 : 120 : touch ? 240 : 520);
        }
      }}
      onpointerdown={(event) => { drag = { y: event.clientY, height: mode === "strip" ? 0 : height }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onpointermove={(event) => { if (drag) resize(drag.height + (touch ? drag.y - event.clientY : event.clientY - drag.y)); }}
      onpointerup={() => (drag = null)} onpointercancel={() => (drag = null)}><span class="h-1 w-12 rounded-full bg-border"></span></div>
  {/if}
  {#if mode === "strip"}<div class="flex items-center gap-3"><img src={`https://img.youtube.com/vi/${videoId}/mqdefault.jpg`} alt="" class="h-10 w-16 rounded object-cover" /><div class="flex-1">{@render controls()}</div></div>{/if}
</div>
<style>
  .video-stage { position: relative; display: flex; justify-content: center; width: 100%; }
  .video-stage :global(iframe), .video-stage :global([data-testid="youtube-player"]) { height: 100%; max-width: 100%; aspect-ratio: 16 / 9; }
  .video-stage.strip { position: fixed; left: -10000px; top: 0; width: 320px; pointer-events: none; }
  .video-stage.floating { position: fixed; z-index: 40; width: 320px; max-width: calc(100vw - 32px); border: 1px solid var(--border); border-radius: 8px; }
</style>
