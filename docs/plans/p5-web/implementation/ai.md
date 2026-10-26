# P5 AI panels: consent, analysis, chat

Owner: `w23:p21`; original source by Opus, continued in the same saved session by Astra under explicit user authorization. Status: **core compiled in Y's generator run5; 3 generator tests passed and types/fixtures published. REPAIR-P5-1 owned AI Vitest: 3 files / 38 tests passed.** No worker browser, Storybook or provider certification is claimed; Y owns rendered verification. Earlier generator evidence belongs to lead/p1Y; the bounded worker component-test run is recorded below. The slice consumes the unchanged P4 start/header/drop/watch protocol, `llm_core::StreamState` and the analysis wire. There is no mounted cancel control, no quota or provider change, and no transport change.

## Files

Core (pure Rust, `transgate-core`; p1X adds `pub mod ai;` in `presentation/mod.rs` and folds the fixtures):

| File | Contents |
|---|---|
| `transgate/core/src/presentation/ai/mod.rs` | `RequestKey`, `WatchKind`, `AnalysisWire`, `WatchFailure{Disconnected, Http{failure}, Protocol}`, `StartRejection`; declares `analysis`, `chat`, `consent`, `fixtures`, `section_copy` |
| `presentation/ai/consent.rs` + `consent/tests.rs` (10) | `ConsentPanelState`, `Gate`, `Decision`, `ConsentPanelVm` |
| `presentation/ai/analysis.rs` + `analysis/tests.rs` (17) | `AnalysisPanelState`, `PanelState`, `ProgressVm`, `AnalysisPanelVm`, `ExportFailure` |
| `presentation/ai/chat.rs` + `chat/tests.rs` (13) | `ChatPanelState` (Debug + Default only), `ChatPanelVm`, `ChatLineVm`, `ChatErrorVm`, `LimitsVm`, `UsageVm`, `ResetLabel`, `Rating`, `HistoryEntry` |
| `presentation/ai/section_copy.rs` + `section_copy/tests.rs` (7) | `SectionId` (19 ids), `section_copy(&Analysis, SectionId) -> Option<(text, html)>`, `by_priority` |
| `presentation/ai/fixtures.rs` (1 test) | `NOW`, `consent()`, `analysis()`, `chat()`, all reached through `update()` |

Web (`transgate/web/src/lib/features/ai/`):

| File | Role |
|---|---|
| `intents.ts` | `ConsentIntent`, `AnalysisIntent`, `ChatIntent` (views emit these; no keys, no clocks) |
| `format.ts`, `markdown.ts` | on-screen `m:ss`/duration/local date; escaped `simpleMarkdown` |
| `views/desktop/AnalysisPanel.svelte`, `views/mobile/AnalysisPanel.svelte` | analysis tab per layout, props `{consent, view, onconsent, onintent}` (+ phone `scrollResetKey`) |
| `views/desktop/ChatPanel.svelte`, `views/mobile/ChatPanel.svelte` | Ask AI per layout; bindable `draft` (+ desktop bindable `scrollTop`, phone `scrollResetKey`, `bottomOffset`) |
| `views/shared/AiGate.svelte` | AIFeatureWrapper + AIIntroduction + AIConsentModal |
| `views/shared/AiConsentSettings.svelte` | settings AI tab (one responsive view, as the source); props `{consent, showTitle?, onintent}` |
| `views/shared/{Generic,Meeting,Lecture,Sales,Support,Interview}Analysis.svelte`, `AnalysisContent.svelte`, `SectionHeader.svelte`, `TypeBadge.svelte`, `ExportMenu.svelte`, `ChatMessage.svelte`, `ChatLimits.svelte` | building blocks |
| `*.stories.ts` (5 files) | `Features/AI/DesktopViews|MobileViews/...`, rendering only `fixture(...)` |
| `views/desktop/ai.test.ts`, `views/mobile/ai.test.ts`, `views/shared/shared.test.ts` | component tests per platform; markdown escaping |

## Contracts other owners consume

**p1X composer (AppIntent → leaf events).** The app projection supplies keys and clock/layout context when constructing leaf events. The table below describes those internal leaf events, not fields the TypeScript container must supply. Run5 generated analysis/chat intents contain no `at` or `phone` fields; the container only maps callbacks to the generated `AppIntent`.

| View intent | Leaf event |
|---|---|
| consent `enable`/`decline`/`disable`/`dismiss_error` | `consent::Event::EnableRequested`/`DeclineRequested`/`DisableConfirmed`/`ErrorDismissed` |
| analysis tab shown/hidden (p1Z container) | `Shown{key}` / `Hidden` |
| analysis `retry` | `RetryRequested{key}` (new key) |
| analysis `copy` | `CopyRequested{at, utc_offset_minutes}` |
| analysis `copy_section` | `CopySectionRequested{section}` |
| analysis `export` | `ExportRequested{format, at, utc_offset_minutes, phone}` (`phone` selects the source title rule) |
| analysis `dismiss_export_error` | `ExportErrorDismissed` |
| chat `send{text}` | `SendRequested{text, key}` |
| chat `retry{index}` | `RetryRequested{index, key}` |
| chat `copy_message`/`rate`/`copy_chat`/`download{}` | `CopyMessageRequested`/`RateRequested`/`CopyChatRequested`/`DownloadRequested{at}` |
| chat `dismiss_error`/`dismiss_notice` | `ErrorDismissed`/`NoticeDismissed` |

The leaf's `at` and `utc_offset_minutes` are internal app-supplied context, not additional view-intent fields. Consent reaches the panels as `ConsentChanged{granted: consent.granted()}` whenever it changes; the app fences epoch and job.

**p22 driver.** Signatures as messaged: consent `LoadConsent`/`RecordConsent{granted}`/`RevokeConsent` → `Loaded`/`DecisionSaved`. Analysis `LoadAnalysis`, `StartAnalysis{job,key}`, `Watch{generation, kind: Analysis, after: None}`, `Detach`, `Copy{text,html}` (no reply) and `Export{document, format}` → `ExportFinished{format, result}`. Chat `LoadHistory`, `LoadLimits`, `StartChat{job,key,message,history}`, `RetryChat{generation,key}`, `Watch{kind: Text}`, `LoadGeneration{generation}` → `GenerationLoaded` (new), `CopyText`, `CopyChat` → `ChatCopied{ok}`, `DownloadText`, `Telemetry`. Lead D3: a held font load, copy or save must not deliver captured content after logout, account switch, consent DELETE/refusal or disposal. The guard is the executor's `AppEffect::Ai{epoch, …}` envelope; the leaves capture at click and never re-read the latest data.

p22 acknowledged: `CopySectionRequested` reuses the existing `Copy` path. `LoadGeneration` maps to `HttpApi::generation(user, id)` (`GET /api/ai/generations/{id}`) and returns `GenerationLoaded{generation, result}`. For D3, the driver checks p1X `AppState::is_current(epoch)` and disposal immediately before every `Copy`/`CopyText`/`CopyChat` clipboard write, every `DownloadText` save/share and every `Export` save, and again after the PDF font await. A stale held capture is dropped and never re-captured. The held-font regression is in `transgate/client/tests/export.rs`; an AppState-driven variant follows `app.rs`. The p1X aggregator, not p22, folds the fixture functions in.

**TS/fixtures (p22 convention).** VM types carry `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]`; `i64`/`Micros` fields are typed `number`. p1X owns the `ts` feature and the derives on reached types: `crate::ai::analysis::*` shapes (and `Rating` as `number`), `ChatRole`, `Msg`/`Arg`, `job_list::Tone`, `documents::DocumentFormat`, `section_copy::SectionId` (already derived). Fixture names: consent `loading, required, granted, enabling, disable_failed, cleanup_pending, load_failed`; analysis `checking, no_transcript, starting, streaming, generic, meeting, lecture, sales, support, interview, interrupted, rejected, unconfirmed, exporting, export_failed`; chat `loading_history, welcome, history, disabled, typing, streaming, sync_warning, limit_reached, copied`.

**p1Z composition (reported against run5).** `features/transcript/containers/TranscriptContent.svelte` maps AI callbacks to the actual generated `AppIntent`, with no extra time/key fields. It chooses mobile or desktop panels via `layouts.phone_ai`, gates visibility with `capabilities.ai`/`capabilities.ai_chat`, and retains chat `draft` and desktop `scrollTop` in the keyed job container. The panels mount their consent introduction/modal themselves. No p21 source changes or worker checks were needed for this composition; the clock-stamping guidance above supersedes the earlier container-stamping proposal.

**p24 locales.** Every key used is listed in the messages sent on this date: intro/modal, limits, type badge, quota codes, and the `app` keys incl. `"AI content deletion is in progress."` and `"Scroll to latest message"`. The D1 fix replaced the old local-save key with `app` `"This answer could not be saved. It may disappear after refresh."`. Strict i18n makes stories and tests fail until these land.

## Behavior notes and deviations (for lead disposition)

- **D21 (lead decision, applied):** a failed consent write (enable, decline, disable) keeps the settings controls and the last server-confirmed state, with that action's error inline (`action_error`). Decline now reports "Failed to record consent decision". Only the initial read failure (`load_error`, "Failed to load consent preferences") replaces the tab. The `app` keys "Failed to record consent" and "Failed to revoke consent" are no longer used. Regression: `consent/tests.rs::a_failed_write_is_an_inline_action_error_never_a_load_error`.
- **D22 (final keys consumed in source; p24 reports final catalogs source-ready; behavioral verification PENDING with Y).** The mapping is unchanged, per corrected decisions.md and core.md 1.1a. Modal "Decline" is `RecordConsent{granted:false}` (`POST false`): nondestructive, keeps completed results. The settings action stays `DisableConfirmed` → `RevokeConsent` (`DELETE`): revoke plus account-side purge, provider deletion unverified. There is no new endpoint or control, and the consent epoch fence is unchanged.
  - **Modal (`views/shared/AiGate.svelte`):** the "What this means:" list renders `app` `p5.consent.provider_processing{provider}`, the source HTTPS bullet, `provider_retention`, `account_results`, `decline_keeps` and `settings_delete`. These replace the source processing, permanence, local-cache and "revoke and delete at any time" bullets; none of the replaced sentences render.
  - **Settings (`views/shared/AiConsentSettings.svelte`):** the action is labelled `p5.consent.turn_off_delete_action`. Its confirmation is titled `turn_off_delete_title`, described by `turn_off_delete_confirm` (replacing the common source body) and confirmed with `turn_off_delete_button`. While `ConsentView.cleanup_pending`, a status shows `p5.consent.cleanup_pending`, never a completed-deletion claim. This replaces p21's earlier `"AI content deletion is in progress."`, which is withdrawn.
  - **Withdrawn keys, not used:** `turn_off_keeps`, `deletion_scope`, `disable_confirm`, `delete_confirm`, `delete_action`, `delete_title`, `delete_button`. The briefly drafted separate delete control, `DeleteConfirmed`, `DeleteAiData`, `can_delete` and `p5.consent.delete_failed` were all reverted before handoff.
  - **Regressions:**
    - `consent/tests.rs::turning_off_in_settings_is_the_revoking_delete_distinct_from_the_refusal`: turn-off emits `RevokeConsent`. A failed DELETE leaves consent granted with no purge shown as started and an inline error. A pending purge stays pending until the server reports it done.
    - `declining_in_the_learn_more_modal_records_a_refusal`: decline emits `POST false`.
    - `views/desktop/ai.test.ts`: the modal shows the disclosure, found by catalog names, and no permanence or local-cache promise.
    - `views/shared/shared.test.ts`: the destructive action dispatches once, only after confirmation; a failed revoke keeps the control; a pending purge is shown as pending.
- **Escaping:** section copy HTML and the chat markdown escape model/server text first. The source interpolated it raw.
- **Copied marks:** section, whole-analysis and message copy show the check mark optimistically for 2 s, because `Effect::Copy`/`CopyText` have no result. Whole-chat copy uses the core `ChatCopied` notice.
- **Interview next steps** are displayed and copied high → medium → low (stable), as the source did. Export keeps the captured order, as the source passed raw data.
- **Chat history load failure:** welcome card plus an error card, not the source's synthetic assistant bubble. Progress percentage is clamped to 100.
- **Unknown start outcome:** one same-key replay, then a new error state with `app` "The AI request could not be confirmed…".
- **Settings tab:** one responsive view, as the source. The settings owner mounts it in both layouts.

## Test selectors (web-shell.md)

Tests locate by role and accessible name first. The export control is a disclosure button (`aria-expanded`) over a list of buttons, not an ARIA `menu`. `data-testid` marks only containers that have no semantic role: the panel roots `analysis-panel`, `chat-panel`, `ai-introduction` and `ai-consent-settings`; `analysis-progress`, `analysis-type`, `chat-limits`, `chat-message-user` and `chat-message-assistant`; the shape roots `generic-analysis`, `meeting-analysis`, `meeting-analysis-empty`, `lecture-analysis`, `sales-analysis`, `support-analysis` and `interview-analysis`; and the source section ids `action-items-list`, `decisions-list`, `discussion-points-list`, `next-steps-list`, `topics-list`, `concepts-list`, `review-questions`, `study-guide`, `call-metrics`, `coaching-tips`, `customer-insights`, `sales-next-steps`, `issue-resolution`, `sentiment-analysis`, `support-insights`, `kb-updates`, `follow-up-actions`, `candidate-assessment`, `skills-assessment`, `notable-responses` and `interview-next-steps`.

## Verification to run (lead/p1Y)

```sh
cargo test -p transgate-core presentation::ai          # consent 10, analysis 17, chat 13, section_copy 7, fixtures 1
(cd transgate/web && npm run check && npm run test:ci)  # ai.test.ts ×2, shared.test.ts, stories.test.ts
```

Remaining verification: AI behavior selectors (including chat replacement/append behavior and D21/D22), web type checking/component tests, `Msg` structural fit with `i18n.t`, Storybook a11y results, and real clipboard/download behavior on phones. Y's generator compilation and artifact publication do not establish those results.

### Y first-generator diagnostic: E0502 repair (source only)

- Y reported `E0502` at `transgate/core/src/presentation/ai/chat.rs:539:30`; log `/tmp/rustr-p5-tsgen/run1/generate.log`. The first generator run produced no outputs.
- `ChatPanelState::set_answer` used `match self.lines.get_mut(index)` with a guard reading `self.lines.len()` while the mutable borrow remained live. The repair uses `if let … else if …`, separating the borrows.
- Behavior is unchanged: an existing line's content is replaced without changing its role/generation; `index == len` appends one assistant line with no generation; `index > len` does nothing. No allocation or text clone was added.
- Changed source: `transgate/core/src/presentation/ai/chat.rs`, SHA-256 `ab89a40f7364a99aa26d00326228e473b7faea56bb998db8fb6e26d13dc4e386`.
- p24 has reported the final catalogs source-ready, superseding the earlier catalog-wait note. D22 remains settings `DELETE` and modal decline `POST false`; D21 inline failures and honest `cleanup_pending` reporting are unchanged.
- No worker checks were run. Y subsequently compiled this repair in succeeding generator runs; run5 evidence follows. No chat-regression selector result is claimed.

### Y generator run5: published contracts consumed

- Evidence: `/tmp/rustr-p5-tsgen/run5/report.json` and `/tmp/rustr-p5-tsgen/run5/generated.sha256`. Y ran `cargo test --locked -p transgate-core --features ts --lib presentation::fixtures::web::generate`: exit 0, 3 tests passed, 0 failed.
- Published under `transgate/web/src/lib/core`: 144 TypeScript files and 21 fixture roots. Reported generated-manifest SHA-256: `5ff92934aa5daa730a35e6a7fd694c2a43dbeb40d6715bccc0e231edbfa2bb28`; fixture SHA-256: `602865cdf1c9da5786b5234a8287f036912d09a91715e1ef40aa6eb31a9137b2`.
- The real AI contracts are now available: `Analysis` is a discriminated union intersecting its six content shapes; `AnalysisRating` is numeric; `Rating` remains chat `"like" | "dislike"`; `MeetingAnalysis.decisions` uses `AnalysisDecision`, distinct from consent `Decision`. Y reports 17 content structs with 49 optional fields excluding null. Nullable panel fields remain explicitly nullable.
- Existing AI view imports and optional-field reads already use these shapes; no adapter, generated-file edit, or speculative view change was needed. The published fixture helper exposes `ConsentPanelVm`, `AnalysisPanelVm`, and `ChatPanelVm`, with the named fixtures consumed by the existing stories.
- Locale and generation prerequisites are source-ready. Source remains held for Y's concrete diagnostics; no worker checks or new batch were launched.

### REPAIR-P5-1: settings privacy-link identification

- Lead C1 finding: `link-in-text-block` on `Features/AI/DesktopViews/AiConsentSettings`, 71 cases; accent link versus surrounding text measured 1.22:1 with no persistent non-color cue. Original evidence remains `/home/mu/.cache/rustr-integration/s1-20261026151947/logs/next-p5-c/c1-summary.json`.
- Fix: `views/shared/AiConsentSettings.svelte` now sets `text-decoration-line: underline` directly on the privacy-policy link instead of relying on `hover:underline`. It is unconditional across states/themes and needs no token change. Desktop and phone use this same responsive component; the `Phone` story mounts it without a separate link implementation.
- Regression authored before the source fix: `views/shared/shared.test.ts` → `distinguishes the privacy-policy link from surrounding text without hover`. It renders the real settings component and checks the link's computed decoration, not a Tailwind class string.
- Before-fix execution attempts exited 127 (`vitest: command not found`) during the shared dependency-install collision; neither attempt executed a test. Those infrastructure failures are not claimed as red regression evidence. Only p1V repaired dependencies; p21 ran no install or deletion.
- After p1V's authoritative corrective readiness, `npm run test:ci -- src/lib/features/ai/views/shared/shared.test.ts src/lib/features/ai/views/desktop/ai.test.ts src/lib/features/ai/views/mobile/ai.test.ts` in `transgate/web` exited 0: **3 files / 38 tests passed**, start 19:46:37, Vitest duration 1.80 s.
- One unowned warning surfaced: `src/lib/ui/Dialog.svelte:49:2`, `a11y_no_noninteractive_tabindex` (noninteractive element with nonnegative tabIndex). Relayed to p1V/lead; that file was not changed.
- Changed files: the shared settings component, its shared component test, and this handoff. No token, candidate3, Storybook/browser or generated-contract changes. Y must certify the repaired rendered link on a newly released candidate; passing jsdom tests is not C1 certification.
