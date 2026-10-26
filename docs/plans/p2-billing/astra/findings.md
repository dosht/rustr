# Astra independent findings

Status: reading and design only. No implementation, builds, tests, formatters or agents run.

## Captured constraints

- P2 must cover BIL-001 through BIL-079, FRE-001 through FRE-025 and CPN-001 through CPN-019. Some are policy projections consumed by later host features, but their billing behavior and evidence must exist in P2.
- Inclusive package ends, iterative calendar stepping from checkout creation, negative balances and the three distinct freemium definitions remain defaults. A universal is_freemium flag would change business behavior.
- Two first jobs can both qualify while no other job is completed. Serialization must prevent overspending in the strict path without inventing a lifetime bypass marker.
- Strict deferred deductions may fail while the transcript is still delivered. P2 must record that outcome without charging again after a later top-up.
- Payment failed-to-completed recovery, unauthenticated renewal trust, cross-account ledger writes, equal-expiry allocation and unlocked deductions are documented defects. Proposed corrections must be explicit in the implementation contract.
- Historical ledger anomalies cannot be erased by positive-amount constraints. Imported records need a constrained import path and parity reporting, while new writes must enforce account ownership.
- Full module readiness still includes CMS in P6. A separately named billing-only phase gate must leave the final gate's absent-CMS failure visible.

## Read evidence so far

- docs/plans/p2-billing/brief.md
- docs/prd.md, sections 1-8
- docs/milestones/p1-foundation.md and modules/README.md
- transgate/docs/discovery/billing.md, rules and defects, inventory, migration and Needs mu
- scripts/gate-modules.sh and relevant check-prd.py entries
- docs/thoughts.md billing and concurrency references

## Delivery environment

The shell reports HERDR_ENV is not 1. The herdr skill's environment gate says not to drive panes from outside. The requested report target remains w23:p1E; delivery capability will be checked without changing any pane or spawning anything.

## Additional source findings

- `scripts/assemble-discovery.py::lint_rules` currently accepts only status `ledger`, while `check-prd.py::coverage` needs `tested` or `changed`. Lead must reconcile these validators before any honest P2 ledger success.
- The SQL allocation formula carries negative package balances into later positive packages. With earlier package remaining -16 and later package remaining 100, requesting 10 produces 26 positive deduction minutes; it returns arithmetic balance 74 but the next derived balance is 58. This is a newly surfaced outcome question, not an approved correction. Source: `frontend/supabase/migrations/20251230190000_fix_request_breakdown_user_id_again.sql:45-72` and `20260115162255_fix_freemium_first_job_bypass.sql:74-89`. A simple greedy allocation would change this behavior.
- Soniox source uses `metadata.duration_ms`, with maximum token `end_ms` fallback. The jobs discovery shorthand `audio_duration_ms` must not override that observed source.
- Iyzico's current official webhook documentation names `subscription.order.failure`, while the legacy ledger names `.failed`. The adapter should normalize both authenticated variants and pin each with a protocol fixture.
- `herdr pane get w23:p1E` reached the CLI but the sandbox denied socket access. The final explicitly requested report may need a narrowly scoped sandbox escalation.

## Completion and handoff

`proposal.md` is complete: 123 billing-rule IDs assigned; transaction/host seam, gateways, migration, verification, phase gate and implementation slices specified. The negative-prefix allocation counterexample and source contradictions are explicit, with no unapproved business correction applied.

The lead pane was verified as `rustr-lead`, workspace w23, cwd `/home/mu/src/rustr`. The sandbox escalation succeeded and the explicitly requested `herdr pane run w23:p1E` report returned exit 0. The report named the proposal, atomic account design, negative-prefix issue, checker/CMS boundary, JOB-008 contradiction and provider verification limits. No other advisor design was read. Holding for the lead's critique assignment.
