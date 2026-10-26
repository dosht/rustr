# P4 core policy: source handoff

Owner: w23:p1X. The original P4 handoff below was source-only and unexecuted by this owner. The bounded REPAIR-P5-3b section records subsequent executed regression evidence; it does not imply new acceptance of the entire P4 phase.

## Files

- `transgate/core/Cargo.toml`: adds `llm-core = { path = "../../crates/llm-core" }` and `regex = "1"` (p23 sanitation). The lead refreshes `Cargo.lock`.
- `transgate/core/src/lib.rs`: `pub mod ai;`.
- `transgate/core/src/ai/`
  - `mod.rs`: `GenerationId`, `AiError` (`http_status()`, `message()`), `QuotaRefusal`, the views `GenerationView` (with `execution` and `tokens_used`), `Started`, `ConsentView`, `LimitsView`/`LimitUsage`, `SummaryView` and `AiPassReport`. It declares p23's `analysis`, `prompts` and `sanitize`.
  - `ports.rs`: the `AiTx` trait and its records. Lock order and conflict outcomes are documented on each method.
  - `consent.rs`: `is_valid`, `epoch`, `may_write`, `may_read`, `covered`, `decide` (POST), `revoke` (DELETE) and `view`.
  - `quota.rs`: `ChatLimits::from_env`, `estimate_tokens`, `next_reset`, `refreshed`, `admit`, `actual`, `settle`, `release` and `current`.
  - `generation.rs`: the lifecycle and its fences (`fenced`, `digest`, `accepted`, `claim`, `lease_expired`, `owns`, `checkpoint`, `stage`, `complete`, `interrupt`, `cancel`).
  - `live.rs`: `LiveHub` (`open`/`subscribe`), `Publisher` and `Subscription::recv`. It uses bounded `llm_core::AttachmentQueue`s. Only the original attachment receives raw deltas.
  - `presentation.rs`:
    - `public_error`;
    - `GenerationView::frame()`, the replacement frame for text generations. Terminal frames are snapshots that keep the retained partial text;
    - `AnalysisEvent` and `analysis_events(previous, current)`, which map to the source events sectionCount, typeDetected, sectionReady, complete and error;
    - `Staleness`/`staleness` (AI-008).
  - `app.rs`: `Ai`, `AiServices`, `AiConfig`, `ModelSettings::{source, source_baseline}`, `Bounds::default()`, `ChatRequest`, `HistoryTurn` and `Watch`.
- Existing files I integrate:
  - `jobs/ports.rs`: `JobUow::ai(&mut self) -> &mut dyn AiTx`, plus three new `JobQueries` reads: `runnable_generations`, `pending_summary_requests` and `pending_ai_purges`.
  - `jobs/testing.rs`: `AiData` and `impl AiTx for MemUow`. AI rows are removed by erasure and by hard delete.
  - `jobs/app.rs`:
    - a completed result records the summary request in the same transaction;
    - soft deletion and line edits advance the source revision.
  - `privacy/app.rs`: retention line deletion calls `bump_source_revisions`.

## Use cases (all `async`, `Result<_, AiError>`)

| Call | Meaning |
|---|---|
| `consent(account)` | `GET /api/ai/consent`; no prior consent is needed |
| `record_consent(account, bool)` | POST: a grant or a refusal; refusal fences in-flight work and keeps completed artifacts |
| `revoke_consent(account)` | DELETE: commits false, a new epoch and the purge cutoff first, then purges. `cleanup_pending` stays true if the purge fails |
| `purge_pass()` | Purges that were left pending |
| `limits(account, job)` | API-BIL-011. Read-only, no consent needed. Paid callers get `has_limits: false` |
| `start_analysis(account, job, key)` / `start_chat(account, job, key, ChatRequest)` | Admission in source order. Returns `Started { created }`: an exact replay gives `created: false`; reusing a key for another payload is `Conflict` |
| `retry(account, generation, key)` | New metered work that repeats the request of a terminal generation |
| `generation`, `watch`, `analysis`, `chat_history`, `completion_summary` | Reads; each requires ownership, current valid consent and no covering DELETE cutoff |
| `cancel(account, id)` | Idempotent. Releases any reservation; proves nothing about remote cancellation |
| `run(id)`, `run_pass()`, `summary_pass()` | Workers. Recovery completes a staged final with no provider call. A lost worker without a final becomes interrupted with its remote outcome unknown |
| `summary_outcome(job)` | The callback ordering boundary. `None` while pending; once decided it is `Saved`, `Absent`, `Denied` or `Failed`. Never changes the job or triggers mail |
| `Ai::request_summary(uow, account, job, now)` | The producer, called by `Jobs` inside the completing transaction |

## Policy choices made in source

- **D1:** `ModelSettings::source()` omits `json_object` for live analysis. `source_baseline()` keeps it, for E4 only.
- **D8:** analysis `tokens_used` is `prompts::analysis_input_estimated_tokens`, the UTF-16 estimate of the transcript actually submitted. The D4 size bound applies to decoded content: prose bytes, or the serialized typed analysis. The outer JSON envelope and its escaping are not counted.
- **D2 quota:**
  - Source precedence: questions use the stored maximum; tokens use the configured maximum plus the estimate.
  - Admission reserves the question and its estimate, bound to the admission period. Settlement charges `ceil(UTF16(question + answer)/4)` once.
  - Paid callers skip everything, as do freemium callers with no enabled cap.
  - A failed plan query counts as freemium only if its savepoint rolls back cleanly.
  - One active metered generation per account; a second request is `Busy`.
  - An unknown COMMIT on admission or completion is resolved by reading the row back.
  - Single-bucket invariant (no history table): a counter rolls over only inside an admission. Admission returns `Busy` before locking or refreshing any counter while the account has an active metered generation. Limits reads only project the new period. A late settlement therefore always lands in its admission period.
  - Regression: `an_answer_settled_after_midnight_lands_in_its_admission_day`.
  - A unique-key refusal on insert (23505) is rolled back, then re-read in a fresh transaction: a replay, a conflict or `Busy`.
- **D3:** consent is valid only when granted and not revoked. Every decision advances the epoch. Writes need the exact admission epoch. Reads need current valid consent and an epoch above the DELETE cutoff. Legacy rows have epoch 0 and are always covered by a DELETE.
- **D4:** `Bounds::default()` sets 16 MiB input, 1 MiB output, 64 KiB queued deltas, 8 dispatches per process, a 60 s deadline and a 90 s lease. Prose is checkpointed every 4 KiB as a sanitized replacement. A process at capacity refuses admission before anything is reserved.
  - While the provider is quiet, a tick every `quiet_millis` (250 ms) saves unsaved prose, or else runs a fence probe with no write. Other replicas therefore see the partial, and a refusal or cancellation stops a silent stream.
  - The pending provider read is kept across ticks and never restarted. The timer comes from `AiServices.sleeper` (the host's `Sleeper`).
  - Live watch attachments: at most 32 per process and 4 per account. Beyond that the request is refused at once with `AiError::Capacity`; there is no queue. The slot is freed when the `Watch` is dropped.
  - The 250 ms fence check also runs while the provider's headers are still pending.
- **D9 (LLM dispatch intents):**
  - What is recorded: `ai::ports::LlmDispatch` holds account, generation, provider, model and disposition, plus at most 8 distinct provider ids of at most 256 bytes each. It never holds content.
  - Unkept evidence: a non-empty observed id that cannot be kept (over 256 bytes, or the list is full) sets the durable, monotonic `unkept_id_observed`. Once set, no later settlement can make the dispatch "not dispatched" or its erasure item Absent. Ids are never truncated or invented.
    - Store and model: p1R's migration `0006` and p1W's `ItemTarget::LlmDispatch` carry the flag.
    - Before-fix evidence: the before-fix failure of this two-call case was never observed. The captures in artifacts 662 and 663 stopped at compile.
  - When: it is written in the claim transaction, before any provider call.
  - Disposition: pre-open exits, and open errors where `RemoteOutcome::NotDispatched` says nothing was sent, record "not dispatched", but only when no id was ever observed. Every observed id is stored at once through `Ai::observe`.
  - Lifetime: the record survives consent DELETE purges and hard job deletes. Account erasure turns it into an `ItemTarget::LlmDispatch` item (p1W's model), then deletes the row.
  - Late ids: an id observed after the cascade updates only that erasure item, through `save_erasure_item_target`. The attempt count goes up, and new dispatch evidence reopens a closed erasure.
  - Closing the item: a possible dispatch stays Unresolved (`LLM_RETENTION_UNVERIFIED`). Only an operator's explicit approval with a rationale closes it. `ExceptionPolicy` never does.
- **D5:** the original question is stored as the user message; the sanitized copy goes to the provider and to accounting. History roles must be user or assistant (AI-D06). An existing summary of the job is reused (JOB-033).
- **D15 (P3), operator approval:** `Privacy::approve_exception(id, item, &OperatorApproval { operator, rationale })`.
  - Core refuses a blank operator, and a blank rationale on an unresolved `UploadResweep` or `ArchiveInFlight` item.
  - Legacy inventory and other pending work cannot be approved.
  - Auth boundary (p1T's route `POST /api/admin/erasures/{erasure_id}/items/{item_id}/exception`): a non-admin gets 403 before core is called. `operator` comes only from the verified session (`caller.email`), never from the body.

## Tests (source only)

`cargo test --locked -p transgate-core`:
- `ai::quota::tests`: `configured_caps_follow_the_source_parser`, `admission_keeps_source_precedence_and_maxima`, `settlement_charges_the_combined_estimate`, `late_settlement_never_touches_a_later_period`, `periods_reset_on_utc_day_iso_week_and_calendar_month`.
- `ai::consent::tests`: `refusal_fences_writers_but_regrant_restores_reads`, `delete_cuts_off_every_older_epoch_for_good`, `no_row_is_no_consent_and_delete_still_covers_legacy`.
- `ai::app::tests`:
  - `a_freemium_answer_saves_the_pair_and_settles_once`
  - `a_request_key_replays_and_never_redispatches`
  - `paid_callers_bypass_the_quota_lane`
  - `freemium_caps_and_the_single_metered_lane_refuse_before_dispatch`
  - `refusal_during_streaming_fences_the_answer`
  - `refusal_keeps_artifacts_and_delete_purges_older_epochs`
  - `a_failed_purge_stays_pending_and_covered_rows_stay_hidden`
  - `recovery_completes_a_staged_final_without_the_provider`
  - `a_lost_worker_without_a_final_is_interrupted_not_replayed`
  - `unsuccessful_finish_and_source_edits_interrupt`
  - `analysis_streams_typed_sections_in_source_event_order`
  - `the_completion_summary_releases_callback_ordering_with_an_explicit_outcome`
  - `consent_and_limit_metadata_need_no_prior_consent`
  - `a_quiet_provider_still_checkpoints_the_partial_before_the_final`
  - `an_answer_settled_after_midnight_lands_in_its_admission_day`
  - `a_dispatch_intent_is_durable_before_io_and_survives_purges`
  - `not_dispatched_is_recorded_only_without_dispatch_evidence`
  - `a_late_provider_id_after_the_cascade_updates_only_the_erasure_item`
  - `watch_attachments_are_capped_and_released_on_disconnect`
  - `a_refusal_while_waiting_for_headers_stops_the_worker`
- `privacy::app::tests` (D9): `a_possible_llm_dispatch_keeps_erasure_open_until_an_operator_approves`.
- `privacy::app::tests` (D15): `an_unobservable_upload_closes_only_by_operator_approval`, `an_unresolved_legacy_inventory_cannot_be_approved_away`.

## Remaining and live prerequisites

- Real PG two-process admission, period and unknown-COMMIT races are p1R's (`transgate/jobs-postgres/tests/ai.rs`). Host restart, reconnect and SSE are p1T's. Browser/wasm and UniFFI are p22's. The E4 comparison is p24's.
- Imported null-reset counter history, live model availability, provider retention, region and cost, and deployment config remain live prerequisites.
- P7 callback copies and P9 legacy summaries/highlights must join `purge_ai_artifacts` when they exist.
- AI-027/API-F44 (voice demo) is excluded under D6. No code was written for it.

## REPAIR-P5-3b: chat context refusal parity

Lead confirmed Y measured the stored C2 line at **33 UTF-16 units**; the minimum-context rejection is the confirmed trigger, not missing transcript storage or host job-ID remapping.

Read-only source evidence under `/home/mu/Business/Transgate/frontend/src/`:

- `lib/ai/content-processor.ts:85-98` accepts 50..512000 JavaScript string units inclusive; its internal short-content diagnostic is `"Content too short for meaningful analysis"`.
- `lib/ai/ai-service-manager.ts:189-199` sanitizes the joined context, invokes that validator, and deliberately replaces **every** invalid-context diagnostic with `"No transcript content available for chat"`. The internal short-content diagnostic is not user-facing chat copy.
- `pages/api/v1/ai/chat.ts:91-93` separately refuses zero transcript rows with `"No transcript content found"`; `:152-155` forwards service error events unchanged.
- `features/aiAnalysis/services/ChatStreamService.ts:103-105` forwards the event error; `viewmodels/useChatViewModel.ts:153-163` stores it as the displayed error.

Approved decision: preserve all thresholds, sanitization and validation precedence. In `transgate/core/src/ai/app.rs:559-560,574-576`, both refusals use the existing `AiError::Invalid` with their distinct exact source strings. The shared `NoTranscript` variant and its analysis-specific `"No transcript content found for this job"` copy stay intact. No new API, enum variant, locale key, automatic retry or length-specific UI copy was introduced.

| Chat condition | HTTP status | JSON code | JSON `error` / displayed detail |
|---|---|---|---|
| Zero transcript rows | 400 | `invalid_request` | `No transcript content found` |
| Sanitized context empty, shorter than 50 or longer than 512000 UTF-16 units | 400 | `invalid_request` | `No transcript content available for chat` |

Current surface trace for Y assertions:

- Core status/text: `transgate/core/src/ai/mod.rs:104-108,117-125`.
- p1T route forwards admission errors at `transgate/http/src/ai.rs:120-132`; `:43-70` maps existing `Invalid` to `invalid_request` and preserves the text; `transgate/http/src/lib.rs:104-109` emits `{error, code}`.
- p21 projection: `transgate/core/src/presentation/ai/chat.rs:292-297,603-617` routes non-quota refusals to `ChatErrorVm.detail`, not a translated or invented message.
- Desktop `transgate/web/src/lib/features/ai/views/desktop/ChatPanel.svelte:140-149` and phone `views/mobile/ChatPanel.svelte:124-134` show the generic “Failed to get response” heading and the exact detail above.

Executed evidence:

- Read-only Node execution of the actual legacy `ContentProcessor` confirmed sanitized lengths 0 and 49 reject, 50 accepts. No source-repository writes or provider calls.
- New admission regressions first failed on the old analysis-style `NoTranscript` mapping: `chat_missing_rows_and_invalid_context_keep_distinct_source_errors` and `chat_minimum_uses_sanitized_utf16_units_and_accepts_fifty`.
- After repair, `cargo test --locked -p transgate-core --lib ai::app::tests` passed **22 tests**. Cases cover absent rows, sanitation-to-empty, 33-unit text, over-limit context, rejection without generation/quota/history/provider effects, and 49/50 UTF-16 boundaries with markup/non-BMP text; the accepted 50-unit case completes through the existing scripted worker and saves one chat pair.
- HTTP/browser assertions above are source-traced, not newly exercised by p1X. No candidate copy, host/client/view implementation, sanitizer, route parsing or post-sign-in decision was edited. p22 retains REPAIR-P5-3a diagnosis.

## REPAIR-P5-5: reattach a chat answer running in another tab

Root cause (real `AppState` diagnostic, `/tmp/rustr-p5-5-core-diagnosis-8tVKxK`): chat history returned only saved pairs, so an in-flight answer could not be represented, and the chat leaf never watched on load. Legacy Transgate never reattached either (`useChatViewModel.ts:63-99`, `ChatMessageRepository.ts:15-28`); this is the lead-approved P5 D6 correction (reload/layout switch without redispatch; a second tab is the same case).

Contract: `GET /api/ai/jobs/{job}/chat` serializes `transgate_core::ai::ChatHistory { messages: Vec<ChatMessage>, in_flight: Option<InFlightChat { question, generation: GenerationView }> }` directly (p1T route, no API wrapper).

- Core `Ai::chat_history` (`ai/app.rs`) reads consent, saved pairs and `AiTx::active_for_job(account, job, Chat)` in one read transaction. `in_flight` is offered only when `consent::may_write` holds for its admission epoch (work admitted under an earlier decision is fenced and never offered). `question` is the original accepted text (D5). Reading never dispatches, meters or extends work.
- Leaf `presentation/ai/chat.rs`: `HistoryLoaded { result: Result<ChatHistory, ApiFailure> }`. With `in_flight` and no request pending in this tab, it appends the user question and, when the draft has text, the held partial as an assistant line rendered `streaming = true`, then emits exactly one `Watch { generation, kind: Text, after: None }`. The attachment (`Request::Attach`) has no request key, so it is never started, replayed or metered; completion, failure, detach on job change and later retry follow the ordinary answer path. No view change (p21).
- Fixture `ChatPanelVm.reattached` (real transitions) regenerated in `fixtures.json` / `fixtures.generated.ts`; no generated type changed.

Executed evidence (p1X): four new regressions failed first on behavior (no watch, `in_flight: None`), then passed: `ai::app::tests::chat_history_reports_the_running_answer_until_its_pair_is_saved`, `presentation::ai::chat::tests::an_answer_running_in_another_tab_is_shown_and_watched_without_a_new_request`, `…::a_running_answer_without_text_yet_shows_typing_and_leaving_detaches`, `presentation::app::tests::a_second_tab_reattaches_a_running_chat_answer_without_starting_one`. Full `cargo test --locked -p transgate-core --features ts --lib`: 394 passed, generator included. p1T reports its real-PG route suite `ai_http_` 4/4 against this core (second-replica held partial, foreign 404, unauthenticated 401, completion → null). Client/wasm transport (p22) and browser C2 re-run (Y) are not claimed here.

## REPAIR-P5-5b: own-tab question erased by a late history load

Candidate6 original tab: start 200, one watch, provider finished, but no question, partial or answer rendered (welcome card, quota 1/2, no error). Root cause: the chat leaf accepted a question while its history load was still in flight; the late `HistoryLoaded` replaced `lines[]` under the pending request, leaving its answer index past the end, so frames never rendered while completion still refreshed limits. **The race predates REPAIR-P5-5** (the earlier leaf replaced lines identically); REPAIR-P5-5 only changed request timing/order (the original tab's GET chat now followed the analysis read). No second watch or reattach occurred: the own generation is never attached while a request is pending.

Fix (`presentation/ai/chat.rs`): as in the source (`DesktopChatView.tsx:142,307`, phone `MobileChatInput.tsx:73-109`), send and retry are refused and `can_send` is false while history loads; nothing is queued. History loads only on `Selected` (which resets requests) or a consent grant (after the refusal's reset), so a load never lands over a pending question, and a late duplicate is ignored once loaded.

Real-AppState regressions: `the_own_tab_question_keeps_its_lines_through_history_and_stream` (red first: the early send started), `history_landing_while_a_question_is_pending_keeps_both_lines` (late duplicate and own `in_flight`: no effects, lines kept, completes in place; already green before the fix through the loading guard, kept as the invariant), and the REPAIR-P5-5 second-tab test (still exactly one watch). Full `transgate-core --features ts --lib`: 396 passed. Generated change: `ChatPanelVm.loading_history.can_send = false`. p22's client regression `a_slow_history_never_erases_a_question_sent_after_it` encodes the same contract.
