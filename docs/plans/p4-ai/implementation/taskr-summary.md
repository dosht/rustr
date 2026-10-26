# P4 taskr audio-work summary host: review repairs handoff

Owner: p1Z (Opus), 2026-10-25. p1T relinquished the taskr summary files and keeps the Transgate host. This note covers the repair source written after lead's RED capture (artifact657: 15 tests, 7 passed, 8 failed). **Source only:** nothing has been built, tested or formatted since, and no provider was called. Decisions D4 and D10 apply, along with `../opus/taskr-summary-review.md`. The published DTOs in `hosts.md` are unchanged.

## Observable contract (agreed with p22)

- **Watch admission.** Watch attachments are capped at 32 per process and 4 per account. A watch over the cap is refused immediately with `429 {"error":{"code":"watch_limit","message":"Too many live summary views are attached. Close one, then resume."}}`. Nothing is queued. The 401/404/410 checks run first.
  - The slot permit belongs to the response body and is released when the body is dropped: on disconnect, at a terminal frame, or at a Revoked/Interrupted control frame.
  - An idle attachment writes `: keep-alive` (an SSE comment) every 5 s, so hyper notices a closed client. `SseDecoder` ignores the comment, and no sequence number moves.
- **Unlocked polling.** Watch and driver fences poll every 250 ms. An unchanged poll is one unlocked statement that reads the barrier, enrollment, ownership, source and generation (`Store::poll_summary_generation`). Only work that really needs reconciling takes the account and row locks: a source fence, a staged final or an expired lease. `GET …/generations/{id}` uses the same read.
- **Replay before new-admission checks.** Order: authenticate, then owner/barrier, then look up an existing request id. A replay returns 200 with the same snapshot even when the disclosure flag is false or the LLM is not configured. A new id still needs the disclosure acknowledgment (400 `invalid`) and configuration (503 `not_configured`). A host without an LLM never claims or drives a generation.
- **Claim-relative lease.** An accepted generation must be claimed within 65 s of admission. The first successful claim sets `lease_until = claim + 60 s provider deadline + 5 s settlement`, in both the payload and the `lease_until_us` column. Nothing is claimed or dispatched twice.
- **Quiet open.** One `Llm::open` future survives every fence tick. A cancel or barrier committed on any replica drops it within one poll and never opens a second request.
- **Provider ids (M1/M2).** Each generation keeps up to 8 distinct ids of at most 256 bytes, first seen first, so the header id and body id both survive. The same rule applies to the post-cascade `llm` erasure resource (`target.remote_ids`), which is seeded from the generation row so a known id is never replaced.
  - An id that does not fit interrupts with its own reason: "The provider request identity exceeds its byte limit." / "…more request identities than can be retained." It never gets storage advice. Known ids are kept and nothing is truncated.
  - Ids observed in memory are merged into the interruption transaction (or the erasure resource under the barrier) on a best-effort basis. A database or process failure can still lose them, and nothing claims otherwise.
  - Only a request with no observed id can be marked never sent.
- **Cancel (D10).** A cancel wins over a staged final that was not settled. A completed generation is canonical and is returned unchanged.
- **Retention (D10).** `Tx::save_work` of a completed work without its transcript calls `Tx::purge_summary_source_content` in the same transaction. It clears the text and staged final of every non-completed generation, interrupts active ones ("The source transcript was removed by retention.") and bumps the sequence. Terminal reasons, provider facts, remote ids, completed generations and saved summaries are kept. The snapshot keeps its generation_id/execution_id with `text: ""`.

## Source

- `crates/taskr-work/src/summary/domain.rs`:
  - Constants: `MAX_REMOTE_IDS`, `MAX_REMOTE_ID_BYTES`, `PROVIDER_DEADLINE_US`, `SETTLE_MARGIN_US`, `ADMISSION_LEASE_US`.
  - `RemoteIdLimit`, `merge_remote_ids`, `Generation::remote_ids: Vec<String>` (required; there is no serde default).
  - The claim lease, the cancel rule and `purge_source_content`.
- `src/store/summary.rs`:
  - New: `Store::replay_summary` and `poll_summary_generation`.
  - Changed signature: `interrupt_summary(&Generation, why, observed: &[String], now)`.
  - `record_summary_remote` accumulates ids (signature unchanged).
  - `Tx::summary_erased_remote`, `Tx::purge_summary_source_content`, and `summary_save` now also writes `lease_until_us`.
- `src/summary/app.rs`:
  - `FENCE_POLL`, `WATCHES_PER_PROCESS`, `WATCHES_PER_ACCOUNT`.
  - `SummaryService::admit_watch -> Option<WatchPermit>`.
  - Replay-first `start`; the driver's quiet-open fence and truthful interruption reasons. `SummaryService::new` is unchanged.
- `src/summary/http.rs`: watch admission, 250 ms unlocked polling, heartbeat. `SummaryHttp` is unchanged.
- `src/store.rs` (coordinated with T): the `save_work` retention hook, and the erasure capture now copies `remote_ids`.
- New migration `migrations/0003_summary_remote_ids.sql`: converts 0002-era scalar `remote_id` on generations and `llm` resources into `remote_ids`. It is idempotent and keeps every known id. The 0002 migration is not edited.

## Tests (lead executes)

`TASKR_WORK_TEST_DATABASE_URL=<owned disposable> cargo test --locked -p taskr-work summary:: -- --ignored`, plus pure `cargo test --locked -p taskr-work summary::domain`.

- **RED-captured (artifact657), now expected to pass:**
  - `pg_tests::the_first_claim_starts_a_full_execution_lease_and_never_dispatches_twice`
  - `pg_tests::transcript_retention_clears_noncanonical_partials_and_keeps_reasons_provider_facts_and_the_saved_summary`
  - `pg_tests::distinct_provider_ids_accumulate_before_and_after_the_erasure_cascade`
  - `pg_tests::driver::{a_quiet_open_observes_another_replicas_cancel_and_erasure_without_a_second_dispatch, distinct_header_and_body_provider_ids_survive_completion_replay_and_erasure, an_oversized_provider_identity_interrupts_with_its_own_reason_not_storage_advice, an_existing_request_replays_before_disclosure_or_configuration_and_ownership_comes_first}`
  - `http::tests::attachments_are_refused_immediately_past_four_per_account_and_thirty_two_per_process`
- **Replaced false positive:** `http::tests::unchanged_authorized_polls_traverse_the_database_under_held_account_and_work_locks_then_see_session_loss`. It actively reads the body while the account and work locks are held. A counted, notified authentication handshake proves that two unchanged authorized polls finished their DB read before revocation. There is no sleep; the timeout only bounds failure. Against the pre-repair source it is expected to fail: the first poll blocks on the account lock.
- **Guards:**
  - `pg_tests::cancel_wins_over_a_noncanonical_staged_final_but_never_over_a_saved_summary`
  - `http::tests::a_watch_reconciles_a_real_staged_final_and_stops_at_a_fresh_barrier`
- **New with this API:**
  - `pg_tests::identities_observed_but_not_recorded_are_carried_into_the_interruption_or_the_erasure_manifest` (M2)
  - `pg_tests::migration_0003_converts_the_0002_scalar_provider_id_without_losing_it`
  - `summary::domain::tests::{provider_ids_stay_distinct_first_seen_and_bounded_without_dropping_known_ones, the_execution_lease_starts_at_the_first_claim_however_late_it_was_admitted, retention_clears_noncanonical_text_and_keeps_the_reason_provider_facts_and_completed_summaries}`
- **Existing test updated:** the erasure late-metadata test now asserts `target.remote_ids`.

## Remaining

The earlier actual-surface proof stays recorded: taskr browser 4/4 and the real Linux Swift UniFFI listener passed. That was before these repairs and the lint cleanup, so it is not current-binary green. A rerun after cleanup is still a lead obligation. Nothing here adds a per-account generation quota. Synthetic fixtures do not prove live provider retention or deletion.
