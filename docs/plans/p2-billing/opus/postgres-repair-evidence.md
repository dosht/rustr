# Postgres repair evidence (Opus)

Source-complete handoff, 2026-10-24. Scope: `modules/billing/postgres/**` only, ownership transferred by Sonnet (`w23:p1T`) under the user's authorization. Core (`ports.rs`, `app.rs`) was not edited. **Nothing here was compiled or run**: no build, test or formatter. The lead runs the integrated pass.

## 1. BIL-042: imported cross-account hard-delete refunds

Rule: deleting a job for good removed every `usage_history` row of that job through the foreign key cascade, whoever the row named and whichever package it drew on, and the minutes returned (BIL-042; `astra/host-review.md` P1; `repair-contract.md` item 1).

Core seam (Sonnet, already in place): `release_subject_charges` calls `BillingTx::imported_release_owners`, locks every returned account in ascending order, rediscovers and fails with `StoreError::Conflict` if the set changed, then calls `release_subject`.

Postgres side, `src/tx.rs`:

- `release_subject(account, subject)`: deletes runtime `billing.usage` rows of `(account, subject)` (runtime rows always belong to the subject's account by the composite key) and **every** `billing.legacy_usage` row with `subject_ref = subject`: cross-account rows, rows on another account's package, negative and zero rows, rows whose package is null or gone. This replaces Sonnet's interim filter `(l.account_id = account OR package owner = account)`, which still retained a row naming a second account on a third account's package. Imported subject ids are the legacy job ids, unique across accounts, and the host has authorized the subject; that assumption is stated in a code comment.
- `imported_release_owners(account, subject)`: `{account}` plus the owner of every package any imported row of the subject references, sorted and deduplicated. A row's own `account_id` is not an owner: balances count usage by referenced package, so only package owners' balances change.
- Runtime authority is unchanged: DELETE on `legacy_usage`, no INSERT or UPDATE (existing test `def_002_the_runtime_login_cannot_import_update_the_ledger_or_gain_authority`).

Tests, `tests/ledger_tx.rs`:

| Test | Proves |
|---|---|
| `bil_042_a_hard_delete_cascades_every_imported_row_of_the_subject_across_accounts_and_signs` | Accounts A, B, C. Imported rows of subject J: A on A's package (30), A on B's package (+10), B on C's package (-15), A with no package (5), plus B on B's package for another subject (4). Anomalies on B's and C's packages are reported by `owner_anomalies` before the delete; `imported_release_owners` returns A, B, C sorted; the host hard delete releases 5 rows (1 runtime + 4 imported); A back to 100, B 66 (the unrelated row stays), C 60 (the negative row's credit goes, as the cascade did); anomalies gone afterwards |
| `bil_042_def_001_a_debit_of_an_affected_owner_waits_for_an_imported_refund_and_spends_it` | A's imported row drew 30 from B's package (B 70). While A's release holds its transaction, B's strict debit of 90 is shown waiting on a lock by its own backend pid, then succeeds after the commit; B ends at 10 |
| `bil_042_def_001_a_debit_of_an_affected_owner_waits_for_a_negative_imported_row_to_go_and_cannot_overspend` | A negative imported row credits B (120). B's debit of 110 started during the release waits, then is refused with `available: 100`: it decided on the post-release balance, not the one an unlocked read would have allowed |

The previous test `bil_042_a_hard_delete_also_returns_imported_consumption_of_the_subjects_own_packages` asserted the retention this repair removes, so it was replaced by the first test above. One pre-existing comment in it ("The other account's row for the same subject number is not touched: its owner was not locked.") described that retention and is gone with the assertion; the other comment was kept verbatim.

At handoff, the initial migration still contained a stale "own packages" comment. Lead subsequently removed that obsolete comment from this unreleased migration and created a new owned integration database, `rustr_billing_p2`, rather than modifying any applied checksum. Existing `rustr_billing` remains the historical first-run fixture database; final tests use the fresh database. No deployed migration was rewritten.

## 2. Runtime login and `billing_runtime` group authority

Finding: `authority_problems` checked only the login; an existing contaminated group passed and any member could `SET ROLE billing_runtime` to use it (`astra/host-review.md` P1; `repair-contract.md` item 4).

`src/lib.rs`:

- `group_problems`: the group must exist and must not be able to log in, and it gets the same full check as a login: attributes (superuser, create role/db, bypass RLS, replication), roles reachable through membership (predefined roles included), schema CREATE or ownership, table ownership, every column INSERT/UPDATE outside the runtime allow-list, and table DELETE/TRUNCATE/TRIGGER/REFERENCES outside the two ledger tables. Messages are prefixed `the group billing_runtime ...`.
- `runtime_login_problems(login)`: the login's own problems, plus `can grant billing_runtime to other roles` when the login holds the group WITH ADMIN OPTION, plus `group_problems`.
- `provision_runtime_login` refuses before creating or re-keying anything when the group is contaminated, and checks existing and newly granted logins with `runtime_login_problems`. `verify_runtime_role` (used by `connect_runtime`) uses `runtime_login_problems`, so a server refuses to start on a contaminated group. No owner-pool shortcut; ordinary runtime import permissions unchanged.

Tests, `tests/provisioning_roles.rs`:

| Test | Proves |
|---|---|
| `def_002_a_contaminated_billing_runtime_group_is_refused_by_provisioning_and_by_the_runtime_connection` | Five committed contaminations of the real group, one at a time: CREATEROLE, LOGIN, membership in `pg_write_all_data`, `UPDATE (minutes)` on `billing.usage`, CREATE on schema `billing`. For each, `connect_runtime` of the provisioned test login and `provision_runtime_login` of a fresh login are refused with the group message and no login is created. A member granted the group `WITH INHERIT FALSE, SET TRUE` gets 42501 on a direct ledger write but succeeds after `SET ROLE billing_runtime`, and `connect_runtime` refuses it; the restored group is accepted again |
| `def_002_a_login_that_can_grant_the_runtime_group_onward_is_refused` | A login holding the group WITH ADMIN OPTION is refused by provisioning and by `connect_runtime` |

Role state is cluster-wide, so a committed contamination would be visible to parallel tests. `tests/support/mod.rs` adds `ROLE_FIXTURE_LOCK` and `with_role_fixture_shared`: provisioning in `owner()`, every `runtime()` connection, the existing contaminated/clean login tests, and the whole body of the "runtime cannot write" test hold it shared; the group test holds it exclusively and restores each contamination before asserting. The last point matters: while the group briefly held `pg_write_all_data` or schema CREATE, that test's forbidden statements (`DELETE FROM billing.payments`, `CREATE TABLE billing.runtime_made_this`) would really succeed. Limits: advisory locks are per database, and host test binaries that call `connect_runtime` on the same cluster are not coordinated; cargo runs test binaries one at a time, and lead smoke processes should not run during the billing-postgres suite. `WITH INHERIT FALSE, SET TRUE` needs PostgreSQL 16 or later (the disposable database is 17.11).

## 3. Earlier Postgres review findings rechecked

| Finding (`astra/postgres-review.md`) | State |
|---|---|
| Host attachment accepts autocommit | Already fixed (`attach` compares two transaction ids) |
| Column-level runtime privileges | Already fixed; group and admin-option paths added above |
| Conflicting subscription identity | Already fixed (`insert_subscription` compares the full binding) |
| Hard delete leaves imported consumption | Fixed above, including cross-account and signed rows |
| Deferred SQL failure | Already covered (`bil_036_a_sql_failure_inside_a_deferred_debit_is_isolated_by_a_savepoint_and_delivery_still_commits`) |
| Held-event dedup discards conflicting evidence | Partly fixed earlier (customer and payment id joined the identity). Remaining gap fixed now: the same identity with a different payload no longer updates the reason or returns the first row as a retry; `hold_notification` returns `StoreError::Conflict` and the first evidence stays exactly as stored. Test: `tests/subscriptions.rs` `held_notifications_keep_the_first_evidence_and_refuse_a_different_payload_for_the_same_identity`. Core effect (Sonnet to confirm): a conflicting re-hold now surfaces as a 409 and the provider retries, instead of a silent 200 |
| Migration coexistence wrote into the host history | Already fixed (synthetic host uses its own history table) |
| Historical unknown status | Withdrawn by the lead |
| Lease expiry proof | Present (Sonnet's lease-expiry test in `tests/outbox_customers.rs`), not re-reviewed |

## 4. Files changed by Opus

- `modules/billing/postgres/src/tx.rs`: `release_subject`, `imported_release_owners`, `hold_notification`.
- `modules/billing/postgres/src/lib.rs`: `group_problems`, `runtime_login_problems`, their use in `provision_runtime_login` and `verify_runtime_role`.
- `modules/billing/postgres/tests/support/mod.rs`: role fixture lock; `owner()` and `runtime()` hold it shared.
- `modules/billing/postgres/tests/ledger_tx.rs`: three BIL-042 tests and helpers (`import_usage`, `legacy_rows_of_subject`, `host_hard_delete_in`, `debit_during_release`).
- `modules/billing/postgres/tests/provisioning_roles.rs`: two new tests, shared-lock wrapping of four existing paths.
- `modules/billing/postgres/tests/subscriptions.rs`: one new test.

## 4a. Unique provider identities per test (repair contract item 9)

Audit of every fixed provider identity in `modules/billing/postgres/tests` against the unique keys `payments_provider_token`, `payments_provider_order`, `subscriptions (payment_gateway, environment, subscription_ref)` and `unresolved_identity`:

| File | Fixed identities found | Change |
|---|---|---|
| `payments.rs` | `tok-1`, `tok-same`, `tok-plan-1`, `tok-plan-2`, `sub-9`, `tok-f`, `tok-u`, `tok-race`, `tok-env` | Every scripted gateway gets `test_env("sandbox")`, a fresh environment per test. The environment is part of each of those keys, so the literal tokens and `sub-9` are isolated per test and per run. The two tests that insert payment rows directly (`bil_045_an_invalid_package_interval_rolls_back_the_whole_completion`, `bil_043_058_an_imported_canceled_payment_...`) now write the gateway's own environment so confirmation still finds them. `tok-same` stays deliberately duplicated inside its one test |
| `subscriptions.rs`, `lemonsqueezy_orders.rs`, `outbox_customers.rs`, `ledger_tx.rs` | none: refs are already built from a per-test uuid tag, LemonSqueezy checkout and order ids are uuid-based, payment ids are database ids | none. LemonSqueezy cannot use a salted environment anyway: core derives `test`/`live` from the order's `test_mode` |
| `support/mod.rs` | `success("cust-...")` customer refs and `provider_payment_id: "pay-1"` | none needed: neither is under a unique index (customer refs only join a held-notification identity together with unique order and subscription refs) |

`support::test_env(base)` returns a leaked `&'static str` because the scripted adapters keep `&'static str`; it is test-only. Production indexes are unchanged and nothing is serialized.

Sonnet's relay change (each recipient tried independently) is compatible with `a_failed_recipient_keeps_the_event_unacknowledged_and_a_retry_does_not_resend_the_buyer`: it asserts the buyer is mailed once and the event stays unacknowledged while sales fails, not that a failing recipient blocks later ones. No relay test needed changing.

## 4b. Integrated-run failure: `eml_bil_001_002_003_committed_events_are_delivered_to_buyer_sales_and_support_with_stable_keys`

Observed by the lead: "condition not reached" (`tests/outbox_customers.rs` `wait_for`) on fresh `rustr_billing_p2`, other tests of the binary passing. Cause from source, not timing:

- The outbox is one table for every test, and earlier suites leave unpublished events in it. Four tests of this binary run in parallel and three consumed it globally: the email test and the failed-recipient test through `relay_outbox`, and the lease and concurrent-claim tests through `claim_outbox(1000, ...)` directly.
- Claims, leases and back-offs are measured on each fixture's fixed clock (all start at 2026-01-15 12:00). The lease test claimed every unpublished event with a 10-second lease and released only its own, so the email test's fresh events stayed `claimed_until 12:00:10` while the email test's clock never left 12:00. The failed-recipient test's relay likewise backed foreign events off by 30 s and then moved its own clock 24 h ahead, and every relay acknowledged other tests' mail-free events (`FreemiumGranted`, `NotificationHeld`) as delivered.
- No sleep or retry bound was changed.

Repair (lead direction: test isolation only; no core change; production claim SQL and API unchanged). A first version added an account-filtered claim API; the lead rejected it as an unrequested production surface, and it was reverted: `src/store.rs` is back to its previous text.

- `tests/support/mod.rs`: `isolated_runtime()` creates a fresh database `billing_t_<16 hex>` on the shared test server (owner connection from the explicit `BILLING_TEST_DATABASE_URL`, no environment mutation), runs the billing migrator in it, and connects the already-provisioned runtime login to it through `connect_runtime` (under the shared role-fixture lock). `IsolatedDatabase` drops the database `WITH (FORCE)` when it goes out of scope, on success or panic, from a short-lived runtime on its own thread. `isolated_fixture_with(gateways, configure)` builds the normal `Billing` over the real `PgBillingStore` on that database; `Fixture.isolated` holds the guard. Shared fixtures are unchanged.
- `tests/outbox_customers.rs`: the four tests that claim or relay the outbox use `isolated_fixture_with` and the real, global `claim_outbox`: `eml_bil_001_002_003_...`, `a_failed_recipient_keeps_the_event_unacknowledged_...`, `two_concurrent_consumers_never_hold_the_same_event` (with the added assertion that all 8 events are claimed exactly once, now meaningful because the database holds only those 8) and `outbox_after_a_lease_expires_...` (original lease, stale-claimant and positive-bound checks unchanged). No shared outbox rows are deleted; no sleep, retry or serialization change.
- Not covered by isolation: tests that never claim the outbox stay on the shared database. Host test binaries that relay on the shared database are outside this crate.

Coordination: no core relay defect found. Core's relay is global by design and correct for one relay per database; the defect was test isolation in this crate.

## 5. Compile risks to watch in the first run

Not compiled. Points most likely to need a touch: `impl Future` in `with_role_fixture_shared` (relies on the 2024 prelude); `<sqlx::PgConnection as sqlx::Connection>::connect_with(&options)`; `sqlx::query(contaminate)` with `&'static str` literals; deref coercion of `&f` inside the extracted `runtime_login_cannot_import_update_or_gain_authority(f: &Fixture)`.

Run: `BILLING_TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:32769/rustr_billing cargo test --locked -p billing-postgres -- --include-ignored`.
