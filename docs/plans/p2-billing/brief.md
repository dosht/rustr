# P2 billing: independent designs, then reconciliation

Status: design in progress. No implementation is approved by this brief alone.

## Goal

Implement PRD P2 completely: reusable billing core, Postgres and HTTP boundary; Iyzico and minimal LemonSqueezy adapters, fakes and recorded/documented fixtures; genuine taskr and Transgate consumers; billing ledger evidence and a passing billing module gate. Do not implement P3-P10 just to make their future gates green. Preserve business policy. PRD section 2 permits clearly established defect fixes when current behavior, intended behavior and the decision are recorded; explicit Needs mu policy choices retain their documented defaults until mu decides.

mu requested two independent proposals, a concrete critique exchange, then a separate OMP developer. Advisors produce design and interface sketches, not executable implementations, builds, tests or format runs. No hidden subagents. Each advisor saves incrementally in its own folder so findings survive interruption.

## Ownership and communication

- Lead: herdr pane `w23:p1E` in workspace `w23`; owns integration decisions, root manifests/lockfile, gate changes and final evidence.
- Opus advisor: `w23:p1R`, output only `docs/plans/p2-billing/opus/`.
- Astra advisor: `w23:p1S`, output only `docs/plans/p2-billing/astra/`.
- Shared checkout: `/home/mu/src/rustr`. Do not change branches, create worktrees, commit, touch other writers' files or invoke another coding agent.
- Source Transgate repositories under `/home/mu/Business/Transgate` are read-only. No production calls, secrets/env reads, provider transactions or real customer data copied into artifacts.
- When the first proposal is saved, send a short report with `herdr pane run w23:p1E 'P2 <model> proposal ready: <path>; <main decision and unresolved risks>'`. Await a critique assignment, not an implementation assignment.
- Do not contact the other advisor or read its design before your independent proposal is saved. After lead dispatches critique, read the other proposal, write `critique.md` in your own directory with specific counterexamples and concessions. Debate correctness, not model preference.

## Required reading

Read the relevant sections, not whole repositories:

- `docs/prd.md`: sections 1-5, 7-8, especially P2 and both-consumer rule.
- `WORKING-ON.md`, `docs/milestones/p1-foundation.md`: implemented baseline and verification limits.
- `docs/thoughts.md`: stored procedures, ledger and concurrency decisions.
- `modules/README.md`: boundaries; module cannot depend even transitively on taskr or Transgate.
- `transgate/docs/discovery/billing.md`: complete topic, including rule/defect/parity tables and Needs mu assumptions.
- Cross-ledger facts only as needed in `jobs-transcription.md`, `privacy-identity.md`, `ai.md`, `api.md`, `data-model.md` beside it. Canonical assembled rows are in `transgate/docs/rules.md`, `keep-throw.md`, `parity.md`, `pgtap-map.md`.
- Existing workspace/storage/server conventions, found by targeted reads. `crates/taskr-core` must remain dependency-free and I/O-free. Prefer normal Rust adapters with explicit ownership/transactions.
- `scripts/check-prd.py`, `scripts/gate-modules.sh`, `scripts/assemble-discovery.py` explain evidence formats and current gate behavior.

## Proposal contents

Write `proposal.md` with these concrete outputs:

1. Scope matrix listing every billing topic rule ID/range (including freemium and coupons), its implementation owner, boundary and test strategy. Separate actual runtime rule from an explicitly excluded legacy item. No dropping rows or labeling business gaps as future work.
2. Crate/module graph, key domain types and port/API sketches. Both consumers must exercise behavior, not merely depend on an unused crate. Keep business policy out of shells and gateway-specific details out of pure core.
3. Transaction and concurrency protocol: atomic plan/apply, per-account serialization, request/webhook deduplication, conflicting payload handling, rollback, outbox and failure recovery. Explain what happens under simultaneous deductions, two first jobs, repeated and out-of-order payment notifications. No provider HTTP while holding a database transaction.
4. Fidelity details: package windows and equal-expiry ties; integer minutes and exact money; calendar month stepping; negative balances; first-job web guard versus database eligibility versus strict deferred deductions; coupon applicability and currency; recurring Iyzico versus one-off LemonSqueezy; signature/authentication trust boundary. Evidence links for contentious choices.
5. Gateway execution path and fixture provenance. A fake is a deliberate test adapter, never a fallback for failed live configuration. Iyzico sandbox credentials and LemonSqueezy test store are unavailable for this work; adapters still must be real HTTP implementations proved with local protocol fixtures.
6. Storage schema prefix/migration plan and host integration. Preserve migration-compatible IDs and historical anomalies without silently trusting new cross-account inputs. Identify any data-dependent assumptions without blocking everything on a production snapshot.
7. Verification matrix with concrete consumer-visible behavior, Postgres concurrency/rollback proofs, local HTTP adapter proof, both-consumer smoke, and exact ledger test-reference strategy. Domain changes require failing tests first during implementation. Advisors do not run them now.
8. Ordered implementation slices with file ownership and narrow dependency contracts for one OMP developer to start. Avoid optional abstractions, broad cleanups, and premature framework work.
9. Unresolved business decisions only. Preserve documented current behavior when the ledger supplies that default; do not use debate to silently approve a product change.

## Known integration issue to resolve

The current module gate hard-requires both billing and CMS, but CMS is P6. Propose an honest P2 execution path that proves the whole billing module and both consumers without fake CMS crates, hiding absent final requirements or weakening final readiness. Lead will own the gate implementation and document the distinction.

## Review exit

Independent proposals first, then one substantive critique exchange. Revise until concrete correctness objections are resolved or explicitly escalated to lead with alternatives and evidence. Lead writes one implementation contract; the developer must not pick arbitrary pieces from conflicting proposals.

No claims of compilation, test success, source completeness or provider verification without observed evidence. Plans are plans. No long dashes in documentation.
