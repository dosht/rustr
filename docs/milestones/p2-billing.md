# P2: billing backend checkpoint

Status: in progress. The backend checkpoint below was exercised on 2026-10-24 before the P3 account-erasure extensions. It is not P2 completion or proof of the current evolving P3 workspace.

## What was built

Five reusable billing packages provide pure rules, PostgreSQL transactions, HTTP and Iyzico/LemonSqueezy adapters. Both taskr and Transgate consume them through host-owned transactions and restricted runtime logins. Ordinary task creation stays unmetered; the explicit taskr sample demonstrates package purchases and paid/freemium work.

Independent Opus/Astra proposals and reviews converged before implementation. Decisions D1-D5 record allocation, measured price corrections, authenticated completion, manual initial-order alias resolution and provider uncertainty: [decisions](../plans/p2-billing/decisions.md). No guessed Iyzico order alias or fabricated provider success is used.

## Exercised evidence

| Check | Observed result |
|---|---|
| Database-backed workspace, `--all-targets -- --include-ignored` | 454 tests passed across 39 test binaries before P3 changes |
| Actual taskr and Transgate server processes | Both passed against separate disposable databases, restricted logins and real loopback provider/auth/mail/analytics services; restart, duplicate delivery and graceful shutdown exercised |
| Unconfigured Transgate runtime and Docker | Health/readiness distinction, invalid/occupied bind, unknown route, non-root/read-only container, healthcheck and stop passed |
| Resolved billing dependency graph | Five packages, both consumers, no transitive host dependency; graph check only |
| Discovery and gate regressions | Generated discovery current; 13 Python regressions passed |

Detailed scenarios, integration failures and their fixes: [lead integration notes](../plans/p2-billing/integration-notes.md). Named behavioral tests: [implementation evidence](../plans/p2-billing/implementation-evidence.md). The book's existing porting chapter now explains the transaction, provider-observation and delivery boundaries.

## Why P2 remains open

The canonical ledger records 103 tested rows, 14 explicitly changed rows and six incomplete whole rows. BIL-068/072/073/075 still need browser behavior in P5; FRE-022 needs actual downloads in P3; FRE-024 needs chat enforcement/bypass in P4. Tested backend subsets do not certify those remaining clauses. No final gate or rendered parity row is promoted by this checkpoint.

Strict Clippy found small cleanups after the test pass; those were applied, but a final clean run awaits coherent P3 integration. P3 adds account erasure and must rerun the affected billing tests. Live payment/provider acceptance, production credentials/configuration, hosted CI and migration remain unproved. The Docker check did not exercise configured billing inside the container.

## Next owner

The lead owns integrated verification, canonical evidence and completion gates. Visible P3 owners implement core, persistence, transports, processor and both hosts; independent review findings return to the same owners. P3 is implementation work now, not a fresh discovery restart.


## Final scoped acceptance, 2026-10-28

This checkpoint supersedes the open status above. All 123 canonical billing rows are settled: 108 tested and 15 explicitly changed. BIL-072 now has executable desktop and phone evidence for accepted and rejected analytics consent, actual checkout navigation, SDK delivery and once-only intent events. The Google Analytics omission follows P5 D12.

Independent OMP verification ran `scripts/gate-p2-billing.sh` against its own disposable PostgreSQL database and completed with zero failed checks, including workspace consumer builds. The new billing analytics and analysis export browser specifications passed all eight desktop/phone cases. Receipts: `/tmp/rustr-codex-lead/final-verification-20261028/billing-gate.log` and `browser.log`. The current-source tracked manifest at verification start is `5570f8497bea420e8394022667ac9c7344fd61ab9a03de0ee87f85a7fc5a29fd`; this is an uncommitted working-tree verification, not a new release commit.

P2 is accepted within the agreed scope. Live provider, production configuration and migration acceptance remain with P9.
