<script lang="ts">
  import { untrack } from "svelte";
  import type { TranscriptVm } from "$lib/core/types/TranscriptVm";
  import type { MediaPlatform } from "$lib/platform/media";
  import { decorativePeaks, mountWaveform, loadYouTubeIframeApi, mountYouTube } from "$lib/platform/media";
  import type { MountedPlayer, PlayerSnapshot } from "$lib/platform/media/player";
  import type { WaveformPlayer } from "$lib/platform/media/waveform-player";
  import { i18n } from "$lib/i18n/index.svelte";
  import PlayerControls from "../views/shared/PlayerControls.svelte";
  import YouTubeSurface from "../views/shared/YouTubeSurface.svelte";
  interface Props { vm: TranscriptVm; media: MediaPlatform; phone: boolean; touch: boolean; expandedEnabled: boolean; onstate: (snapshot: PlayerSnapshot) => void; }
  let { vm, media, phone, touch, expandedEnabled, onstate }: Props = $props();
  let waveformHost: HTMLDivElement | undefined = $state(), youtubeHost: HTMLDivElement | undefined = $state();
  let mounted: MountedPlayer | null = $state.raw(null), waveform: WaveformPlayer | null = $state.raw(null);
  let playback: PlayerSnapshot = $state({ playing: false, time: 0, duration: null });
  let rate = $state(1), loadError = $state(false);
  const source = $derived(vm.media.kind === "audio" ? vm.media.url : vm.media.kind === "you_tube" ? vm.media.video_id : null);
  const kind = $derived(vm.media.kind), job = $derived(vm.job ?? 0), duration = $derived((vm.duration_minutes ?? 0) * 60);
  const peaks = $derived(kind === "audio" ? decorativePeaks(job, Math.max(duration, 20)) : []);
  function seek(seconds: number) { if (mounted) mounted.seek(Math.max(0, Math.min(mounted.duration() ?? seconds, seconds))); }
  function skip(seconds: number) { if (!mounted) return; seek(mounted.currentTime() + seconds); navigator.vibrate?.(10); }
  function toggle() { if (!mounted) return; if (mounted.isPlaying()) mounted.pause(); else void mounted.play(); }
  function changeRate(next: number) { waveform?.setPlaybackRate(next); rate = next; }
  function connect(player: MountedPlayer) {
    mounted = player;
    const detach = media.players.attach(player);
    playback = player.snapshot();
    untrack(() => onstate(playback));
    const unsubscribe = player.subscribe((next) => { playback = next; onstate(next); });
    return () => { unsubscribe(); detach(); if (untrack(() => mounted) === player) mounted = null; };
  }
  $effect(() => {
    const url = source, type = kind, wave = waveformHost, video = youtubeHost;
    const seconds = type === "audio" ? duration : 0, drawnPeaks = peaks;
    loadError = false;
    playback = { playing: false, time: 0, duration: null };
    if (!url) return;
    const abort = new AbortController();
    let disconnect: (() => void) | undefined, dispose: (() => void) | undefined;
    if (type === "audio" && wave) {
      const colors = getComputedStyle(wave);
      const result = mountWaveform({ container: wave, url, peaks: drawnPeaks, duration: seconds, waveColor: colors.getPropertyValue("--border").trim(), progressColor: colors.getPropertyValue("--accent").trim() }, () => (loadError = true));
      waveform = result.player;
      result.player.setPlaybackRate(untrack(() => rate));
      disconnect = untrack(() => connect(result.player));
      dispose = result.dispose;
    } else if (type === "you_tube" && video) {
      // The API replaces its child host; the Svelte-owned wrapper remains stable.
      const target = document.createElement("div");
      video.appendChild(target);
      void loadYouTubeIframeApi().then(async (api) => {
        if (abort.signal.aborted) return;
        const result = await mountYouTube(target, url, api, undefined, abort.signal);
        if (abort.signal.aborted) { result.dispose(); return; }
        disconnect = connect(result.player);
        dispose = result.dispose;
      }).catch(() => { if (!abort.signal.aborted) loadError = true; });
    }
    return () => { abort.abort(); disconnect?.(); dispose?.(); waveform = null; if (video) video.replaceChildren(); };
  });
  $effect(() => {
    const player = mounted;
    const title = vm.title || i18n.tk("app", "Untitled");
    if (!phone || !player || !("mediaSession" in navigator)) return;
    const session = navigator.mediaSession;
    if (typeof MediaMetadata !== "undefined") session.metadata = new MediaMetadata({ title, artist: "Transgate AI" });
    const registered: MediaSessionAction[] = [];
    const handlers: [MediaSessionAction, () => void][] = [
      ["play", () => { void player.play(); }], ["pause", () => player.pause()],
      ["seekbackward", () => player.seek(Math.max(0, player.currentTime() - 10))],
      ["seekforward", () => player.seek(Math.min(player.duration() ?? Infinity, player.currentTime() + 10))],
    ];
    for (const [action, handler] of handlers) {
      try { session.setActionHandler(action, handler); registered.push(action); } catch { /* Some browsers expose only a subset of Media Session actions. */ }
    }
    return () => { for (const action of registered) session.setActionHandler(action, null); session.metadata = null; };
  });
</script>
{#if vm.media.kind === "you_tube"}
  <YouTubeSurface {touch} videoId={vm.media.video_id} player={video} controls={controls} />
{:else if vm.media.kind === "audio"}
  <div class="space-y-2">
    {#if vm.media.loading}<p role="status" class="text-sm text-muted">{i18n.tk("common", "Loading...")}</p>{/if}
    {#if vm.media.unavailable}<p class="text-sm text-muted">{i18n.tk("app", "Audio is not available.")}</p>{/if}
    <div bind:this={waveformHost} data-testid="player-waveform" class="min-h-10"></div>
    {@render controls()}
  </div>
{/if}
{#if loadError}<p role="alert" class="text-sm text-error">{i18n.tk("app", "Audio is not available.")}</p>{/if}
{#snippet video()}<div bind:this={youtubeHost} data-testid="youtube-player"></div>{/snippet}
{#snippet controls()}
  <PlayerControls state={playback} title={vm.title} {phone} expandedEnabled={expandedEnabled && kind === "audio"} {rate} disabled={!mounted || loadError} ontoggle={toggle} onseek={seek} onskip={skip} onrate={changeRate} />
{/snippet}
