# P3 acceptance review (Opus, w23:p1Z)

Read-only review of source and evidence against `implementation-contract.md`, `decisions.md`, and `astra/convergence-contract.md` (clauses C1–C12), using the discovery ledgers in `transgate/docs/discovery/`. I ran no builds, tests, or checks. The review itself edited no source; the F9 transport seam was added afterwards, as separately assigned work. The snapshot is the working tree on 2026-10-24. Owners were editing while I reviewed (history retention, retention default). Every reference below was re-read after their latest messages.

**Evidence labels:** **E** means the lead ran it and it passed at that snapshot (`integration-notes.md`, lead brief). **S** means source only, never executed. A fixture is not live provider acceptance.

**Lead updates after this review:**
- Passed: processor (40 tests), billing (244), media-postgres (5), and the taskr actual-process smoke.
- The Transgate archive failure (F2) is real, not only a source reading.
- F9 is now an owned P3 implementation item; see `opus/legacy-inventory-evidence.md`.

## 1. New findings (not the three known billing-erasure issues)

| # | Sev | Clause | Defect | Source | Consumer counterexample | Owner and narrow repair |
|---|---|---|---|---|---|---|
| F1 | high | C10 | The erasure manifest drops issued uploads whose PUT capability has already expired. A hard-deleted draft leaves its uploaded object (JOB-038), and that object is then only listed in `issued_uploads`. | `core/src/privacy/erasure.rs:175-179` emits `UploadResweep` only `if expires_at > now`. `jobs-postgres/src/uow.rs:835-843` takes media only from surviving job rows. | User uploads an audio file, removes the draft, and waits more than 15 min before deleting the account. The saga reaches `FullyErased`, but the GCS/R2 object stays forever. | p1X: emit a delete/HEAD item for every issued upload. Use `not_before = max(expires_at, now)`. Add a regression that erases after expiry and checks the object. |
| F2 | high, **observed real by lead** | C10 (diagnostic body, archival honors barrier) | The provider-payload archive is written after the result commit. The PUT happens first and `record_archived_payload` runs afterwards. That record refuses with `Conflict` once the job row is gone, so the object is never tracked. | `core/src/jobs/app.rs:1579-1583`, `1644-1669`; `jobs-postgres/src/uow.rs:288-299` | A Soniox result commits. The user requests erasure and the cascade runs before the archive PUT finishes. The PUT then lands `<account>/<job>/soniox_transcript.json` (the full transcript). The record fails, the manifest never sees the object, and the saga reports `FullyErased`. A DB outage between PUT and record orphans the object the same way, even without erasure. | p1X with p1R: record the archive key inside the apply transaction, before commit. PUT after commit. Erasure then deletes the known key (absent counts as confirmed). |
| F3 | medium | C5, C10 (late-bound identity) | Remote identities that arrive late are dropped. **(a)** `Attempt::accept` returns an error for `Superseded`, so a late acknowledgement rolls back and loses the remote id. The cleanup branch at `app.rs:1158-1163` is unreachable for this state. The claim lease is 2 min (`domain.rs:702`), which URL staging plus the 60 s request timeout can exceed (`app.rs:1123-1134`, `media-adapters/src/http.rs:17`). **(b)** The apply `Ignore` path marks cleanup on the *bound* attempt and never records `observation.remote.id`. For an attempt with no remote id, `delete_remote_copy` is then a no-op (`app.rs:1542-1548`, `1675`). **(c)** A late acknowledgement after the barrier fails on the billing lock and drops the id. Taskr already handles all three (`record_late_remote`, `learned`). | `core/src/jobs/domain.rs:735-745`; `core/src/jobs/app.rs:1141-1201`, `1525-1548` | The claim lease expires during staging, a second claim submits, and the first POST then answers Accepted with remote X. X is never recorded or deleted, the provider bills both, and erasure can only find X through a Soniox listing (never for Deepgram). | p1X: on a stale or superseded acknowledgement or callback, record the authenticated remote id when the attempt has none, and set cleanup on *that* id. p1R adds a second identity column only if one attempt must hold two ids. Regression: lease expiry, then second claim, then late Accepted, and check the first id is deleted. |
| F4 | medium | C4, seam 5 | Lock order is inverted. These paths take job `FOR UPDATE` before the billing account row: `wait`, `fail_job`, claim, `acknowledge`, `supersede`, `with_job` (`app.rs:880, 907, 951, 1145, 1315, 1753`). These paths lock account → job: `apply`, `fail_ingestion`, `start`, `delete_jobs`, `edit` (`1461, 1515, 594, 1926, 1961`). `lock_job` is `FOR UPDATE` (`uow.rs:171`) and `lock_account` is `FOR UPDATE` on `billing.accounts` (`modules/billing/postgres/src/tx.rs:276`). | as listed | A callback-before-ack race (W3: apply against acknowledge on the same job), or user delete against `stuck_pass`, deadlocks. PG aborts one side with 40P01, which maps to Unavailable, so the user's request fails or the acknowledgement is lost until reconcile. | p1X: read the account unlocked, lock the account, then lock the job and recheck ownership. Add a real-PG two-connection regression. |
| F5 | medium | C5 | Soniox reconciliation deviates from the contract. **(a)** It matches only the reference plus the substring `attempt=<id>` in the callback URL, not the submitted audio URL or creation time. **(b)** A listing error is treated the same as no match. **(c)** After 30 min it supersedes and redispatches, even with zero or several matches, where the contract says the attempt stays unresolved. Taskr follows the contract. | `core/src/jobs/app.rs:1277-1309` | A Soniox response is lost and the listing is down or capped. After 30 min a second paid Soniox transcription is created. | p1X: apply the exact taskr predicate, and give up only for providers without a listing. The other path needs a recorded decision. |
| F6 | low | seam 6 | A conflicting callback payload gets the same 200 as a replay, and the conflicting body is not kept. Taskr answers 409 and holds it. | `http/src/lib.rs:362-364`; `core/src/jobs/app.rs:1374-1378` | Soniox sends an `error` webhook, then a `completed` one for the same id. The second is silently acknowledged, and the job stays failed with no trace. | p1T: respond 409. p1X: keep a durable conflict marker. |
| F7 | low | C10 signing | `streaming_url` checks the barrier without a lock and then signs. Nothing rechecks the barrier after signing. | `core/src/jobs/app.rs:2027-2045` | Erasure commits between the guard and the sign, and a 900 s GET URL is issued for audio under erasure. | p1X: re-read the barrier after signing, before returning. |
| F8 | low | seam 8 | The workers claim slots but take no overlap advisory lock, though `schedule.rs:1-4` says the host does. The Jobs slot (180 s) is shorter than its 600 s lease, so the passes on two replicas can overlap. Rows stay fenced, but the Deepgram 5-per-pass budget doubles. | `server/src/workers.rs:61-69`, `28` | Two replicas run a 4-minute Jobs pass, and Deepgram gets up to 10 submissions per 3 min. | p1T: `try_lock(schedule.name())` around each pass. |
| F9 | high, P3 (not a P9 waiver) | C10, D2 | Legacy Supabase `jobs/{user}/{job}/*` objects of imported jobs exist without rows, and erasure never inventoried them. | `core/src/privacy/erasure.rs:152-180` | An imported account's legacy archives survive a `FullyErased`. | **In progress.** p1Z: `ObjectInventory` port and `LegacySupabaseStorage` implementation (source, unrun). p1W: `ItemTarget::LegacyAccountInventory`. p1X: `run_item`. p1T: host wiring. P9 still proves the real inventory. |

## 2. Lead concerns

**History retention (PRV-025, PRV-031, CRN-007, C14 adapted, C11).** This is a P3 obligation: C11 requires weekly history with the source's extra-day arithmetic. The keep-throw "pg_cron statement" route does not apply, because the rewrite has no pg_cron and its run history is `media.schedule_slots`, about 620 rows a day from the 3-minute and 10-minute schedules.

The gap is now closed in source:
- `Privacy::history_retention_run` (`core/src/privacy/app.rs:185-235`): uses the `cleanup_cron_job_history` lock, the 100000 threshold, and an audit row committed with the delete.
- `JobUow::history_count` and `delete_history` (`jobs-postgres/src/uow.rs:684-698`), backed by `media_postgres::{count,delete}_finished_slots`. These delete only finished rows of `Schedule::ALL` names, `started_at <= now-91d`.
- Migration `0002_slot_history.sql`: an RLS DELETE policy allowing finished rows only, plus an authority check update.
- The host loop runs `Schedule::ALL` (`server/src/workers.rs:61`).

Regression `the_history_purge_and_its_audit_row_commit_together_and_interrupted_runs_become_failures` exists. All of it is **S**. Still needed: a run on real PG, and a binary smoke that claims a past Sunday slot or calls the use case. Minor: `HISTORY_TABLE = "schedule_runs"` (`retention.rs:60`) names a table that does not exist. The actual table is `media.schedule_slots`, so the audit row's `table_name` is wrong.

**Interrupted runs.** Repaired in source by `fail_interrupted_runs` under the job lock (`app.rs:105-117`, `uow.rs:669`). **S**.

**Retention default (PRV-013, 014, 016, 022).** There is a real mismatch, and it is not the fake against PG. In the source, every user has a row with `NOT NULL DEFAULT true`. In the rewrite, a settings row exists only after a write (`uow.rs:585`), and turning cleanup on over defaults is a no-op (`settings.rs:31`). Reads answer true (`store.rs:208`), so the UI says cleanup is on. Yet the PG sweep still uses `COALESCE(s.auto_delete_transcripts, false)` at `uow.rs:84` and `:650`, and the comment at `:616-617` describes that old rule.

Counterexample: an account created after cutover that never toggles is never swept, and PRV-022 counts it as opted out.

Repair state: p1X's fake now treats an absent row as true (`testing.rs:655-691`). The PG regression `retention_treats_an_absent_settings_row_as_the_source_default_...` (`tests/store.rs:538-578`) now expects deletion and **fails against current `uow.rs:84/650`** until p1R changes both predicates and the comment. p1R's handoff sentence "the in-memory fake treats a missing row as `true`" was contradictory when written; it is now accurate for the fake, while the PG adapter is still wrong.

## 3. Clause status

| Clause | Rules (main) | Implemented surface | Meaningful regressions | Evidence | Open in P3 |
|---|---|---|---|---|---|
| C1 scope | LNG, JOB statuses | `languages*.rs`, `JobStatus` (12) | `transcription_languages_are_a_superset_of_the_fixture` (61), `translation_targets_...` (53), `ui_locales_match_the_fixture` (16), `twelve_statuses_round_trip` | E (core 159) | Mixed rows stay strict; none certified |
| C2 boundaries | — | Dependency graph checked: media-ports and media-adapters have no billing or host dependency; core has no I/O crates | — | S (manifests) | none found |
| C3 taskr | sample | `crates/taskr-work`, `taskr-billing` 0003 | 26 PG service tests and 15 unit tests (callback, fallback, erasure, retention); isolation uses a unique `retention_schedule` per harness, past-dated sweeps and a `WORKERS` guard (`tests/service.rs:43-57, 315, 522-550`) | E (actual-process smoke) | Service-suite interleaving failure under owner repair (integration notes) |
| C4 atomic jobs | JOB, BIL-025..037 | `Jobs::start` / `charge_and_start`, revision CAS | `a_lifecycle_save_is_a_revision_compare_and_swap...`, `two_processes_claiming_the_same_provider_capacity...` | core E, PG S | **F4** |
| C5 attempts | JOB-018..025, TRX-015 | claim, ack, reconcile, stuck | `a_live_claim_fences_the_job`, `stale_attempts_are_fenced` (pure), `uncertain_submission_does_not_fail_over` | E (pure/fake) | **F3, F5**; no use-case test for a superseded callback |
| C6 routing | TRX routing | `routing.rs` | `pins_that_cannot_execute_fail_over`, `last_resort_waits_or_fails_explicitly`, `deepgram_capacity_keeps_one_slot`, `translation_requires_soniox` | E | — |
| C7 callbacks/money | D6, BIL-034..037, JOB-027..031 | `intake`, `ingest`, `apply`, `apply_duration` | `d6_soniox_callback_preserves_received_duration_...` (adapter, E), `callback_completes_once`, `deferred_charge_from_the_result` (E), `a_rolled_back_savepoint_undoes_only_its_own_writes` (S) | partial E | No use-case test for refused or storage-error deferred debit committing lines (required "unknown-duration debit rollback"); **F6** |
| C8 editing | EDT, D7 | `Jobs::edit` (account lock, update-only, `RenameSpeaker`) | `edits_are_owner_scoped_last_write_wins` (E), `d7_line_edits_update_only_...` (S) | partial E | No concurrent-LWW race on real PG |
| C9 billing erasure | D9, D10, D13 | billing core/PG/HTTP | billing suite, 244 passed | E | Known issues were repaired by p1X; not re-reviewed here |
| C10 barrier/resources | PRV-001..010, D2, D9 | `Privacy::request_erasure`, `erase_locally`, `run_remote`; `account_resources` | `erasure_runs_to_fully_erased`, `erasure_waits_for_subscription_stop`, `manifest_phases`, `phases_gate_each_other` (E); `the_manifest_captures_superseded_...` (S) | partial E | **F1, F2, F3c, F7, F9**; no expired-PUT recreation run on a real store |
| C11 retention | PRV-013..025/029/031, CRN-005/007 | `retention_run`, `retention_plan`, `history_retention_run`, slots | `retention_respects_the_preference`, `retention_skips_while_locked`, `cutoff_adds_one_day`, schedule slot tests (E); media-postgres slot and purge suite, 5 passed (E); jobs-postgres retention and history (S) | partial E | PG default predicate (section 2); jobs-postgres history and interrupted-run handling unrun |
| C12 composition | CRN-004, EML-004/005, API rows | `transgate/server` host and workers, `transgate/http` | http: 4 helper tests only, nothing on routes. server: config, mail and retry tests only | E (21 api/http/export) | No route-level callback, export (FRE-022), 410 or stale-JWT test; no composed DB run, restart, mail delivery or two-process race. All required regressions in the contract's list stay unproved for Transgate |

**Processor (D1, D12).** 40 tests passed (E): real FFmpeg, HTTP and process lifecycle, all seven families. I did not review it again.

## 4. Later-phase obligations kept visible (not waived)

- **P4:** FRE-024 chat check and increment, AI consent enforcement, and PRV-040 AI deletion.
- **P5:** rendered views, including PRV-027/028 settings copy.
- **P6:** free-tool pages.
- **P7:** API-key URL mode (`submit_urls` route answers "API customers only"), customer callbacks, and public limits.
- **P9:** inventory evidence from the real snapshot and drain of the legacy writer (the F9 runtime capability itself is P3), D8 drain of unsigned/RunPod attempts, latest-snapshot migration, deployed bucket retention policies, and live provider acceptance (D3 opt-out effect, D4 Whisper languages, D11 v5).
- The six strict P2 rows stay red: BIL068, BIL072, BIL073, BIL075, FRE022, FRE024.

## 5. Limits of this review

- I did not trace every one of the 526 rules.
- The processor and the billing-erasure internals were only sampled.
- Apart from F2, which the lead has since observed, none of the findings has been reproduced. Each one is a source path with a consumer-visible schedule and needs a failing-first test from its owner.
