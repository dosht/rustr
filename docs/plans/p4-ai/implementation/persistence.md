# P4 persistence: handoff

Owner: w23:p1R (Opus). Slice: `transgate/jobs-postgres` AI migrations, runtime grants and authority checks, and the `AiTx` implementation on the same `JobUow` connection and transaction, plus new AI tests. Not touched: billing, core ports, P3 `tests/store.rs` and `tests/store/` (p1Z).

## Status

Source-ready against p1X's `transgate/core/src/ai/ports.rs` (2026-10-25). It compiles only once p1X declares `ai` in `transgate/core/src/lib.rs` and adds `JobUow::ai` plus the three `JobQueries` reads to `jobs/ports.rs` (his integration point). Nothing built, formatted or run; no provider calls.

## Changed files

- `migrations/0006_dispatch_unkept_evidence.sql`: the `unkept_id_observed` column, its CHECK and grant, and explicit `false` on existing `llm_dispatch` erasure targets. 0005 was executed and is unchanged.
- `migrations/0005_llm_dispatch.sql`: `llm_dispatches` (payload-free D3/D9 ledger, no foreign keys, `remote_ids text[]` bounded to 8, evidence check) and `UPDATE (target)` on `erasure_items` for late dispatch identity.
- `migrations/0004_ai.sql`: `ai_consent`, `source_revisions`, `generations`, `chat_messages`, `completion_summaries`, `summary_requests`, `quota_counters`, with column-level runtime grants to `billing_runtime`. 0001..0003 untouched.
- `src/ai.rs` (new): `impl AiTx for PgJobUow`, every method on the uow's own connection.
- `src/uow.rs`: `JobUow::ai` returns `self` (no allocation, same transaction); `erase_account_rows` deletes chat, completion summaries, summary requests, generations, quota counters and consent; `hard_delete_job` relies on `ON DELETE CASCADE` from `jobs` for every AI child, transcript-scoped quota counters included; account-wide counters stay. `conn` is crate-visible.
- `src/store.rs`: `JobQueries::{runnable_generations, pending_summary_requests, pending_ai_purges}`.
- `src/lib.rs`: `mod ai`; the runtime allow-lists cover the new tables.
- `src/rows.rs`: `get` is crate-visible.
- `tests/ai.rs` (new).

## Persistence rules

- The caller locks the billing account first; nothing in `AiTx` locks the account or a job or opens network I/O. `lock_generation`, `lock_counter` and `lock_summary_request` are `FOR UPDATE`.
- Idempotency: unique `(account_id, request_key)`; a duplicate insert is `StoreError::Conflict`. `save_generation` and `save_summary_request` never insert and return `Conflict` for a missing row.
- D2: a partial unique index allows at most one `accepted`/`running` generation with a quota reservation per account, as a database backstop to `active_metered` under the account lock.
- Generations keep one draft, an optional staged final, the execution token and lease (both or neither), the frozen reservation and the interruption as JSON, and the provider completion id as `remote_id text` (matching `Generation.remote_id: Option<String>`). There is no token or event log.
- Chat pairs: orders follow the source `get_next_message_order` rule (`COALESCE(max + 1, 1)` per job, then +1 for the answer); unique `(generation_id, role)` makes a second pair a `Conflict`; `chat_pair` is the unknown-COMMIT reconcile key. Chat messages and completion summaries are never updated by the runtime.
- Completion summaries: one per `(job, source_revision)`, an existing one is kept. Summary requests: one per `(job, source_revision)`, content-free.
- Consent: one row per account, upserted whole. `consent_epoch` columns are nullable for legacy imports; a NULL reads as 0, and a DELETE purge removes `IS NULL OR <= cutoff`.
- `purge_ai_artifacts` deletes chat messages, completion summaries and generations (drafts and staged finals included). Summary requests and quota counters stay. Per p1X, P9 legacy imports and P7 callback copies must join this purge when they add their tables.
- Quota counters are unique on `(account, scope, job, kind, period)` with `NULLS NOT DISTINCT` (PostgreSQL 15 or later), so user-scope rows never duplicate. They survive refusal, revocation and purge. Transcript-scoped rows reference their job with `ON DELETE CASCADE` and go with a hard job deletion, as source `chat_limits.job_id` did (`20251130113249_remote_schema.sql:4282`); a transcript counter for a missing job is a `Conflict`. Account-wide rows stay. Account erasure deletes all counters with consent.
- `bump_source_revision` is an upsert keyed by job with a foreign key to `jobs`, so a missing job is a `Conflict`. `bump_source_revisions` advances every listed job once in one statement (repeated ids are collapsed); one missing job fails the whole statement with `Conflict`.
- `Generation.tokens_used` is a nullable `bigint` written by insert and save.
- `runnable_generations` includes barrier accounts (the worker interrupts them as erased).
- LLM dispatch ledger (D3/D9): one row per generation keyed by `generation_id`, no foreign key to jobs, generations or accounts, so consent DELETE purge and hard job deletion keep it. `record_dispatch` keeps an identical row and returns `Conflict` for another account, provider or model. `settle_dispatch` is `LlmDispatch::settle` in one statement (new, nonempty, at most 256-byte ids appended first-seen up to 8; `not_dispatched` only while no id is stored and no nonempty id was observed in that settle; durable unkept evidence: migration 0006 adds `unkept_id_observed` (default false, CHECK possible-or-no-evidence, UPDATE grant, existing `llm_dispatch` manifest targets set to an explicit false) and `record_dispatch`/`account_resources` map it; `settle_dispatch` sets it for a new nonempty id it cannot keep, never stores a truncated id, and refuses `not_dispatched` while it is set). The before-fix overflow behavior was never observed in a run: the two capture attempts did not compile, matching account and generation, `false` when the row is gone. `account_resources` returns the rows; `erase_account_rows` deletes them after the manifest captured them. `save_erasure_item_target` rewrites target, state and attempts only when the stored and the new target are both `llm_dispatch`, otherwise `Conflict`.
- F2 original period (p1X's invariant in `ai/quota.rs`): a counter rolls over only inside an admission, after `active_metered` returned none. PG keeps it: no trigger, no read-path create or reset (`counters` is a plain SELECT), `lock_counter` is `FOR UPDATE` under the account lock, and the partial unique index backs one active metered generation.
- A `Conflict` from `insert_generation` aborts the PostgreSQL transaction; core rolls back and re-reads in a fresh transaction (p1X's protocol).

## Tests

`tests/ai.rs`, 16 tests, all `#[ignore]`d without a database:

```sh
TRANSGATE_JOBS_TEST_DATABASE_URL=postgres://... cargo test -p transgate-jobs-postgres --test ai -- --include-ignored
```

- `the_runtime_cannot_rewrite_a_generations_fences_or_an_artifact_after_it_is_written`
- `two_processes_admitting_metered_chat_for_one_account_admit_exactly_one` (two pools, the second waits on the account lock, then sees the first as busy; the partial unique index refuses a direct second insert)
- `a_request_key_is_unique_per_account_and_returns_the_original_generation`
- `a_committed_pair_and_settlement_are_readable_for_reconciliation_and_never_written_twice` (adapter scope: reads and unique keys)
- `ai_completes_once_when_the_completing_commit_answer_is_lost` (real `Ai` over `PgJobStore` with the test-only `tests/ai_support/uncertain_commit.rs` wrapper: the first transaction holding the armed generation's chat pair is committed, or rolled back, then reported as a failed COMMIT; asserts one pair, one settlement, no held reservation, one provider call, and no replay from a restarted app)
- `a_staged_final_survives_a_rolled_back_completion_for_local_reconciliation`
- `user_counters_are_one_row_per_key_and_transcript_counters_are_per_job`
- `a_delete_purge_removes_every_artifact_at_or_below_the_cutoff_including_legacy_and_keeps_newer_ones`
- `the_source_revision_advances_per_job_and_a_hard_delete_takes_every_ai_child_and_transcript_quota_but_not_account_quota`
- `account_erasure_removes_every_ai_row_including_quota_and_consent`
- `the_summary_producer_queue_is_idempotent_and_a_decided_request_leaves_it`
- `runnable_generations_are_accepted_or_running_with_an_expired_lease`
- `retention_advances_every_listed_jobs_revision_once_in_one_statement` (also round-trips `tokens_used`)
- `the_dispatch_ledger_survives_purge_and_hard_delete_and_goes_only_with_erasure`
- `dispatch_evidence_is_appended_distinct_and_bounded_and_never_cleared`
- `only_an_llm_dispatch_erasure_item_can_have_its_target_rewritten`
- `an_answer_settled_after_midnight_lands_in_its_admission_day_across_two_processes`: two real `Ai` apps on separate pools and a shared clock; admission one minute before UTC midnight, a limits read and a competing admission from the other app after midnight (zero shown, row untouched, `Busy`), completion settles into the admission day, the next admission starts the new day. Needs the new dev-dependency `llm-core` (lockfile refresh by the lead).

## Not covered here

Quota policy (paid bypass, period rollover, check precedence), consent epoch decisions and late-write fences are core behavior over these ports; their two-process and host evidence belongs to the policy and host owners' suites on top of this store. Source tables used as the parity reference: `ai_consent`, `chat_limits`, `chat_messages`, `summaries`, `ai_analysis` (frontend Supabase migrations, read-only).
