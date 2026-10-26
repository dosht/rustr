# transgate-jobs-postgres handoff (p1R)

Source only. Nothing in this crate has been built, formatted or run.

## Registration

New package `transgate-jobs-postgres` at `transgate/jobs-postgres`. The lead adds `"transgate/jobs-postgres"` to the root workspace members and updates the lockfile. It depends on `billing-core`, `billing-postgres`, `media-ports`, `media-postgres`, `transgate-core`, `sqlx 0.9`, `serde`, `serde_json` and `uuid` (v4, v5). Dev: `tokio`.

## Schema and roles

- Schema `transgate_jobs`, history `transgate_jobs._sqlx_migrations` (`migrations/0001_jobs.sql`).
- `transgate_jobs_postgres::migrate(owner)` runs the jobs migrations, then `media_postgres::migrate` and `media_postgres::grant_runtime(owner, "billing_runtime")`. Order: `billing_postgres::migrate`, transgate-billing migrations, then this.
- Runtime rights go to the `billing_runtime` group, so a server keeps one verified pool and one transaction with billing. Column-level grants: lines update only `line` and `speaker` (D7); settings write only the two user flags (C6); jobs never change `id`, `account_id` or `created_at`; attempts never change identity, request or secret; the barrier (`erasures`), retention audit rows and rate windows are never deletable.
- `verify_runtime(pool)` runs `billing_postgres::verify_runtime_role`, then this schema's and media's authority checks for both the login and `billing_runtime`.

## Public API

`migrate`, `verify_runtime`, `runtime_authority_problems`, `SCHEMA`, `RUNTIME_ROLE`, `PgJobStore` (`JobQueries`, `JobStore`, `claim_events`, `ack_event`, `release_event`, `ClaimedEvent`), `PgJobUow` (`JobUow`), `PgLocks` (`Locks`), `PgRateLimiter` (`RateLimiter`).

## Behavior decisions (confirmed by p1X; the in-memory fake now matches retention)

- Retention, lead decision after the p1Z audit: an absent settings row behaves as the source default `true` (swept, not opted out); `opted_out_count` counts only explicit false with lines. The regression `retention_treats_an_absent_settings_row_as_the_source_default_and_keeps_only_explicit_opt_outs` is in place; the production predicate still reads `COALESCE(..., false)` until the lead captures its failure. `lock_retention_set` never creates a settings row.
- History retention (CRN-007, PRV-025/031): the scheduler run history is `media.schedule_slots`. `history_count`/`delete_history` cover finished runs of the named schedules whose run started at or before the cutoff, in the caller's transaction with its audit row. Media migration `0002_slot_history.sql` limits the runtime DELETE to finished runs by row security. `fail_interrupted_runs` turns a job name's `running` audit rows into `failure` with `rows_deleted = 0`.
- Worker selection skips accounts with a host barrier for dispatch, stuck and unresolved attempts. Pending inbox entries of an erased account stay visible so ingestion settles them as ignored and drops their bodies. Remote cleanup is not filtered (it needs no billing lock).
- Settling an inbox entry as applied or ignored keeps its remote identity, origin, Soniox status and reported duration or the Deepgram request id and duration. Transcript content is dropped; the fingerprint stays.
- `account_resources` captures upload and staged media objects, every attempt's remote identity (superseded included), acceptance-unknown attempts (claimed or uncertain with no remote id) by client reference, and issued uploads by account, so a hard-deleted draft's upload still gets its re-sweep.
- `erase_account_rows` deletes lines, inbox, attempts, issued uploads, archived payload rows, job events, jobs and settings, then, when `transgate_billing.customers` exists, takes `pg_advisory_xact_lock(transgate_billing.customer_lock_key(account))` and deletes the customer row (p1T's `0003_erase_customer.sql`).
- D9: when an erasure closes, confirmed and absent items are deleted; approved exceptions stay listed.
- A second concurrent erasure request for the same account fails on the unique barrier (`StoreError::Conflict`).
- Rate keys are stored only as a name-based UUID; denied requests do not count.

## Tests

`tests/store.rs`, 12 tests, all `#[ignore]`d without a database:

```sh
TRANSGATE_JOBS_TEST_DATABASE_URL=postgres://... cargo test -p transgate-jobs-postgres -- --include-ignored
```

The URL needs owner authority on a disposable database. The tests migrate billing and jobs, provision the login `transgate_jobs_test_runtime`, and connect through `connect_runtime` plus `verify_runtime`. They cover restricted grants, revision CAS and owner-scoped locks, the two-process capacity lock wait, durable intake replay/conflict and body drop, savepoint rollback, update-only LWW edits and byte-order sorting, worker filters under erasure and the unique barrier, manifest capture and host cascade with the surviving barrier and D9 item drop, retention opt-in, the shared rate window, cross-process advisory locks, and fenced event claims.

## Not covered here

The transgate-billing customer deletion is exercised only where that schema is installed; these tests do not install it.
