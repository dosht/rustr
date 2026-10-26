# Postgres review

Read-only source review against the approved contract and D1-D4. No builds, tests or formatters run. Locations refer to the saved slice reviewed on 2026-10-24. HTTP, gateway protocol and unfinished host composition are excluded.

## P1: host attachment accepts autocommit

`modules/billing/postgres/src/tx.rs:45`, `PgBillingTx::attach`, checks only `transaction_isolation`. An ordinary acquired `PgConnection` outside `BEGIN` reports `read committed` and passes. Its account lock ends with that SELECT; subsequent ledger inserts commit separately. A later insert or host CAS failure leaves a partial charge, violating DEF-001 and the atomic host/batch contract.

Narrow fix: make the public attachment seam require a borrowed SQLx `Transaction<Postgres>` and retain the isolation check. Do not expose a safe autocommit constructor. Prove a late failure rolls back host facts and every debit through this exact seam.

## P1: runtime-role validation misses effective write authority

`modules/billing/postgres/src/lib.rs:98`, `verify_runtime_role`, checks table-level UPDATE/INSERT only. A runtime login with an extra `GRANT UPDATE(minutes) ON billing.usage` passes those checks yet can rewrite usage. An extra column-level INSERT grant on the legacy columns likewise bypasses the table-level import check. `provision_runtime_login:55` preserves existing direct grants and memberships, so re-keying an existing login does not establish its documented restricted authority. This breaks DEF-002 and the actual-runtime-pool acceptance condition.

Narrow fix: reject prohibited column privileges as well as table privileges, and reject authority reachable through permitted SET ROLE paths to billing owners or other privileged roles. Provisioning must refuse unsafe pre-existing authority, or explicitly reconcile it before reporting success. Exercise contaminated existing logins through `connect_runtime`, not only a freshly created role. Keep the server on the verified pool.

## P1: conflicting subscription identity can grant twice or use the wrong account lock

`modules/billing/postgres/src/tx.rs:533`, `insert_subscription`, accepts every existing binding with the same account (`:573`), even when `initial_payment_id`, customer, plan or package specification differ. Two pending checkouts P1 and P2 for the same account returning subscription S both reach `modules/billing/core/src/app.rs:773` and grant; the second binding silently returns P1's row. There are two completed payments and two schedules for one initial subscription identity. BIL-060/061 and DEF-004/D3/D4 require conflicting bindings to fail, not become successful duplicates.

There is a related account-lock escape at `modules/billing/core/src/app.rs:971`, `notification_in`: the existing order payment, or the duplicate returned by `tx.rs:339`, is used without checking its account/subscription/customer against the locked binding. A verified S-B notification naming an order already attached to S-A can transition and grant A's pending payment while only B is locked. Even within one account, a different subscription must conflict. This is a persistence/caller identity contract issue, independent of provider order-list protocol.

Narrow fix: compare immutable subscription binding fields on insertion conflicts; reject a different initial payment. Validate the full stored payment binding before treating an existing order/token as a duplicate or transitioning it. Refuse conflicting identity without acquiring a second account lock out of order. Add a same-account/different-initial-payment case and a cross-account/cross-subscription existing-order case. Also use a composite `(account_id, initial_payment_id)` FK in `migrations/0001_billing.sql:128` so new subscription rows cannot reference another account's payment.

## P1: hard-delete leaves imported consumption behind

`modules/billing/postgres/src/tx.rs:326`, `release_subject`, deletes only `billing.usage`; balances also count `billing.legacy_usage`. After importing a normal 10-minute usage row for a subject, deleting that subject returns zero rows and leaves its package charged. The BIL-042 test at `tests/ledger_tx.rs:529` creates only new usage, so it misses the cutover case.

Narrow fix: include matching imported usage in the authorized subject-release operation, within the account/host transaction, while retaining the prohibition on runtime INSERT/UPDATE of imported rows. Add legacy-only and mixed-ledger hard-delete cases. For anomalous rows, preserve source cascade-by-subject behavior after authorizing the host subject; any additional affected package owner must participate in ordered account locking rather than having its balance changed unlocked.

## P1: deferred SQL failure is not isolated from delivery facts

`modules/billing/postgres/tests/ledger_tx.rs:473`, the deferred callback, updates completion facts, calls `charge_strict`, and commits regardless of its result (`:495`). `src/tx.rs:305`, `insert_usage`, propagates a failed SQL statement without recovering the transaction. A constraint failure on the second allocation aborts the outer transaction, so committing cannot preserve completion facts. The existing BIL-036 case only exercises a domain refusal before SQL; it does not prove the claimed failure behavior.

Narrow fix: demonstrate a savepoint around the entire deferred debit, after taking the account lock and writing host completion facts in the outer transaction. On a recoverable debit failure, roll back that savepoint before committing delivery; all debit rows must disappear. Add a forced second-insert failure and inspect both host completion and zero usage. If the connection itself is unusable, return a retryable failure instead of claiming that completion committed. This is a correction to the saved Postgres seam proof, not a review of unfinished host code.

## P2: held-event deduplication discards conflicting evidence

`modules/billing/postgres/src/tx.rs:654`, `hold_notification`, and `migrations/0001_billing.sql:158` identify duplicates by gateway/environment/event type/order/subscription/reason, then return the original row without comparing customer, payment or payload (`tx.rs:698`). Two verified LemonSqueezy notifications with the same order and `payment_unknown` reason but different missing payment IDs both receive the first held ID; the second payload is never persisted. A changed customer on a held subscription event is similarly lost. D3/D4 and the conflicting-payload acceptance require a conflict to remain distinguishable from an exact retry.

Narrow fix: compare normalized immutable fields when the unique insert loses. Return a conflict for a different binding/payload, or retain that conflicting observation explicitly in the bounded unresolved store. Exact retries remain idempotent; failure and success remain distinct. Do not add generic command receipts.

## P2: migration coexistence test writes a fake host version into the real host history

`modules/billing/postgres/tests/provisioning_roles.rs:245`, `migration_histories_coexist_and_rerun_cleanly`, writes synthetic version 1 into `public._sqlx_migrations`. The actual taskr migrator in `crates/taskr-postgres/src/lib.rs:25` also has version 1 (`0001_workspaces_tasks_outbox.sql`) with a different checksum. Running this test before the required taskr smoke poisons that database's host migration history; running it afterward fails validation. The billing migrator's separate history is correct, but this fixture does not prove actual-host coexistence.

Narrow fix: use the actual host migration for coexistence in the shared disposable database, or isolate the synthetic marker in a separate disposable database. Prove both real histories can rerun before marking migration coexistence complete.

## P2: historical status preservation is narrower than the contract

`modules/billing/postgres/migrations/0001_billing.sql:40` permits exactly four statuses for every writer, including the import role. The contract explicitly preserves unknown historical statuses. `src/tx.rs:132`, `payment_from`, also rejects an unrecognized stored status. `tests/provisioning_roles.rs:219` only imports the already-permitted `canceled` value, so it does not establish that preservation.

Narrow fix: preserve raw imported status without allowing it to enter runtime completion/grant transitions; cover an unrecognized imported value and ensure it cannot grant. Keep ordinary runtime status validation. This is a storage constraint finding; core interpretation remains with Opus.

## Lease proof needed before acceptance

`src/store.rs:49` uses a short atomic claim statement, and `ack_outbox:113` / `release_outbox:129` fence mutations by claim token. I found no concrete lost-ack defect in that SQL. However, `tests/outbox_customers.rs:318`, `two_concurrent_consumers_never_hold_the_same_event`, checks only two claims at the same instant. Add crash/expiry/reclaim evidence for BIL-066 delivery: claimant A expires, B claims with a new token, A cannot acknowledge or release B's claim, and B can finish. Bound inputs to a positive lease. Overlap after lease expiry is compatible with the approved at-least-once contract; do not claim exactly-once delivery from this test.

## Lead disposition

Lead accepted the concrete Postgres P1 findings and the migration-history, held-conflict and lease evidence requirements, and forwarded them to the developer.

The historical unknown-status finding is withdrawn. Lead confirmed that the baseline `payment_status` enum (lines 193-198) and discovery/data-model census (line 96) admit exactly `pending`, `completed`, `failed` and `canceled`. The earlier contract wording was overbroad. The corrected contract preserves these four states; a future divergent snapshot is a migration discrepancy, not a requirement for a speculative raw runtime state model. No change is requested for this finding.

Review is on hold until source handoff. No additional implementation review is authorized in the meantime.
