# Executable gates

The three PRD commands are independent readiness checks, not phase-completion percentages:

- `./scripts/gate-rustr.sh`: workspace tests, warning-free Clippy, book listings, unchanged generated native bindings/tokens, all 17 scenarios with executable evidence.
- `./scripts/gate-modules.sh`: resolved direct/transitive module boundaries, billing and CMS with both host consumers, module tests and compilation of all workspace consumers.
- `./scripts/gate-transgate.sh`: current rules/parity documents match their discovery sources, every retained obligation has passing evidence, server/web tests, desktop/phone flows, Docker liveness, migration parity, Android signed-in flow and Linux Swift compilation.

Every stage runs even after an earlier failure. Any failed or missing requirement makes the command exit nonzero. P1 permits these product gates to fail; it does not permit an absent implementation to pass. CI exposes the same failures rather than using `continue-on-error` or skipping missing components.

## P2 billing phase proof

`BILLING_TEST_DATABASE_URL=postgres://... ./scripts/gate-p2-billing.sh` proves billing without pretending P6's CMS exists. The URL must point at a disposable PostgreSQL database. This phase gate checks both billing consumers, runs all billing crate tests including database tests marked ignored for ordinary offline workspace runs, requires every one of BIL-001 through BIL-079, FRE-001 through FRE-025 and CPN-001 through CPN-019, and builds the workspace consumers. Missing rules cannot reduce the denominator. Boundaries are checked for every module already present, even outside the selected billing test scope.

The underlying commands are `check-prd.py modules --module billing` and `check-prd.py billing`. The latter validates discovery/canonical ledger consistency but executes only those 123 rule obligations; it does not certify web/API parity rows or other topics. The final `gate-modules.sh` remains unchanged in scope: billing and CMS must both exist and have two consumers. The final Transgate gate still requires every rule and parity row.

Module and named-evidence runs include ignored Rust tests explicitly. Database tests must fail when explicitly run without their required database configuration, never return early and report success. Ordinary offline test runs may leave these tests ignored; ignored output alone is never implementation evidence.


## Evidence references

Rule test cells use `cargo:<package>:<fully-qualified-test-name>`. Parity also accepts `playwright:<report-relative-spec-path>#<full test title>`. Multiple references are separated by `; `. The checker executes the named Rust package suites and requires each exact test to appear as passed. An ignored, missing or failed test, including a successful Cargo command that ran zero tests, cannot satisfy a row.

A Playwright evidence reference must pass in projects named `desktop` and `mobile`; skipped and expected-failure cases do not count. The dedicated e2e gate also runs the complete configured suite, so references are not a license to omit other flows.

Rule statuses `tested` and `changed` both require passing tests. Parity `dropped` is accepted only with `keep-throw:<exact item name>` in its test cell, a `throw` verdict for that item, and a reason containing the parity ID and explicit `mu approved` text. Record the actual approval date and rationale there. This is an auditable decision record, not automated authorization to remove scope.

Discovery assembly accepts `ledger`, `tested` and `changed` rule states. An implemented state without a test reference is invalid; assembling a reference does not certify it passed. The readiness checker still executes the referenced tests and rejects missing, ignored or failed evidence.

Scenarios live in `docs/scenarios.md`, exactly SC-01 through SC-17. `done` requires Rust evidence and an existing book chapter. `existing` preserves prior implementation claims awaiting registered evidence; it is not accepted as gate-green.

## Later implementation entry points

`transgate/web` will expose `test:ci` and `e2e` npm scripts. Later phases supply these real executables; missing files currently fail explicitly:

- `transgate/scripts/check-migration.sh`: run the real migration dry run on the supplied current snapshot, require all PRD parity counts and per-user balances to match. Never replace this with a seeded-fixture pass for release readiness.
- `transgate/scripts/check-android.sh`: exercise the actual signed-in flow on Waydroid.
- `transgate/scripts/check-swift.sh`: compile Transgate bindings and views using the Linux Swift strategy required by the PRD, with any Mac-only limit explicitly recorded.

These are integration contracts, not shipped stub scripts. Runtime `/health` currently proves only process liveness; it does not imply those features exist. Secrets and snapshots are external inputs and must not be committed.

## Foundation commands

`python3 -B scripts/smoke-transgate-runtime.py` exercises the built local server, HTTP JSON, 404, invalid/occupied bind and SIGINT/SIGTERM. `--docker` builds the image, runs it as non-root with a read-only filesystem, checks HTTP and Docker health, then verifies graceful stop and removes only its own container. `python3 -B scripts/gen-tokens.py --check` checks generated shell assets without rewriting them. `./scripts/gen-bindings.sh --check` generates into a temporary directory and compares Kotlin/Swift bindings without rewriting checked-in outputs.
