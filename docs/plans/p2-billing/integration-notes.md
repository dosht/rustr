# P2 lead integration notes

## Current execution

Both independent proposals and reciprocal critique rounds are complete; lead approved `implementation-contract.md`. Visible OMP developer `p2-developer-sonnet` is pane `w23:p1T`, tab `w23:tF`, using `developer-models.yml` as a process-only overlay: Sonnet 5.5 High default, Opus 5.5 High plan, Astra High slow/advisor. Session: `/home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T12-26-23-396Z_01a16de0-5f24-7604-829d-1ae4df4843b2.jsonl`. Opus advisor investigates actual provider order-identity evidence; Astra reviews rule/runtime coverage risks. No machine-wide configuration changes.

## Verification environment

Lead created an isolated disposable `postgres:17-alpine` container named `rustr-p2-postgres`. It publishes only `127.0.0.1:32769`, uses tmpfs storage, and reports healthy. Its local development database URL is `postgres://postgres:postgres@127.0.0.1:32769/rustr_billing`. These are deliberately public local test credentials, not provider or production credentials. Do not use the unrelated existing Supabase containers on 55322.

Tests must isolate accounts or schemas and never truncate shared tables. Required adapter tests may not quietly return success when the database variable is absent. Lead will remove only this owned container after evidence is saved. Runtime proof must exercise actual transactions, including simultaneous deductions and rollback, not only in-memory fakes.

Observed PostgreSQL 17.11, UTC session zone, and zero application tables before module migrations. No existing application database is reused.

## Evidence boundaries

There are 123 billing-topic rules: BIL-001 through BIL-079, FRE-001 through FRE-025, CPN-001 through CPN-019. Some rows describe presentation, HTTP or host lifecycle behavior; a scalar helper test alone does not establish integration. Full web product parity remains P5, but P2 business/presentation policy must be implemented and meaningfully exercised.

The discovery keep/throw table contains proposed simplifications that conflict with its explicit Needs mu defaults. In particular, its suggestion of one freemium definition does not authorize unifying the three captured policies. Preserve rules until an outcome-changing decision is explicitly recorded. Also distinguish security/concurrency hardening from payment/package outcome changes; merely appearing in the Defects proposal column is not human approval.

PRD section 2 does permit clearly established defect fixes without a new approval round, provided the ledger records current behavior, intended behavior and the decision. Apply that exception narrowly (for example authenticated ownership, serialized deductions or a paid transaction receiving its packages exactly once). It does not authorize changing explicit Needs mu choices such as window inclusivity, month stepping, freemium eligibility or deferred delivery.

The existing final module gate requires CMS, a P6 deliverable. Advisors must propose phase-aware checking without hiding absent final requirements. Do not add an empty CMS crate or mark the entire project ready.

## Lead ownership

Root Cargo.toml/Cargo.lock, gate/checker scripts and their regression tests, canonical ledger integration, progress board and final milestone/book integration. Developer ownership will be defined by the reconciled design. No concurrent writes to these lead-owned files without a handoff.

## Gate prerequisite completed

Discovery assembly previously rejected every rule state other than P0's `ledger`, so it could not preserve implemented evidence. Lead extended validation to accept `tested` and `changed` only with a nonempty test reference; unsupported states still fail. Seven discovery checker regressions pass, and the actual `assemble-discovery.py --check` still reports all 526 obligations and 143 mapped SQL assertions. No obligation has been marked tested by this prerequisite.

## Resolved SQLx migration API

Rust analyzer resolved `Migrator::run` to the installed sqlx-core 0.9.0 source. `src/migrate/migrator.rs:97-127` exposes `dangerous_set_table_name` (schema-qualified names accepted) and `create_schema`. A new billing migrator can create schema `billing`, use history table `billing._sqlx_migrations`, then run its own migrations without changing taskr's public history or connection search paths. Set both on the mutable migrator in separate calls; each setter returns `&Self`. Keep migration locking enabled. The dangerous method warning concerns changing an existing history location; this is a new module with no previous migration history. Implementation must prove both migrators coexist and rerun cleanly.

## P2 execution gate

Lead added `scripts/gate-p2-billing.sh`, with `BILLING_TEST_DATABASE_URL` required, and explicit `check-prd.py modules --module billing` / `check-prd.py billing` modes. The final default module gate still requires billing and CMS. The scoped rule mode requires exactly 123 BIL/FRE/CPN IDs, keeps global discovery integrity checks, and does not certify web/API parity. Boundary checks still reject host dependencies in every present module. All module/named-evidence runs use `--include-ignored`; implementation database tests may be ignored for ordinary offline workspace runs but must fail, not skip successfully, if explicitly run without their database URL.

Observed prerequisite proof: all 13 Python checker tests pass. The actual billing checker reports 123 rows and 123 unimplemented requirements, exits 1. The phase shell gate refuses a missing database URL with exit 1. These are expected red checks before implementation, not P2 completion.

## Cross-ledger corrections

Lead checked the actual checkout return order and DAO error handling: a payment insert failure does not return the Iyzico form. DEF-008 now states the orphaned initialization/correlation risk without claiming that this path exposes a payable form. The data-model inventory and keep/throw row now retain the live Iyzico `pricing_plan_id`, `customer_id` and `subscription_canceled_at` fields. Canonical discovery output was regenerated: 526 rules, 153 defects, 206 keep/throw rows, 388 parity rows, 143 mapped SQL assertions.

## Integration preparation and before-fix reproduction

Lead registered all seven new packages and resolved Cargo.lock with offline Cargo metadata (472 resolved nodes), without building the developer's partial sources. Docker now copies the billing crates/host and SQL migrations, with CA certificates for outbound TLS. CI has a PostgreSQL 17 service and an explicit P2 billing matrix entry. `actionlint` was unavailable locally; no workflow-lint success is claimed.

Lead reproduced the exact legacy allocation expression in the owned PostgreSQL 17.11 database using a self-contained VALUES query, with no schema writes: before balance 84, actual debit 26, persisted balance 58, arithmetic return 74, and balance 74 after the negative package expires. This is executable before-fix evidence for D1, not a production observation or a passing Rust regression.

Current discovery assembly includes 154 defects after the measured exact-money finding; all 526 rules, 388 parity rows and 143 mapped SQL assertions remain. No billing rule is verified yet. The provider alias limitation and selected manual-resolution path are explicit D4, not an automatic-correlation claim.

## First integrated source and test pass

The workspace initially failed compilation on a synthetic LemonSqueezy fixture holding a mutex guard across an await, then on test-support imports and its missing restricted-runtime helper. Those were repaired. The developer reported a clean `cargo test --locked --workspace --all-targets --no-run`, without running tests.

Lead then ran the actual workspace with `--include-ignored` and both database URLs pointing to the owned disposable PostgreSQL. Earlier suites passed, but the run stopped at `taskr-server/tests/billing.rs:214`: a fixture checkout collided with the global `payments_provider_token` uniqueness constraint, returning 409 rather than 200. This is a failed integrated run, not a green workspace or a reason to weaken production uniqueness.

The new CI YAML parsed successfully with PyYAML and its matrix/service/environment fields were checked. This does not replace unavailable actionlint or an actual GitHub Actions run.

Repair ownership is in `repair-contract.md`: Sonnet handles core/storage/hosts; a separate visible OMP Astra handles gateways. Gateway pane w23:p1V, tab w23:tG, session `/home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T14-22-43-431Z_01a16e4a-e0e7-768b-9820-906482bf52ae.jsonl`. Opus and the original Astra remain reviewers. Source-complete remains distinct from tested and runtime-proved.

## Exercised consumer proof and final P2 regressions

Lead ran both actual server binaries against separate disposable PostgreSQL databases and restricted runtime logins, with local real-HTTP provider/auth/mail/analytics services. taskr proved explicit signup anchoring, concurrent immutable work replay, credit refusal without writes, hosted checkout, owner-only return status, signed order-item lookup, provider outage without a grant, duplicate safety and unmetered ordinary tasks. Transgate proved trusted-directory adoption and expired trials, inclusive-tax checkout, durable buyer/sales delivery across process restart, PostHog event identities, IYZWSv2 confirmation, V3-held renewals with explicit admin alias resolution, one renewal grant and manual cancellation/support delivery. Both stopped gracefully. These are real process checks, not live provider or production acceptance.

The final pre-P3 workspace command passed **454 tests across 39 test binaries**, with both `BILLING_TEST_DATABASE_URL` and `TASKR_TEST_DATABASE_URL` pointing to the owned `rustr_billing_p2` database and `--include-ignored`. The new post-start metadata PostgreSQL regression also passed separately. Required fixes exposed by integration were:

- Global outbox tests stole/backed off each other's events under fixed clocks. They now use private databases and the real global claim API; no account-filtered production escape hatch remains.
- Parallel host fixtures re-provisioned the same cluster role, causing `tuple concurrently updated`. Provisioning is once per process.
- Reused order ids collided with prior-run provider identities. The renewal fixture now namespaces every order and scopes held listings to its subscription; assertions verify exact package/payment counts before and after replay and duplicate delivery.
- Final evidence review removed dead status mapping and tautological tests, restored `0 min` for expired/no-open packages, derived actual post-start metadata, exercised repeated coupon purchases and checked independent provider plan bindings over HTTP.

`scripts/smoke-transgate-runtime.py` passed local SIGINT/SIGTERM and invalid/occupied-bind checks. Its Docker mode built the locked release image and passed health/readiness distinction, unknown-path 404, non-root/read-only operation, container health and graceful stop. This was the unconfigured runtime smoke, not configured billing acceptance inside Docker.

Strict Clippy subsequently exposed small conditional/ordering/fixture-type cleanups. Lead applied those; a final clean Clippy run is still pending while P3 source is introduced. No lint success is claimed.

The canonical billing ledger now has 103 `tested`, 14 `changed` and six incomplete whole rows. The P2/global gate must still reject BIL-068 (browser GET redirect), BIL-072 (client analytics/countdown), BIL-073 (periodic browser refresh), BIL-075 (browser PDF), FRE-022 (real download path) and FRE-024 (chat check/increment bypass). Their implemented HTTP/data/server-event/formatting/cap-parsing subsets do not establish the remaining P3/P4/P5 behavior. All 123 obligations remain. PRD section 8 permits overlapping phases, not relabeling unfinished clauses green.

The resolved Cargo graph separately passed the billing boundary check: five module packages, both consumers and no transitive host dependency. This is graph proof, not a fresh full module gate after P3 extensions. Discovery regeneration/check and all 13 Python gate regressions passed with the updated evidence.
