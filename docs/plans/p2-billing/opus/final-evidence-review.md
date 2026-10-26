# Final evidence review: BIL-001..049, FRE-001..025

Read-only, 2026-10-24. Checked each row's meaning in `transgate/docs/discovery/billing.md` against the tests mapped in `implementation-evidence.md`, the assertions in those tests and the code they exercise, under decisions D1 to D5. No code, test or ledger edit; nothing run. Rows not listed below have mapped tests whose assertions prove the row's outcome.

## Gaps to fix before marking rows verified

### 1. BIL-008 / BIL-073: an account with no open package shows "-" instead of "0 min" (lost source outcome)

- Source: `frontend/src/dao/Balances.ts:12-17` returns `data?.balance || 0`, so a user with no balance row is displayed as `0 min`. `Balance.tsx` shows `-` only when the fetch result is `null`, which that DAO never returns.
- Now: `billing-core/src/app.rs` `Billing::overview` sets `balance_text: balance_text(bal)` with `bal = balance(&open)`, which is `None` for no open package, so the overview says `-`.
- Why tests stay green: `ledger::tests::bil_008_...` only checks `balance(&[]) == None` and `balance_or_zero(&[]) == 0`; nothing checks what the overview shows.
- Fix (Sonnet, core): keep `balance: None`, render the text from `balance_or_zero(&open)`; add an overview test for an account whose only package expired (`balance: None`, `balance_text: "0 min"`). `balance_text(None) == "-"` stays correct for an unavailable read.

### 2. BIL-043: a historical `canceled` payment now grants packages on completion (withdrawn)

Withdrawn after lead review: `decisions.md` "Review clarifications" (first bullet) already approves a newly authenticated completion granting once, including recovery from a canceled record; I missed that clarification. The lead will make the D3 bullet consistent. No runtime change. The Postgres test was renamed to `bil_043_058_an_authenticated_completion_recovers_an_imported_canceled_payment_and_grants_exactly_once`. The original finding is kept below for the record.

- Source: the confirm and renewal updates can move `canceled` to `completed`, but the package trigger fires only for `OLD.status = 'pending'`, so a canceled-then-completed payment granted nothing.
- Now: `grants::transition(Canceled, Completed)` returns `Apply { grants: true }`, pinned by `grants::tests::bil_043_...` ("a canceled row can still complete, once") and by `billing-postgres` `bil_043_058_an_imported_canceled_payment_follows_the_source_and_grants_once_...`.
- D3/DEF-004 authorizes grants for `pending` or `failed` to `completed` only. The test name's "follows the source" is not accurate for the grant. Production has one `canceled` payment.
- Fix: lead either records that DEF-004 also covers `canceled` (and the row becomes `changed` with that reference), or `transition(Canceled, Completed)` moves the status without granting. Either way, rename the Postgres test so it doesn't claim source parity for the grant.

### 3. Rows whose outcome changed under a decision must be `changed`, not `tested`

The mapped tests prove the new behavior, which is right, but the ledger status must carry the decision:

| Row | Changed by | Note |
|---|---|---|
| BIL-011 | DEF-016 (D1) | equal `end_at` ties now ordered by package id, row-wise |
| BIL-012 | D1 / DEF-034 | the prefix formula is replaced by exact allocation over positive capacities; carry values now run over positive capacity. BIL-013 to BIL-018 keep their captured numbers (all capacities positive) and are plain `tested` |
| BIL-043 | DEF-004 (D3), plus gap 2 | failed to completed now grants |

BIL-042 is restored source behavior (`tested`), not a change.

### 4. Tests that cannot prove their row (tautological or wrong layer)

| Row | Test | Problem | Suggestion |
|---|---|---|---|
| FRE-022 | `freemium::tests::fre_022_downloads_are_never_blocked` | asserts that a function returning the constant `true` returns `true`; no download path exists in P2 | leave FRE-022 unverified until the P3 export/download endpoint has a freemium test, or record it as a P2-untestable presentation constant |
| FRE-024 | `freemium::tests::fre_024_...` | the cap parsing is meaningful, but `chat_limits_apply(x)` is the identity function, so "only freemium (app definition 2), paid users bypass checks and increments" is not proved | drive it from `is_freemium_app2` cases (expired paid, future paid, query error); the check and increment bypass itself belongs to P4 (AI-015 to AI-019) |
| FRE-019 | `freemium::tests::fre_019_url_jobs_have_no_web_guard_function_input` | an absence-style test (contract item 10); the second mapped test, `charge::tests::fre_019_the_deferred_charge_never_uses_the_first_job_bypass`, does prove the row | drop the first reference |

### 5. Mis-attributed Postgres references (low)

- BIL-033 cites `billing-postgres` `bil_028_033_034_036_...`, whose deferred charge of 10 draws from the free package alone, so it never draws from two packages. The core test proves BIL-033, and `bil_001_002_003_...` draws from three packages in Postgres. Drop 033 from that test's reference, or cite `bil_001_002_003_...` instead.
- BIL-031 cites `bil_030_031_...`, whose no-package case is a freemium first job (`NoActivePackage`, the FRE-012 path). BIL-031's paid-path `No available package` rollback is proved only at decision level. Acceptable (same rollback path), but add an account with a completed job and no open package if the Postgres reference is meant to cover BIL-031 itself.

## Outside the requested range, noticed in passing

BIL-060: `grants::tests::bil_059_060_...` still asserts `observation_from_status("INIT_THREEDS") == Failed`. Since D5 the Iyzico adapter decides itself (`SUCCESS` gives success, `FAILURE` gives failure, anything else is uncertain, `client.rs` around line 298), so the core assertion pins a legacy mapping the runtime no longer applies. Drop that reference for BIL-060 (and the assertion, or the helper if unused), cite `billing-iyzico:bil_060_only_success_and_failure_are_final_payment_statuses`, and mark BIL-060 `changed` under D5.

## Checked and adequate

BIL-001 to BIL-010, BIL-013 to BIL-024 (the six captured breakdown vectors exact; BIL-021 exactly zero; BIL-022 no-package case now `NothingDeducted` as in the source), BIL-025 to BIL-030, BIL-032, BIL-034 to BIL-042 (BIL-042 including cross-account, signed and concurrent cases), BIL-044 to BIL-049 (iterative clamped stepping, interval error rollback, checkout-created anchor), FRE-001 to FRE-018 and FRE-020, FRE-021, FRE-023, FRE-025.
