# P4 persistence: bounded read-only review

2026-10-25. Reviewer: Astra, continuing the original advisory session. Requested slice only. No builds, tests, checks, provider calls, source edits, canonical changes or book changes. Findings are source-backed counterexamples, not reproduced failures. Core orchestration is mid-flight; no missing root integration or invented caller is reported as a defect.

## Verdict

**Bounded source review closed; integrated execution remains pending.** F1's hard-delete parity defect is owner-corrected. F2's missing rollover precondition is now explicitly published by p1X and supported by the supplied admission/limits source. Request-key conflict recovery now rolls back before a fresh read transaction. The separately known remote-field mismatch is owner-corrected. No unresolved source defect from this bounded pass remains; the regression/evidence obligations below are not a claim of runtime acceptance or a review of all unfinished orchestration.

Factual questions were sent directly to persistence owner `w23:p1R`; his answers are reflected below. No automatic implementation was requested. Lead owns any corrective dispatch.

## Scope and evidence

Read accepted `docs/plans/p4-ai/decisions.md` and `implementation-contract.md`; `transgate/core/src/ai/ports.rs`; all of `transgate/jobs-postgres/src/ai.rs`, `migrations/0004_ai.sql`, and `tests/ai.rs`; relevant `store.rs`, `uow.rs`, runtime-grant verification in `lib.rs`, base migration ownership/grants, host port preconditions, and `implementation/persistence.md`. One targeted original-source read confirmed the chat-limit hard-delete FK.

Initial reviewed snapshots: AI ports `643C`; adapter `29E3`; migration `DF6A`; AI tests `119E`; uow `DD4A`; store `273A`; grant verifier `089A`. These tool snapshot identifiers describe reads, not a built revision. After lead authorized the two owner fixes, a bounded source confirmation saw migration `B970`, adapter `FFE3`, tests `A6CD`, and grant verifier `E393`; see F1 and the final handoff. Other citations describe the initial reviewed slice, not a claim of a newly executed integrated build.

## Findings

### F1. Hard job deletion retained transcript quota contrary to source [P2, owner-corrected in source; unrun]

**Pre-correction evidence (initial snapshots above):**

- `transgate/jobs-postgres/migrations/0004_ai.sql:111-124` explicitly retains quota through job deletion; nullable `job_id` has no job FK.
- `src/uow.rs:331-337` deletes lines/job and relies on child cascades, explicitly leaving counters.
- `tests/ai.rs:434-475` pins the divergent behavior: after hard deletion, the transcript-token counter remains; lines 448-449 create it and 465 expects one quota row.
- Original `frontend/supabase/migrations/20251130113249_remote_schema.sql:4281-4282` has `chat_limits.job_id -> jobs.id ON DELETE CASCADE`. Account-level rows with NULL job are unaffected by a job deletion.
- Accepted D3 distinguishes consent cleanup from hard deletion; it authorizes retaining numeric quota through refusal/revoke, not retaining job-scoped counters after their job is hard-deleted. The persistence owner confirmed this was his interpretation, not a directed decision, and called it a source deviation.

**Original counterexample [INFERENCE, not executed]:** store a transcript counter for owned job J, hard-delete J, commit. The initial schema left an account-linked counter referencing nonexistent J; source removed it. Revoke retention had been extended to a different operation.

**Disposition:** owner confirmed the deviation and landed the lead-authorized correction. Current `0004_ai.sql:111-125` has `job_id REFERENCES transgate_jobs.jobs(id) ON DELETE CASCADE`; account-wide NULL-job counters stay. Current `tests/ai.rs:434-480` asserts transcript counter removal and unchanged account counter. This closes the source defect, not runtime acceptance: no test was run here. Preserve both counter scopes through refusal/revoke, and remove all on account erasure. No new dispatch is needed for F1.

### F2. Original-period guarantee required an explicit single-bucket invariant [source-contract issue resolved; runtime evidence pending]

**Initial concern:** D2 requires original-period settlement, but `ports.rs:165-201`, `0004_ai.sql:113-124`, and `ai.rs:408-450` expose only one mutable bucket per counter key. Resetting that bucket during an active reservation would discard its original period; matching `reservation.period_end` and skipping a different current period would not recover the spend. At initial review p1R had no published invariant forbidding that reset. This was a seam gap, not a claim that a real limits endpoint performed it.

**p1X's resolution, source-confirmed:** `transgate/core/src/ai/quota.rs:15-22` now publishes the single-bucket invariant. Rollover occurs only within admission; admission returns Busy before counter locking/refresh while an active metered generation exists; limits reads project current usage without writing; settlement/release retains the original bucket until the reservation ends. Persistence must not independently reset or recreate counters.

Targeted confirmation of the supplied closure evidence (`quota.rs#7308`, `app.rs#C0D8`):

- `app.rs:399-418`: limits retrieves counters and projects with `quota::current`; no counter write.
- `app.rs:565-583`: active-metered check returns Busy before locking counters, `quota::admit` and saving refreshed counters.
- Previously reviewed PG methods use plain SELECT for counters and FOR UPDATE for lock_counter, under the caller's account lock. The partial unique active-metered index is the backstop; no automatic rollover trigger/get-or-create exists in the reviewed migration/read implementation.

The former counterexample—reserve before midnight, reset the stored bucket through a metadata transaction after midnight, then settle—now violates this published invariant at its reset step. One current bucket therefore suffices for the supplied lifecycle; no historical-bucket schema or new quota policy is requested.

**Execution still pending:** p1X reports core regression `ai::app::tests::an_answer_settled_after_midnight_lands_in_its_admission_day`: projected limits show zero after midnight without touching storage, concurrent admission is Busy, completion settles the old bucket, and the next admission starts the new period. p1R subsequently reports the real-row PG regression `transgate/jobs-postgres/tests/ai.rs::an_answer_settled_after_midnight_lands_in_its_admission_day_across_two_processes`, explicitly unrun. This reviewer did not inspect or execute either new regression. Their existence is owner-reported, not a passing result or independent confirmation of process topology. The source-confirmed invariant closes the contract question; real-PG execution remains lead-owned.

## Contract precision, not an additional proven defect

### Request-key conflicts require transaction recovery before rereading

`ai.rs:293-329` uses ordinary INSERT; unique violation maps to `StoreError::Conflict` through `lib.rs:36-45`, leaving that transaction aborted. The original port wording only said “caller re-reads”; `tests/ai.rs:259-265` already rolled back and started a new transaction.

**Resolved in supplied caller source:** `app.rs:596-602,616-627` explicitly maps the conflict, rolls back, then uses a fresh read transaction. Same-digest request replays; another digest conflicts; absent request plus active metered generation returns Busy. No read is attempted on the aborted transaction. This confirms the requested recovery precision; it is not an executed collision test or a broader review of that caller's authorization.

p1R subsequently reports the same rollback/fresh-read protocol is now recorded in `docs/plans/p4-ai/implementation/persistence.md`. Documentation update is owner-reported; no additional source audit or execution was performed for this notification.

## Authority and fence assessment

| Area | What current source provides | What must remain caller/integration-owned |
|---|---|---|
| Shared transaction / lock domain | `uow.rs:27-37,114-122`: one READ COMMITTED transaction; `ai()` returns self without another connection/allocation; billing attaches to that connection. `lock_generation`, `lock_counter`, `lock_summary_request` use FOR UPDATE. | Account-first locking, fresh barrier reads, owned-job validation and ordering across participating rows. No network inside the transaction. |
| Account/job ownership | Account-scoped lookups use account predicates (`generation_by_request`, active/latest queries, chat_history, counters, completion_summary). `JobUow::lock_jobs` filters actual job owner (`uow.rs:154-170`). | Raw generation reads/updates are by id; insert foreign keys are single-column existence constraints, not `(account,job)` ownership constraints. Owner confirms core owns ownership checks. Do NOT expose AiTx directly to session callers or treat knowledge of a UUID as authorization. This is not an endpoint exploit finding without a violating caller. |
| Consent and execution fences | Whole consent row with epoch/cutoff/pending persisted; immutable generation identity/input/source/epoch columns are not runtime-updatable. Generation saves are UPDATE-only and fail if gone (`ai.rs:345-375`). | `save_generation` WHERE clause is id only; the caller must lock/reread/check account, current consent, source revision, lease/execution token before modifying it. Refusal/regrant read rules differ from exact-epoch active writes. Lack of repeated policy SQL inside this low-level port is not itself a defect. |
| DELETE cutoff / legacy | `ai.rs:678-700` deletes all three present artifact families by account AND `(epoch IS NULL OR epoch <= cutoff)`, including draft/staged/input carried by generations. Nullable epochs decode to 0. Newer rows survive the direct predicates; counters/summary requests remain. | Commit fence/pending obligation before purge; maintain cutoff across regrant, release/fence any active reservation appropriately, block covered content while deletion is pending. Current port explicitly defers not-yet-added legacy import/callback tables to P9/P7; no imaginary missing table is reported here, but those obligations remain. |
| Metered serialization / paid | Partial unique index `0004_ai.sql:63-65` backstops one nonterminal quota-bearing generation/account. Account-lock query sees it. `quota=NULL` can bypass this lane without creating counters. NULLS NOT DISTINCT intentionally prevents duplicate user counter keys. | Choosing billing facet, true paid/all-caps-disabled bypass, admitted entitlement/caps, period reset and exact successful estimates are core policy. Creating an unmetered fixture does not prove billing-selected paid bypass. |
| Staging / atomic result | Generation staged final, pair inserts, counter saves and status save share one transaction. Chat pair `(generation,role)` uniqueness blocks duplicate pairs; pair reader and persisted generation support reconciliation. | Unknown-COMMIT classification/reconciliation, no double counter mutation, exact settlement, and no provider replay. Read actual committed state before release or retry. |
| Retention / hard delete / erasure | Line deletion does not delete AI tables (`uow.rs:689-698`). Job FKs cascade source revision/generation/chat/summary/request; the owner correction now also cascades transcript quota. Account erasure explicitly deletes AI tables, counters and consent (`uow.rs:970-991`). | Content-revision bump and running-work fences belong to the mid-flight core integration. External identity/uncertainty must be captured in the erasure manifest before local rows vanish. F1's correction is source-confirmed, unexecuted. |
| Runtime authority | Migration grants SELECT/INSERT/DELETE only to the existing service group, narrowly scoped UPDATE for generation mutable fields; artifact content and immutable fences cannot be UPDATEd. `lib.rs:69-111,184-202` adds tables to the verifier; `tests/ai.rs:177-194` exercises forbidden grants. | This is server service authority, not per-user database RLS. The role deliberately has cross-account storage access and can INSERT/DELETE; immutable UPDATE grants are a guardrail, not proof of authorization against a malicious service process. No second identity system or broad DB redesign recommended. |

## Substantive regression gaps / evidence boundaries

These are obligations for coherent integration, not orders to the persistence owner to implement callers mid-flight. The persistence handoff already scopes quota/consent policy out of its own suite.

1. **F1 regression is now corrected in source, not executed.** It distinguishes transcript counter removal from account counter preservation. Refusal/revoke and account erasure retain their separate dispositions. The hard-delete fixture uses an unmetered generation, so it does not cover resolution of an active generation's user-scope reservation before deleting its identity.
2. **Original-period real-PG execution remains pending.** The initial PG suite only upserted within one unchanged reset period. Both owners now report midnight regressions, including p1R's real-row test named in F2, explicitly unrun. No missing-test claim remains for that named scenario; no pass or independent assessment of the newly added test is claimed. Lead-owned execution must establish the reserve/projected-read/Busy/old-period-settlement/new-admission sequence on real rows.
3. **Unknown-COMMIT test is storage reconciliation, not a lost-COMMIT reproduction.** `tests/ai.rs:298-326` calls successful `commit().await.unwrap()`, then reads state and rejects duplicate pair. It does not lose a COMMIT response or exercise the orchestrator's unknown-commit branch; no provider call is involved. `:338-354` meaningfully checks staged final survives an explicitly rolled-back completion, but is not the uncertain commit branch either. Retain these valid storage checks; add coherent boundary evidence before claiming unknown-COMMIT safety. In particular reconcile before retrying counter update even if pair save ultimately conflicts.
4. **No complete consent race or caller-boundary evidence yet.** `:381-430` correctly covers old/current/new epochs and NULL generation/chat rows, but uses direct port operations; it neither authorizes reads under false/regrant nor injects purge failure after a durable fence, nor races stale workers against regrant/cleanup. It does not verify caller-side source/execution checks. Integration must exercise false->grant retained read, DELETE after multiple cycles/legacy copies, newer-write survival, stale/foreign owner refusal and no resurrection after erasure. Do not repurpose an unauthenticated raw port call as proof of those failures.
5. **Paid bypass is not proven by this suite.** Directly creating `quota=None` does not prove the real freemium facet chooses bypass or that unavailable quota storage is untouched. Exercise the actual core operation with paid eligibility, exhausted/poisoned quota operations, future-start/unexpired package and a successful answer. All-caps-disabled free is a separate case.
6. **Concurrency test scope is precise.** `:198-246` uses two pools/database connections, observes a real PostgreSQL lock wait, then tests the partial index. This is useful serialization source, but not an actual multi-host dispatch/provider acceptance run. It does not itself cover concurrent starts on different jobs, original-period rollover, revoke or settlement rollback.

No proposed new test is a constant/helper echo. No execution or pass result is claimed in this review.

## Owner answers and handoff limits

p1R confirmed:

- Hard-delete transcript counter retention was his interpretation, not directed; original source cascades it. He has now landed the lead-authorized cascade and corrected regression (F1). NULLS NOT DISTINCT user-counter uniqueness is intentional, unlike source's duplicate-NULL behavior.
- At initial review quota stored one mutable period row without a published defer-rollover invariant. p1X subsequently supplied the explicit admission-only rollover/read-only projection invariant; F2 records the bounded source confirmation and closes that contract issue.
- Ownership/epoch/source/execution checks are caller-owned; there is no composite ownership FK. Added database backstops are the request-key and active-metered unique indexes, ordinary existence FKs, column grants and pair-consistency check.
- Unique-violation recovery needs a new transaction or caller savepoint. p1X subsequently supplied the fresh-transaction conflict path, confirmed above; this is no longer an unresolved precision item.
- **Known remote-field seam now owner-corrected:** initially `ports.rs:232` had `remote_id: Option<String>` while PG used a remote JSON identity. Owner reports adapter, migration, UPDATE grant, verifier and fixtures now aligned, with no alias left. Targeted source confirmation saw `remote_id text` in migration, `remote_id` decoding/binding in adapter and the corresponding verifier allow-list. No build was run; this is not an unresolved missing-root or compilation finding.

**Next owner: lead.** All source issues raised by this bounded pass are now owner-corrected or source-contract-resolved. F1, remote-field alignment, F2's invariant and request-key recovery still require lead-owned integrated execution; no fresh implementation dispatch is requested here. Preserve the evidence boundaries for unknown-COMMIT, consent races, paid bypass and real-PG rollover. This bounded review and its owner-supplied closure update end here; no broad audit or continuous polling follows.
