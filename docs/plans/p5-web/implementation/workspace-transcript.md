# P5 workspace, upload, history, transcript and player: working handoff

Owner: p1Z, same saved P3/P4/P5 session. Original Opus source/evidence and P5 ownership are preserved; user authorized continuation on Astra high on 2026-10-26 after local no-LLM snapshot compaction. No transfer or rewrite. Slice from `../implementation-contract.md`: desktop and phone workspace, upload, history, transcript and player views; stories and component regressions; browser file/media adapters. **Status: owed source implementation and fixture stories/component regressions written against Y's released run5 contracts; unverified.** No generated/Rust writes and no builds, tests, lint, formatters, browser runs or provider calls by this owner. This continuation is later than the separately copied S1 snapshot.

## Rows

Applies the D9 corrections throughout.

| Rows | What | Main source |
|---|---|---|
| WEB-007, WEB-008 | Upload and start | `TranscriptionContainer.tsx:232-323`; `TranscriptionWorkspace.tsx:183-278`; `MobileTranscriptionContainer.tsx:65-272`; `MobileNewTranscript.tsx:134-319` |
| WEB-009, WEB-010 | Drafts restored; preparation, balance, language, timeout and YouTube alerts | same, plus `useTranscriptionViewModel.ts:378-520` |
| WEB-011, WEB-012 | YouTube dialog, add and resolve | `TranscriptionContainer.tsx:140-190,240-271`; `MobileNewTranscript.tsx:159-167,209-211` |
| WEB-013, WEB-014 | Translation mode, source/target languages, speakers | mode content components and phone `ConfigurationSection` |
| WEB-015, WEB-016 | History | `JobListContainer.tsx:42-176`; `JobListTable.tsx`; `MobileJobListView.tsx`; sidebar list in `pages/home/[[...jobId]].tsx:399-439`; phone drawer list via `MobileNavigationContainer` |
| WEB-017..024 | Transcript, playback, edit, speakers, translation tabs, freemium fade | `components/Transcript/Transcript.tsx`; `TranscriptLinesContainer.tsx`; `MobileJobDetailsContainer.tsx`; `MobileJobDetailsView.tsx`; `SpeakerLabel.tsx` |
| WEB-025, WEB-026 | Export and copy | `transcriptFileSaver.ts`; the bytes are the P3 server/core export bundle |

AI panels (WEB-027..030) belong to p21. App frame, auth, settings, mobile navigation shell and feedback/support modal views belong to p1V. My views render feedback/support triggers, not those dialogs.

## Source composition (`pages/home/[[...jobId]].tsx`)

- **Device branch:** `shouldShowMobileUX = isMobile && MOBILE_UX_JOBS` (`:39-44`). The job list additionally requires `isTouchDevice` (`JobListContainer.tsx:53`). Exactly one branch is mounted (`:478-486`: mounting both duplicated the YouTube iframe).
- **Phone entry:** `/home` without `jobId`/`view` redirects with `replace` to `?view=new` (`:85-90`).
- **Routes:**
  - `?view=new` on phone: mobile navigation plus `MobileTranscriptionContainer` (`:340-369`).
  - `?view=all-transcripts`: the list. Desktop adds a header and wrappers (`:446-475`).
  - `/home/{id}`: `MobileJobDetailsContainer` or `Transcript` (`:476-519`).
  - Otherwise: `TranscriptionContainer` (`:520-537`).
- **Sidebar:** the desktop sidebar collapses (260/360 px or 64 px). It has its own paginated list and load-more (`:72-75,399-439`). The list page uses a separate `useJobListViewModel` (`JobListContainer.tsx:80-87`).
- **Telemetry:** transitions to completed/failed fire telemetry (`:245-331`). Start fires started/failed per file (containers). These are not view logic; asked p1X (item 11).

## Upload: source step order

- **Desktop** (`TranscriptionContainer.tsx:273-323`, `TranscriptionWorkspace.tsx:183-278`):
  1. Welcome header with the profile full name.
  2. YouTube beta banner, if YouTube is enabled.
  3. Mode tabs.
  4. Drop zone. With no files it shows the blank state for the current mode and the plus menu, which is hidden at the max-files limit. With files it shows the file list, then the mode language selectors with the speakers toggle, then the plus menu.
  5. Main Start button below the panel. It is disabled by `isTranscriptButtonDisabled` and shows a spinner while transcribing, otherwise "Start Translating" or "Start Transcribing".
- **Phone** (`MobileNewTranscript.tsx:134-319`):
  1. Alerts: balance, processing timeout, language, YouTube failure. The view scrolls to the top when an alert appears (local mechanic).
  2. YouTube dialog.
  3. Total duration line, only with files: "Calculating..." while any duration is unknown, else "N min".
  4. YouTube banner, at `sm` and above only.
  5. File button. A record button is shown only when the microphone flag is on (the flag is false for P5 per proposals).
  6. YouTube button.
  7. Files heading and file items. A swipe hint auto-hides after 3 s (local mechanic).
  8. Configuration section.
  9. Empty state ("No files added yet") or the loading spinner.
  10. Fixed bottom Start, only with files: "Starting..." while starting, "Start Translating/Transcribing" when ready, "Preparing files..." otherwise.
- **Phone picker validation:** `MobileTranscriptionContainer.tsx:120-157` drops files that fail `validateFile` (`.m4a .mp3 .wav .mp4 .webm .ogg`) or exceed 500 MiB. This contradicts `jobs-transcription.md:220` and core `Event::Picked`. Reported to lead. Asked p1X for a core filter; nothing is encoded in TS.

## History: source notes

- **Desktop table:** initial loading, error and empty states, then the table.
  - Columns: name, status badge (spinner for processing statuses), duration, date, actions.
  - Row actions: download, feedback, delete with confirmation.
  - Infinite scroll uses a sentinel that is disabled while loading or loading more.
- **Row download** exports every line of the job, unfiltered, with time on and speakers as the job has them (`JobListContainer.tsx:95-113`, `transcriptFileSaver.ts:19-56`, ledger `jobs-transcription.md:269`). X has published `DownloadRequested` / `DownloadFinished` and the server `Export` effect; consume the generated contract.
- **Phone list** (per D9): cards, swipe to delete, tap to open, no mounted filter sheet, and no pull-to-refresh on all-transcripts (`JobListContainer.tsx:63-64,153`).

## Browser media adapters: source-ready, unrun

These implement p22's `Platform` `measure`/`put`/`forget`/`player` (`src/lib/core/platform.ts`) in `transgate/web/src/lib/platform/media/`. Entry point: `createMediaPlatform(): MediaPlatform`, which exposes `files: FileStore` and `players: PlayerSlot` for views. p1V spreads it into the single `Platform` object. Sources: `AudioDurationCalculator.ts:30-219`, `FileUploadService.ts:54-99`, `media-player/impls/*`, `YouTubeMount.tsx:38-104`, `AudioPlayer.tsx:65-303`, `audioUtils.ts:11-78`, `useActiveLineIndex.ts:23-41`.

- **`files.ts`:** Files stay in the page under a numeric handle; same-named files get distinct handles. Unknown or forgotten handles never cause I/O.
- **`measure.ts`:** HTML5 metadata with a 5 s timeout (finite and positive), then a Web Audio decode only at 100 MiB or less with a 10 s timeout, else `null`. The object URL is revoked, the context is closed, and late events are ignored. The source's server fallback is not part of core UPL-006.
- **`put.ts`:** one XHR `PUT` of the `File` itself; the source's ArrayBuffer copy was behavior-neutral. Content-Type is `file.type` or `application/octet-stream`; progress is reported only when the total is known. Only status 200 is ok; network error, abort or timeout gives `status: 0`. Never retried.
- **`player.ts`:** a one-player `PlayerSlot` (attaching a player destroys the previous one), change-only `PlayerState`, `activeLineIndex` (the source rule) and `clockTime` (`HH:MM:SS`).
- **`audio-player.ts`:** state only from media events. A refused `play()` leaves state unchanged. The optional engine-load callback reports a technical load failure; URL reuse/refresh and the five-second no-audio alert expiry are core/wasm effects, not TS timers or invented intents.
- **`waveform-player.ts`:** wavesurfer.js `^7.8.4`, imported statically, with the source options: height 40, drag to seek, MediaElement backend, duration at least 20, and `[[0.5]]` when there are no peaks. `decorativePeaks` ports the source's seeded decorative LCG and smoothing. These are not decoded audio; lead approved keeping them in TS (decision below).
- **`youtube-player.ts`:** BUFFERING counts as playing; time is polled every 250 ms only while playing; play, pause and seek are optimistic. The iframe API script loads once per page. The iframe uses `autoplay 0, rel 0, modestbranding 1`.
- **Tests:** `measure.test.ts`, `put.test.ts`, `player.test.ts` (media platform, slot, audio player, active line, clock), `waveform-player.test.ts`, `youtube-player.test.ts`. All run under Vitest/jsdom with injected engines and no real timers. Command, run by lead: `(cd transgate/web && npm run test:ci -- src/lib/platform/media)`.

**Third-party contact (lead decision, preserves source).** Opening a YouTube-sourced job's transcript page automatically loads `https://www.youtube.com/iframe_api` and embeds the iframe. There is no click, no cookie-consent gate and no nocookie origin (`YouTubePlayerLayoutContainer.tsx:51`, `MobileYouTubePlayerLayoutContainer.tsx:48`, `YouTubeMount.tsx:27,97`). D12 consent covers browser telemetry only. The signed storage URLs, for upload PUT and audio streaming, are also direct browser-to-provider contacts. Serving/CSP must allow them; that is recorded with p1V/p1Y. Tests use controlled fixtures only, so real YouTube playback remains a named live limit.

## Decisions received

- **Lead (A), layouts:** source flags are globally on, and there is no dormant flag-off renderer. The three independent layout facts (frame: touch, width below 1024 and the flag; transcript: width below 768; YouTube: touch) are decided in core through `AppIntent::Viewport` and `AppVm` layouts. Containers render exactly that.
- **Lead (B), feedback/support:** p1V owns the modal views; my row and transcript render only triggers that dispatch p1X intents.
- **Lead (C), dependencies:** wavesurfer.js `^7.8.4` (framework-free); p1V's `$lib/ui/SplitPane.svelte` (paneforge 1.0.2) with source sizes 60/30 and 35/20, collapsed 3.
- **p21, AI slots:** I own the tab and panel frames and AI tab labels. p21 exports `features/ai/views/{desktop,mobile}/{AnalysisPanel,ChatPanel}.svelte` with `{consent, view, onconsent, onintent}`. The job-keyed container retains chat `draft` and desktop `scrollTop`, maps callbacks to the actual generated `AppIntent`, and dispatches analysis `shown`/`hidden`. Run5's app projection supplies clocks, timezone, layout and request keys; TS adds none. `layouts.phone_ai` chooses panel views; `capabilities.ai` / `ai_chat` control visibility. p21 acknowledged this composition.
- **p1Y, test ids:** `player-waveform`, `player-time`, `player-toggle`, `youtube-player`, `transcript-line-{id}` with `data-active`. The final list comes with the views.
- **Lead, decorative peaks:** they stay in the TS media/view adapter as presentation-only geometry. The deterministic source shape is kept. They are never used to infer duration, silence, progress or audio content. The view computes them once per stable job id and length, never per frame or render, and evidence labels them decorative, not decoded peaks. Playback intent and state stay in core, and measured timing comes from the real player. No Rust/core scope. p1Y runs the media suite at the coherent frontend checkpoint, not against partial view models.
- **p1V, frame seam (accepted):** both entry containers take `{ vm: SignedInVm, dispatch: (intent: AppIntent) => void, media: MediaPlatform }`.
  - `$lib/features/history/containers/SidebarHistory.svelte` is the desktop sidebar and phone drawer list. It also takes `collapsed`, which p1V owns.
  - `$lib/features/workspace/containers/HomeContent.svelte` switches on `vm.route` and `vm.layouts` between the workspace, history page and transcript. It also takes a bindable `menuOpen` for the phone header's "More options" (`home/[[...jobId]].tsx:387-395`).
  - The frame, header, drawer chrome, discount alert and the feedback/support/settings/purchase dialogs are p1V's.

## Projection batch for p1X (one VM/fixture revision; source-cited, no TS workarounds)

Everything below comes from the mounted desktop and phone views. None of it is a new business policy. X reports the consolidated batch landed in one revision; `core.md` is the source handoff. This inventory is retained for source traceability, not as outstanding requests to X. Consume the actual generated fields; do not derive substitutes from status, names or language tables.

**`JobRow` / `JobListVm`.** Source: `JobListTable.tsx`, `MobileJobListView.tsx`, `MobileJobCard.tsx`, `job-management/domain/models.ts`.
- **H1 display name:** `removeFileExtension(job_name || "Untitled")` (table `:181,299`; card `:72`; `transcriptFileSaver.ts:230`). `name` is raw.
- **H2 translation:** `is_translation` (non-empty `translate_to`) picks the Languages icon over play/mic when the row is not processing (table `:197-203`; card `:105-111`).
- **H3 actions:** `can_download` / `can_feedback`. Both actions show only when completed (table `:252-273,374-395`).
- **H4 speakers:** the card shows duration only when present and "2+ speakers" when `job.speakers` (card `:141-152`; "1 speaker" is unreachable).
- **H5 language fallback:** the desktop subtitle shows `job.language || "Auto-detect"` (table `:228`).
- **H6 dates:** `formatJobDate` is fixed `en-US`, `{year numeric, month short, day numeric, hour 2-digit, minute 2-digit}` (`models.ts:195-203`). The desktop xl column and the card use it; the phone table uses `toLocaleDateString()` with no options (table `:354`). The lead keeps the fixed policy. Platform fact: the source formats in the browser's local time zone.
- **H7 end of list:** "No more jobs to load" shows when `!hasMore && rows > 20` (table `:465-469`).

**`TranscriptVm`.** Source: `Transcript.tsx`, `TranscriptLinesView.tsx`, `TranscriptLineView.tsx`, `SpeakerLabel.tsx`, `TranscriptTitle.tsx`, `statusConfig.tsx`, `TranscriptOptionsMenu.tsx`, `MobileJobDetailsView.tsx`.
- **T1 speaker dropdown:** lists the unique speakers with their labels, checks the line's current raw value, picking one sends `SpeakerChosen`, and "Edit speaker..." renames the current value (`SpeakerLabel.tsx:159-224`). This needs `LineRow.speaker_value` (raw) and options `{value, label: SpeakerLabel}`.
- **T2 language tabs:** shown only for translation jobs (`TranscriptLinesView.tsx:152`), each with a flag and name. This needs a show-tabs flag and a per-tab flag. The desktop tab label also switches between Translation and Transcript and shows the selected flag (`Transcript.tsx:670-680,754-776`).
- **T3 time text:** rendered as `startTime.split(",")[0] - endTime.split(",")[0]` (`TranscriptLineView.tsx:140`).
- **T4 empty state:** "No transcript lines available" when there are no lines and the status is not not_started/pending/running (`TranscriptLinesView.tsx:137-138,197-201`). This needs an `empty` flag.
- **T5 load error:** "Error loading transcripts:" + error (`:141-146`). Is there a lines-load failure state?
- **T6 header:**
  - The raw `job_name` title and `{duration} min` (`TranscriptTitle.tsx:149-152`).
  - The status badge: the key `t(job_status)` plus a 9-entry tone/icon/spin table (`statusConfig.tsx:21-148`), with different hues from the list `Tone`.
  - The translation flag is shown when it is a translation job (`:144-148`).
  - Download is disabled unless completed (`download_enabled`). Download all formats, Show Time and Show Speaker are in the options menu.
- **T7 phone speaker toggle:** the phone action sheet shows the speaker toggle when `job.speakers === true` (`MobileJobDetailsView.tsx:252,343`), while desktop/core `speaker_toggle` also excludes transcriptapi YouTube. Confirm whether a phone-specific flag is needed.
- **T8 copy:** the copy-all button appears when lines exist (`Transcript.tsx:690-699,777-784`).

**`UploadQueueVm` / `FileRow`.** Source: `FileListItem.tsx` (desktop and `mobile/components`), `ConfigurationSection.tsx`, `languageHelpers.ts`, `LanguageDropdown.tsx`, `MobileLanguageSheet.tsx`.
- **U1 flags:** each language has a flag, with `🌐` as the fallback (`LanguageDropdown.tsx:118-119,268,325,351`; `ConfigurationSection.tsx:285`). Labels are the raw English name except auto-detect, which uses `t(name)` (`LanguageDropdown.tsx:126-127`; `MobileLanguageSheet.tsx:103-104`). So pickers do not translate language names. The phone button upper-cases them (`ConfigurationSection.tsx:287`).
- **U2 progress:** the desktop row shows a progress bar for every state (0 failed, 100 uploaded, n uploading); the phone shows it only while uploading and below 100, with an `n%` text (desktop `:221-230`; phone `:269-280`; VM `:706,716`). This needs a `progress` field.
- **U3 size text:** desktop `formatBytes` uses Bytes/KiB/MiB/GiB with 2 decimals; phone `formatSize` uses B/KB/MB with 1 decimal. Duration text: desktop `ceil min` or `-`; phone `ceil min` or hidden; phone total uses `round min`.
- **U4 phone status labels:** "Uploading...", "Preparing...", "Ready ✓", "Failed", which differ from the desktop chip keys (phone item `:59-92`).
- **U5 phone configuration rules:**
  - Switching to translation sets the target to Spanish; switching back clears it (`ConfigurationSection.tsx:65-73`).
  - The phone audio picker writes `source_language` in translation mode and `language` in transcription mode (`MobileTranscriptionContainer.tsx:177-188`).
  - The target list excludes the audio language (`:113-116`).
  - Phone preferred languages are a static en/tr/de/id list, not usage (`languageHelpers.ts:26-33`). Auto-detect is pinned in the phone audio sheet only in transcription mode (`ConfigurationSection.tsx:56-63`).
- **U6 YouTube:** the YouTube source and beta banner are gated by the `TRANSCRIPTION_SOURCE_YOUTUBE` capability; microphone is off. The welcome header needs the profile full name (AppVm, pending).

**Player mechanics (mine, TS adapters).** The phone audio player (`MobileAudioPlayer.tsx`) is wavesurfer-backed. Written controls preserve ±10 s skip, rates 0.5/0.75/1/1.25/1.5/2, optional expansion and Media Session metadata/handlers. `WaveformPlayer.setPlaybackRate` is a narrow mechanical addition, not an adapter rewrite. p24 published the exact player machine keys.

**Addendum.**
- **T9 freemium banner** (`FreemiumLimitsBanner.tsx:41-116`; mounted at `TranscriptLinesContainer.tsx:501-510`): it shows `{current} / {max} min`, a progress of `min(current/max*100, 100)` with max 20, and Upgrade, which opens the billing modal (p1V/p1R purchase). `upgrade_banner` exists, but the current and max minutes and the percent are freemium-policy numbers for the VM, not TS arithmetic.
- **YouTube layouts:** the desktop modes (video/strip/float), height, float corner, the phone video/strip threshold and the phone video height are shell-local UI state persisted in `localStorage` (`useYouTubePlayerLayoutViewModel.ts:44-133`). They stay in my TS containers as UI mechanics. The "Video/Strip/Float" labels are hard-coded English and need p24 keys.

## Current source publication (2026-10-26)

Original source is retained. The released run5 boundary is consumed directly:
- Browser file/media adapters and their existing suites under `src/lib/platform/media/`; only narrow playback-rate and pre-ready YouTube disposal additions during view integration.
- Desktop/phone workspace views, shared language/configuration/file/alert/YouTube dialog views, named-fixture stories and `workspace.test.ts`.
- Existing desktop table and phone list, plus `DesktopSidebar`, `MobileRecent`, their named-fixture stories and `sidebar.test.ts`. Desktop sidebar feedback uses `sidebar_feedback`, not table `can_feedback`; drawer uses `recent` without re-slicing, removing extensions or recomputing durations.
- Transcript header, body, controls, desktop split/phone tabbed views; shared speaker controls, IME-safe line editing, player controls and persistent YouTube layout. Named-fixture transcript stories, UI player-state stories, `transcript.test.ts`, `player.test.ts` and `SpeakerMenu.test.ts`.
- Accepted `HomeContent` / `SidebarHistory` seams are written. `TranscriptContent` composes p21 panels and the one `MediaPlayer`. No hand-typed VMs, fixture replacements or generated-file edits.

| Prerequisite | State and next owner |
|---|---|
| X consolidated projection batch and AppState | Published in source; use refreshed `core.md`. X retains leaf projections including S1/S2 and AppState/epoch. No ownership transfer or duplicate leaf rework. |
| Sidebar/drawer projections | Published: `JobRow.sidebar_name`, `sidebar_feedback`; `JobListVm.recent` (first 20 `RecentRow`s), `recent_empty`. Drawer preserves raw filename, source duration quirk and selection; no paging/delete. |
| p24 catalogs and reachable-key manifest | Published in all 16 locales, source-only. Use exact machine keys, no natural-key aliases. |
| Generated TypeScript and Rust fixtures | **Published by Y, run5:** `/tmp/rustr-p5-tsgen/run5/report.json`, exit 0, 3 generator tests passed, 144 TS files and 21 fixture roots. p1Z is consuming actual `core/types/*.ts` and `core/fixtures.json`; no substitutes. Manifest SHA256 `5ff92934aa5daa730a35e6a7fd694c2a43dbeb40d6715bccc0e231edbfa2bb28`; fixture SHA256 `602865cdf1c9da5786b5234a8287f036912d09a91715e1ef40aa6eb31a9137b2`; captured source manifest `9d9284eef48df25ea754998b61cad09897b173ddbd5749e1f5552237cfac5e86` (Y/p22 publication evidence, not rerun here). |
| Implementation / crossed gate notice | **Resolved by authoritative P5-GEN-RELEASE-5:** lead and Y explicitly confirmed run5 remains valid and supersedes the delayed run2 hold. Source resumed against run5 without rollback or another generation/check. Earlier failure evidence is preserved; no older relay is a new hold. Original ownership and OMP/Astra session unchanged. |
| Verification | Y/lead-owned at the coherent boundary. This owner runs no generation/build/test/lint/formatter/browser/provider checks. No acceptance claim. |

p24's exact app keys: `p5.transcript.edit_line`, `p5.transcript.save_line`, `p5.transcript.edit_speaker_label`, `p5.history.translation_job`, `p5.player.skip_back`, `p5.player.skip_forward`, `p5.player.speed`, `p5.player.play`, `p5.player.pause`, `p5.youtube.mode_video`, `p5.youtube.mode_strip`, `p5.youtube.mode_float`. Also published: common `Download all formats` / `Edit speaker`; app `Audio is not available.`, `p5.duration.minutes` with `{minutes}`, `Untitled`, `Expand player`, `Close player`, `Player mode`. Raw English metadata languages remain raw except auto-detect; settings language names use common keys. Media Session raw titles and the Transgate AI brand remain data; protocol action IDs stay untranslated. Catalog handoff: `locales.md`.

Next owner: Y/lead's already-established verification lane. Source is available for the next explicitly authorized coherent consumer snapshot; no extra check or generation request is made here. The separately copied S1 snapshot predates this continuation and is not evidence for these views. The original media adapters remain; no new agents, audit or ownership expansion.

## Source handoff and verification boundary

| Surface | Source delivered |
|---|---|
| Workspace/upload | Desktop source order and plus menu; phone fixed Start and swipe mechanics; core-projected progress/duration/language choices, capability-gated YouTube entry, alerts and dialog. Picked/dropped `File`s are held once and forwarded unchanged; no TS validation, rounding, provider resolution or start policy. |
| History/sidebar/drawer | Table/cards and existing sentinel; per-surface names/actions/date formatting; S1/S2 sidebar feedback and raw recent rows. A single confirmation dialog owns a delete request when table and sidebar coexist. |
| Transcript | Core-projected header/status/translation tabs, text/time/speaker display, edit draft, rename and IME handling, RTL, freemium fade/banner, copy and both download actions. Phone options sheet is bound to V's header. Purchase/feedback only dispatch to their owners. |
| AI composition | Actual generated intent mapping only. Consent intro remains inside p21 panels. Draft/desktop scroll survives tabs and collapse, resets by job. Three independent layout fields remain independent. |
| Player | One real WaveSurfer/YouTube mount in a stable sibling across layout changes, event-driven timing, active-line highlighting, phone expansion without a second engine, rate/skip/Media Session, persistent video/strip/float and resize/corner controls. Aborting an unready YouTube mount destroys its iframe; late ready cannot reattach it. |

The ready-but-unrun regressions exercise preparation/ready gating, file identity, language selection, provider-error visibility, sidebar action isolation, raw drawer labels, edit initialization/IME, timestamp intent and active highlighting, translation time suppression, unfinished download gating, rename cancel/focus restoration, engine-authoritative playback controls and iframe identity across modes. Existing adapter suites remain; the YouTube suite adds abandonment-before-ready coverage.

Test IDs for Y's existing lane: `upload-files`, `upload-start`, `player-waveform`, `player-time`, `player-toggle`, `youtube-player`, `youtube-layout` (`data-mode`), `transcript-line-{id}` (`data-active`). Player/control stories explicitly describe their UI snapshots as non-playback evidence; transcript stories mount no engine/provider.

## Original source-only comments (before REPAIR-P5-1)

- **No execution evidence from this owner.** All new/changed view, story and regression source is uncompiled/unrun. Type/component/browser integration is pending the existing authorized Y/lead lane. The run5 exporter pass is generated-boundary evidence only; neither it nor the copied S1 snapshot validates this continuation.
- **No live-media claim.** Real YouTube playback, signed-storage streaming, upload PUT and Media Session behavior require their separately authorized runtime/provider proof. Decorative peaks remain source geometry, never measured audio.
- **Lifecycle completion, not rework:** two narrow additions to retained media adapters: playback-rate setter and abortable pre-ready YouTube mount. No transport, upload/measure algorithm, core policy or generated contract was rewritten.
- **Locale coordination:** p24 reports all 16 catalogs and reachable manifest now include the five exact source YouTube resize/corner labels, alongside the prior machine-key batch. No local locale validation was run.
- No intentional business-policy deviation. App context, no-audio expiry, URL refresh, filtering, retries and export semantics remain in core/wasm; the shell renders the landed fields and maps intents.

## REPAIR-P5-1 source handoff

Owned repair source is ready. This supersedes the original unexecuted-source status above only for the commands and surfaces listed here. No candidate3 files, tokens, generated contracts, media adapter architecture, or frame/sidebar props were rewritten.

### Root cause and repair

- C1 `e.subscribe is not a function` and C2 `store_rune_conflict` shared one cause: local bindings named `state` shadowed the Svelte `$state` rune in `PlayerControls`, `MediaPlayer`, and `TranscriptContent`. Snapshot bindings are now `playback`, the callback is `onPlayerState`, and the public `PlayerControls.state: PlayerSnapshot` contract is unchanged.
- `YouTubeSurface` now explicitly derives the `Mode` union instead of retaining the initial narrowed literal. Typed mode lists and validated stored corners remove the related type errors without `any` or type-suppression casts. The numeric resize control uses slider semantics, including Home/End boundaries; the existing iframe survives mode/size transitions.
- `browserMeasureEnv` now supplies its required browser timeout callback. Owned tests import the actual media factory, type the fake YouTube API directly, remove unsupported Testing Library `exact` options, and avoid the incompatible desktop/mobile component union in `render`.

### D10: approved transcript-fade accessibility deviation

Lead approved keeping all free-cut text readable: core already excludes rows outside the free limit (FRE-020). `TranscriptLines` no longer applies opacity to a row containing text or controls. Each core-selected fade instead controls an `aria-hidden`, pointer-transparent `notice-bg` background layer behind opaque semantic text, speaker and timestamp foregrounds. The core still selects the rows and fade percentages; the upgrade banner is unchanged.

This intentionally differs from the source's whole-row fading visual. No token was edited. Static WCAG calculations for `fg`/`muted` against `bg`/`surface`/`notice-bg`, over all three palettes and both modes, found no endpoint below 4.5:1; the minimum was 4.99669:1 (`paper-ochre` light, `muted` on `notice-bg`). This calculation is not rendered axe evidence.

### Actual fixture consumers

X published `TranscriptVm.rtl` and `TranscriptVm.long_strings` through actual state transitions; p22 switched the unchanged `fixture()` API to X's typed generated module. Both desktop and mobile now expose `Rtl` and `LongStrings` stories. Component cases preserve Arabic/mixed-script direction, LTR timestamps, and full long speaker names through rename. No fixture VM was handwritten.

### Files changed in this repair

Paths below are relative to `transgate/web/src/lib`:

- Production: `features/transcript/containers/{MediaPlayer,TranscriptContent}.svelte`; `features/transcript/views/shared/{PlayerControls,YouTubeSurface,TranscriptLines}.svelte`; `platform/media/measure.ts`.
- Tests: `features/transcript/containers/TranscriptContent.test.ts` (new); `features/transcript/views/transcript.test.ts`; `features/transcript/views/shared/{player,SpeakerMenu}.test.ts`; `features/workspace/views/workspace.test.ts`; `platform/media/{player,youtube-player,measure}.test.ts`.
- Stories: `features/transcript/views/desktop/DesktopTranscript.stories.ts`; `features/transcript/views/mobile/MobileTranscript.stories.ts`.
- This handoff: `docs/plans/p5-web/implementation/workspace-transcript.md`.

### Verification and preserved failures

Commands ran only in the live source checkout `transgate/web`, never candidate3.

- Final post-typed-loader command, start 19:52:02: `npm run test:ci -- src/lib/features/workspace src/lib/features/history src/lib/features/transcript src/lib/platform/media` — **exit 0; 13 files, 84 tests passed**. This includes the actual phone-job container using a plain media platform with the fixture's pending URL, snapshot-driven controls, freemium foreground opacity, the real browser metadata timeout callback, and the newly released fixture variants.
- `npm run build` — **exit 1 at svelte-check; 1,412 files, 2 errors, 0 warnings, zero owned-file diagnostics**. External blockers were `core/index.ts:5:42` missing `./pkg/transgate_wasm.js`/declarations, and V-owned `ui/TextField.stories.ts:13:23` union too complex. Both were relayed to their owners; the serialized source build slot was released to V. Vite bundling did not run. No whole-app build pass is claimed.
- New regression source was written before the corresponding production repairs. Initial launches exited 127 because Vitest was unavailable; no local red execution is claimed for those tests. C1/C2's captured runtime/type failures remain the before evidence.
- An overlapping p1Z `npm ci` exited 217 (`ENOTEMPTY` removing `posthog-js/dist`); the shared dependency collision also left subsequent test launches at exit 127. All p1Z installs then stopped. V alone performed the authorized corrective install and supplied an actual Vitest version proof before tests resumed. No further install/delete/retry was performed by p1Z.
- The first executable owned run had 80 passing assertions but **exit 1** from jsdom lacking `setPointerCapture`. A local DOM-method shim in the player test preserves its real pointer/click sequence; a scoped workspace scroll shim supplies jsdom's missing viewport API. Production handlers were not weakened or exceptions suppressed. Subsequent runs passed, including the final 84-test typed-fixture run.

### Remaining boundary / comments

- **No owned running job or installer remains.** Source is held for concrete owner-routed diagnostics.
- The missing source wasm-package prerequisite was subsequently cleared by integration support: `TASKR_WASM_BUILD_STD=1 ./scripts/build-transgate-wasm.sh` exited 0 in 62.49s. Receipt read at `/home/mu/.cache/rustr-integration/repair-p5-1-source-wasm.json` records hashes for all four package outputs, no tester node_modules changes, and no checkpoint execution. The earlier failed p1Z app-build result remains preserved above.
- V subsequently reported `npm run build` **PASS, exit 0: 1,414 files, 0 errors, 0 warnings; Vite 954 modules**, after the glue publication and its TextField typing repair. V also reported its owned Vitest **23 files, 85 tests passed**, with no changes to p1Z files/seams. These are V-reported results, not p1Z reruns or browser certification. V released the serialized build slot and reports no active install/build; p1Z remains held.
- Y retains the rendered checkpoint: rerun affected PlayerControls and phone-job stories, axe freemium contrast, and the new RTL/long-string stories under the authorized candidate pipeline. JSDOM tests do not prove layout, viewport overflow, actual browser pointer capture, iframe playback, or rendered contrast.
- No Storybook/browser/provider certification, new snapshot, new agent, or candidate3 mutation was performed in this repair.
