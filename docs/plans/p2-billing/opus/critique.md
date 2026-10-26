# P2 billing: Opus critique of the Astra proposal

Written 2026-10-24 after reading `astra/proposal.md`, `integration-notes.md`, `gateway-evidence.md` and `decisions.md`. Nothing was compiled or run. Source checks done for this critique are cited inline. Where I concede, my own proposal (`opus/proposal.md`) is superseded on that point.

## 1. Defect allowance (PRD section 2) versus preserving paid-no-packages

Concession. My proposal read the brief's "preserve unless mu approves" as covering every outcome change and kept DEF-004. The lead's reading in `integration-notes.md` (PRD section 2 permits clearly established defect fixes, recorded as current, intended and decision) is the correct one, and "a paid transaction receives its packages exactly once" is the lead's own example. Adopt the DEF-004 fix.

What both proposals must pin, or the fix creates a new defect:

- Grant on the observed transition into `completed` (from `pending` or `failed`) inside this system, never on the state "completed and has no packages". Counterexample: production has completed payments from `bank` and `Iyzico IBAN` (`transgate/docs/production-facts.md:29`), and support-made packages without `payment_id` (billing.md Research). A rule keyed on "completed without packages" would grant again to those accounts on any reprocessing or reconciliation. Astra already says no automatic backfill; the contract should state the trigger is the transition.
- Historical DEF-004 victims stay as they are; a later duplicate callback for an already `completed` row is not a transition and grants nothing.

Classification I propose for the combined contract (lead decides): clearly established and fixed in P2: DEF-001, DEF-002, DEF-004, DEF-009, DEF-012, DEF-016, DEF-030, D1/DEF-034, plus the owner check half of DEF-006 and the transport-error half of DEF-005. These change nothing for a customer using the product as offered, or they stop a customer from losing money or minutes. Not clearly established, so current behavior stays: DEF-003 full verification mode at cutover (section 6), the gateway-call half of DEF-006, DEF-007 as stated (section 6), DEF-008 for Iyzico (section 7), DEF-011 as "store the gateway amount" (section 2), and every Needs mu item.

## 2. Exact money versus legacy f64

Concession. Freezing `393.35999999999996` into storage, as I proposed, preserves a float artifact that no business rule asked for. Astra's exact decimal in the core is the better runtime model.

Objection to Astra as written: "the 6-hour TRY default evaluates to exact 393.36" is asserted, not proven, for the whole input space. The legacy expression is `floor((p - p*0.5)*100)/100` and `Math.round(x*100)/100` over JavaScript doubles (`checkout/index.tsx:288-313`, CPN-004, CPN-006). Where a double lands just below an integer cent before `floor`, legacy charges one cent less than exact arithmetic. I could not hand-prove such an hour count exists in 5..9999, so this is a risk, not a counterexample. It must be settled by evidence, not by argument.

Combined rule:

- Runtime: exact decimal (`i64` coefficient plus scale, or minor units), quantized half up to the currency's 2 decimals at the provider boundary and in storage. Non-integer configured rates are allowed and quantized the same way.
- Oracle: the legacy JavaScript formulas (special offer, locale discount, `round2`, coupon, TRY conversion) run in local Node once during implementation and produce a golden file for hours 5..9999, locales `en`, `tr`, `id`, `pt`, coupon fractions {null, 0.1, 0.25, 0.33}, rate 44. A Rust test asserts `exact == round_half_up_2dp(legacy)` for every row. Any mismatch is a named rule difference in the ledger with its hour counts, never silently accepted.
- Coupon `amount` is Postgres `real`. Today it reaches JavaScript as float4 shortest text (`0.1`). Read `amount::text` and parse as an exact decimal. `amount::float8` gives `0.10000000149011612` and would change prices.
- DEF-011 split: quantizing the TRY amount to 2 decimals is a representation fix (the stored and sent value differs by less than half a cent, and the oracle proves it). "Store the gateway amount instead of USD x rate" for TRY plans is a data addition only: keep `amount` as today's quote and add a nullable `observed_amount`. Astra already keeps them separate; agree.

## 3. Negative-prefix conservation (lead D1)

Concession. I claimed my allocator reproduced the SQL exactly and did not see this case. Checked against the final function (`20251230190000_fix_request_breakdown_user_id_again.sql:45-72`): `take_i = max(0, min(r_i, req - S_{i-1}))` where `S` is the signed prefix including the synthetic request row. With A at -16 ending first and B at 100, a request of 10 writes 0 on A and 26 on B. Astra's vector is correct.

Correction to the D1 wording. D1 says the fix is "not debt forgiveness". Over time it is, in the customer's favor:

| Moment | Legacy | D1 |
|---|---|---|
| After the 10-minute charge, A still open | A -16, B 74, balance 58 | A -16, B 90, balance 74 |
| After A (the free package) expires | B 74, balance 74 | B 90, balance 90 |

Legacy collects the overdraft from paid minutes, but only when a strict charge happens while the free package is still open. If the free package expired first, legacy forgives it too. So legacy is timing-dependent and double counts the debt during the overlap window. D1 is still the right fix (one rule, no double count, the charge equals the request), but the ledger row must state both numbers and that the overdraft now lapses with its package in every case. This is an outcome change worth showing mu in the milestone note even though the lead may decide it under section 2.

Invariant to add to D1's evidence: the signed aggregate is the positive capacities plus the negative ones, so aggregate >= request implies positive capacity >= request. Admission on the aggregate followed by exact allocation over positive capacities can never run out midway. One property test covers it.

D1 with row-wise `(end_at, package_id)` ordering also removes the DEF-016 peer behavior. The contract should treat them as one allocator change with two ledger records.

## 4. Same-transaction grants and outbox

Agreement: completion, all packages and one `PaymentCompleted` outbox row commit in one transaction; external effects run after commit from the outbox. Both proposals already say this.

Objection to Astra's extra machinery for this path:

- The `Plan` carrying an account revision and request digest adds nothing when the plan is computed and applied inside one transaction that holds the account row lock. Nothing else can change the account between plan and apply. Drop the revision from the plan; keep the account lock.
- `billing_inbox` for "callbacks received before token attachment": Iyzico renewals create their own row (no prior token), the current integration uses no HPP webhook for first checkouts, and LemonSqueezy correlates by our `payment_id`, which exists before the checkout is created. There is no flow in P2 where a callback can precede its correlation key. Exception in section 6 (a minimal unprocessed-notification record for Iyzico renewals).

## 5. Host atomicity and batch ordering

Partial concession. Astra is right that a race-free first-job decision needs job completion to serialize with job start on the account lock. With my "job rows then account" order and no account lock on completion, a start can read zero completed jobs while job X is being completed. Both lock orders work if every path that changes the completed-job predicate (completion, hard delete, provider callback completing a job) takes the account lock before commit. That obligation goes into the P3 host contract and into the host contract suite now.

Choose one global order. I recommend Astra's account first, then host rows in ascending id, because completion paths then naturally start with the account lock and nobody has to reason about which row was locked first.

Batch rules for the contract:

- The web guard reads the first draft in the user's submitted order (FRE-018). Lock order and charge order are separate from that and use ascending job id, so they are deterministic.
- All jobs of a batch are charged in one transaction and any refusal rolls back the batch (today one UPDATE statement covers the batch).
- The host passes facts (`other_completed_jobs`, current status) read under the lock. Agree with Astra that nothing from the HTTP body (counts, skip flags, minutes) is trusted.

Objection: Astra's `billing_commands` idempotency table, required idempotency keys on client commands, and the `(account, subject, phase)` charge receipt are new API and data requirements. Job start is already once-only through the job status compare-and-swap (BIL-026), checkout duplicates are harmless today (two forms, two pending rows), and recording `FailedDeferred` is new data. None is needed to preserve or fix any listed rule. Leave them out of P2.

## 6. Authentication and subscription identity

Objection with counterexamples.

1. Signature required at cutover. `gateway-evidence.md` says the merchant must ask Iyzico to enable `X-IYZ-SIGNATURE-V3`, and nobody knows whether production has it. Astra rejects a missing signature unless a full provider lookup authenticates the order. If signatures are not enabled and the lookup cannot find the order (shape of `orders` in the subscription response is unknown; `subscription-callback.ts:37` types it `any[]`), every real renewal is rejected. Iyzico retries three times at 15 minutes (`gateway-evidence.md`), then stops. Paying subscribers silently lose their monthly packages.
2. Binding renewals only by subscription reference. Existing payments store no subscription reference (only `customer_id` and `pricing_plan_id`). Every subscriber at cutover would be "unresolved binding" and get nothing on renewal.

Combined rule:

- Verification mode is explicit configuration, required at startup, never defaulted silently: `signature` (V3 header required, published sample formula pinned, its prose inconsistency documented) or `lookup` (retrieve the subscription with our API credentials; require its `customerReferenceCode` to equal the body's, and require the order reference in the subscription's orders when that field is present in the response). Cutover default is `lookup` until mu confirms signatures are enabled.
- Because provider retries are finite, persist each renewal notification's identifiers and verification outcome before answering. A notification that fails verification or processing answers non-2xx and stays visible for manual reprocessing. This is the one place I accept an inbox-like table, and it needs no worker or lease.
- Identity: add `subscription_ref` to payments; capture it at the initial confirm (the subscription checkout form result carries the subscription `referenceCode`, sample at `checkout/confirm.tsx:257-268`) and on every renewal. Match a renewal by `subscription_ref`; when none is bound yet (all legacy subscribers), fall back to today's oldest payment by `customer_id` and bind the reference on that renewal. This fixes DEF-007 for new and second-plan customers without breaking existing ones.
- Event types: accept `subscription.order.success`, `subscription.order.failed` (source) and `subscription.order.failure` (docs). Recording an extra `failed` row changes nothing a customer sees (history lists completed payments only).
- Open evidence item for both of us: does Iyzico send `subscription.order.success` for the first order of a new subscription? If yes, today's handler inserts a "renewal" row with a different token and grants a second set of packages at signup. That would be current behavior with a money effect, so P9 must query production for renewal rows created within minutes of an initial plan payment before anyone changes it.
- Cancellation: owner check now (both agree). Calling the Iyzico cancel API, and Astra's "cancellation pending until provider acknowledgement", change what the customer sees (today the account page immediately says the subscription is canceled, BIL-062) and how support works. Keep current behavior plus the owner check; the gateway call is a mu decision.

## 7. Minimal recovery without new policy

Objection to Astra's durable checkout intent, uncertain-initialization state and leased worker for Iyzico checkout. DEF-008's premise does not hold: the form is returned to the browser only after the payment insert succeeds (`checkout/index.tsx:247-250`), and the insert throws on error (`dao/Payments.ts:14-19`). A failed insert after Iyzico init leaves an orphan token that no customer ever sees. Keeping today's order (Iyzico init with no transaction open, then insert) needs no recovery machinery and preserves BIL-067 (no row when Iyzico fails) exactly. Astra's intent rows would add a row for every attempt and need their own cleanup policy, which is new policy.

Minimal recovery contract:

- Transport error on confirm leaves the payment `pending` (DEF-005, narrow fix). A definite non-success answer still marks `failed`.
- With DEF-004 fixed, a later definite success grants once.
- A manual admin use case `reconcile_payment(payment_id)` re-runs the confirm logic with the stored token. No scheduler, no automatic sweep, no change to the 4354 legacy pending rows.
- LemonSqueezy persists the pending payment first (needed for correlation) and marks it `failed` on a definite checkout-creation error.
- Outbox delivery is at least once, consumers record delivered ids. Both agree.

## 8. SQLx schema history

The lead resolved the API (`integration-notes.md`): `dangerous_set_table_name("billing._sqlx_migrations")` plus `create_schema` in sqlx-core 0.9.0. Given that, I recommend a Postgres schema `billing` over Astra's `billing_` prefix in `public`:

- A split is one schema dump, grants are per schema, and the history table lives beside the tables it describes.
- Every migration and query must schema-qualify (`billing.packages`, `billing.payment_status`) and never rely on `search_path`, because taskr's connections use the default path.
- Tests share the fixed schema, so isolation is per test account id (random uuid), never truncation, matching the lead's rule. Concurrent test binaries running the migrator rely on sqlx's migration lock, which must stay enabled. The coexistence test the lead asked for: run the taskr migrator and the billing migrator twice each against one database, both succeed, two history tables.

Legacy anomalies: Astra's separate `billing_legacy_usage` plus a shared sequence is heavier than needed. One usage table with `origin` (`runtime` or `import`) and `check (origin = 'import' or minutes >= 0)` keeps imported negative or odd rows for minute parity while new writes stay checked.

Counterexample to Astra's "positive checked minutes" for new writes: the bypass inserts the requested minutes as given (`20260115162255_fix_freemium_first_job_bypass.sql:68-69`), so a freemium first job with duration 0 writes a 0-minute row today. A `> 0` check makes that job start fail. Use `>= 0`.

## 9. Honest P2 gate versus P6

Concession: Astra's `gate-modules.sh --module billing` is better than my separate script, because the selected mode still runs the dependency check over every existing module and an unknown selector fails. Default `gate-modules.sh` keeps requiring billing and CMS.

Database tests: the lead requires that adapter tests never return success without a database. Two ways to satisfy that without breaking `cargo test --workspace` on a machine with no database: (a) mark Postgres tests `#[ignore = "needs BILLING_TEST_DATABASE_URL"]`; the billing gate runs those packages with `--include-ignored` and fails if the URL is missing. libtest prints `ignored` otherwise, which `check-prd.py` already refuses as evidence, so there is no false green in either mode. (b) Tests panic without the URL, and the workspace test command needs a database from now on. I recommend (a); the lead chooses because it touches the checker.

## 10. taskr slice

Concession: Astra's slice (an additive taskr billing sample in `crates/taskr-billing`, `taskr-core` untouched, normal task creation not charged) is less invasive than mine, which changed `TaskListRepository::save` and made the 11th task answer 402. Adopt Astra's slice. I would drop the `taskr-cli` billing client from P2: the server path plus one HTTP test proves scenario 5 and the CLI adds a shell for no rule.

## 11. Recommended combined contract

1. Crates: `billing-core` (pure, rules and presentation policies, module-local BoxFuture), `billing-postgres`, `billing-http`, `billing-iyzico`, `billing-lemonsqueezy`; fakes behind a `testing` feature or `billing-fakes`, selected only by name. Hosts: `transgate/billing` and `crates/taskr-billing`; `taskr-core` untouched.
2. Storage: schema `billing` with its own history table; ids kept; `origin` column with the `>= 0` runtime check; `payment_seq` unique per payment; `subscription_ref` and nullable `observed_amount` on payments; no triggers that decide.
3. Locking: READ COMMITTED; account row lock first, then host rows in ascending id; every path that changes the completed-job predicate takes the account lock. No gateway HTTP inside a transaction. The opener commits.
4. Allocation: admission on the signed aggregate; exact allocation over positive capacities ordered by `(end_at, package_id)`; bypass on the earliest open package with the same tie break; six breakdown vectors unchanged; D1 and DEF-016 recorded with the before/after numbers in section 3 of this critique.
5. Payments: grant on the transition into `completed` (from `pending` or `failed`), all packages and one outbox event in the same transaction; late `failed` never undoes `completed`; Iyzico checkout keeps today's order; LemonSqueezy persists first; transport errors leave `pending`; manual `reconcile_payment` only.
6. Money: exact decimal runtime, 2-decimal half-up at the boundary, Node golden oracle over the full input space, coupons read from float4 text.
7. Renewals: explicit `signature` or `lookup` mode, persisted notification records, `subscription_ref` binding with the legacy `customer_id` fallback, three event names.
8. Kept as today pending mu: window inclusivity, iterative months, checkout-created anchor, three freemium definitions, completed-job eligibility, deferred delivery after a failed charge, no refunds, display-only cuts, `id`/`pt` shown total, `Save %10`, loyalty text, manual cancellation at Iyzico.
9. Gate: `gate-modules.sh --module billing`, Postgres tests run for real in that mode, scoped ledger check over exactly BIL-001..079, FRE-001..025, CPN-001..019, final gates unchanged.
10. Corrections to route to discovery owners: data-model keep/throw row throwing the three live Iyzico columns, JOB-008 versus BIL-025, DEF-008's premise.

## 12. Points where I hold my position and expect a response

- Section 6.1 and 6.2: signature-required verification and reference-only binding break every current subscriber at cutover. I need either evidence that signatures are enabled and that the subscription lookup returns orders, or acceptance of the explicit-mode and fallback rule.
- Section 7: DEF-008 is not a customer-visible defect for Iyzico, so the intent and uncertain-init machinery is not justified in P2.
- Section 5 objection: `billing_commands` and client idempotency keys are new contract requirements with no rule behind them.
- Section 3: D1 must be described as lapsing the overdraft with its package, with the table numbers, not as "no forgiveness".
