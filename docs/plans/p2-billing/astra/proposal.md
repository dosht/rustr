# P2 billing: Astra independent proposal

Status: independent design complete, awaiting critique. This is not implementation approval. No builds, tests, formatters, provider transactions, secrets reads or subagents were used. Only this advisor's directory was written. The proposal deliberately preserves documented business defaults, including inconsistent policies; defect corrections are named rather than hidden inside refactoring.

## Recommendation

Use a pure billing crate, a small Postgres transaction adapter, one HTTP contract/router crate, two real gateway adapters and one explicit fake adapter. Keep the ledger authoritative. Serialize every account mutation on a durable billing account row. Plan billing from a snapshot inside that transaction and apply the resulting writes before committing. Complete payments and grant their packages in the same transaction; the outbox carries notifications after commit. Host job changes join that same local transaction through a narrow adapter seam, without billing reading host tables.

The two consumers are real compositions: taskr exposes a small billing demonstration with free usage, checkout and paid usage; Transgate exposes its actual billing catalog, quote, balance, history, invoice data, checkout, confirmation, recurring callback and cancellation services. P2 implements the billing policies used by transcript presentation and AI entitlement without implementing transcription or AI engines. Later shell flows stay unverified until those shells exist.

## 1. Scope matrix

All 123 billing-topic rule IDs are assigned below: 79 BIL, 25 FRE and 19 CPN. Ranges are inclusive. Runtime means an implemented policy/use case or real adapter, not a prose promise. Projection means a runnable pure policy with a host-consumed output, not a claim that its eventual web screen has been built.

Owners: C = `modules/billing/core`; P = `modules/billing/postgres`; H = `modules/billing/http`; I = Iyzico adapter; L = LemonSqueezy adapter; T = Transgate billing integration; K = taskr billing integration. C includes domain, application and presentation policies. Gateway identifiers are configuration and adapter bindings, never conditions in pure business rules.

| IDs | Runtime obligation or explicit exclusion | Owner and boundary | Evidence to implement |
|---|---|---|---|
| BIL-001 to BIL-009 | Package ownership, signed derived balance, inclusive windows, missing row versus zero | C, P; account-scoped reads | Domain boundaries and negative values; Postgres isolation and empty account |
| BIL-010 | Integer ceiling per file, then sum | C; host supplies measured duration | 61 seconds -> 2; ten 6-second files -> 10; checked overflow |
| BIL-011 to BIL-012 | Earliest-expiry allocation and carry values, explicit equal-expiry correction | C; P supplies snapshot | Exact rows, ties and negative-capacity counterexample in section 4 |
| BIL-013 to BIL-018 | All six original breakdown vectors, including shortfall and empty result | C | Six separately named assertions with exact carry and allocations |
| BIL-019 to BIL-022 | Account isolation, insufficient balance without writes, positive allocations, zero/no-package failure | C, P | Strict debit, rollback, no cross-account effect and exact zero |
| BIL-023 | One new typed debit use case; three legacy SQL overloads explicitly excluded | C, P | One entry contract; no legacy business function installed |
| BIL-024 | Full first-job debit against earliest open package | C | Overdraw and no-active-package cases |
| BIL-025 to BIL-033 | Eligible transitions, once-only start charge, null deferral, strict paid path and atomic job/debit | C, P, T host transaction port | Host contract suite for all transitions, sequential and split debits, rollback |
| BIL-034 to BIL-038 | Deferred strict charge, provider duration normalization, completion despite insufficient balance, no refund or extra charge | C, T | Callback decision suite and persisted failure receipt; web readiness projection |
| BIL-039 to BIL-041 | Distinct web/API preflight thresholds and error mappings | C, H, T | Negative/zero/positive balances, typed 402/500 and legacy mapping |
| BIL-042 | No automatic refund; hard deletion removes effective usage if host deletion succeeds | C, P, T | Failed/soft-delete keeps charge; atomic hard-delete contract; no refund endpoint |
| BIL-043 to BIL-045 | Completion grants once, metadata expansion and iterative calendar windows | C, P | Pending/failed -> completed, atomic grant, invalid interval rollback; DEF-004 change recorded |
| BIL-046 to BIL-049 | Monthly, one-year and 12-package legacy examples | C, P | Exact names, minutes and all start/end pairs |
| BIL-050 to BIL-054 | Five catalog products and prices | C | Catalog fixtures, every interval and price; PAYG hours validated |
| BIL-055 to BIL-061 | Renewals, plan identity, provider keys, monotone completion, confirmation and status mapping | C, P, I, T | First/repeated/out-of-order events; two subscriptions for one customer; transient lookup failure |
| BIL-062 to BIL-065 | Cancellation, next-payment projection, display renewal text, current plan | C, I, P, T | Owner checks, cancel operation recovery, paid minutes retained; 30/365-day copy and calendar date kept distinct |
| BIL-066 | Buyer and sales notifications on first completion, replacing trigger HTTP | C event, P outbox, T notification consumer | One logical event and two delivery intents; exact subjects and safe configured recipients |
| BIL-067 to BIL-068 | Durable checkout initialization and signed-in POST contract | C, P, H, I | Provider failure, persistence fault, retry ambiguity; GET redirect and no-session result |
| BIL-069 to BIL-071 | Subscription/PAYG protocol fields and result retrieval | I | Local HTTP request matching, authenticated bytes, success/error payloads; source constants treated as compatibility data |
| BIL-072 | Payment/subscription analytics intents and confirm countdown | C presentation/events, T | Status-change dedup, first-plan-only event and five-second view state |
| BIL-073 to BIL-074 | Balance text, refresh intent and response metadata | C, H, T, K | Missing/negative/59/60 minutes, 30-second timer intent and correct freemium facet |
| BIL-075 to BIL-076 | History/invoice model and authorized admin lookup | C, P, H, T | Newest completed first, TRAN padding, date/money formatting, latest billing info; 401/403/400/404/200 |
| BIL-077 | Disabled fraud rules explicitly excluded; no new fraud restrictions | C policy surface | Small-file sample remains allowed; no active fraud rejection |
| BIL-078 | Preserve historical canceled/unknown values without exposing arbitrary new gateway/currency strings | C, P | Import round trip versus validated new write |
| BIL-079 | Removed resume/partial rules explicitly excluded; no charge on completed -> not_started | C, T | Transition matrix and no dropped-column dependency |
| FRE-001 to FRE-002 | One free 20-minute, two-calendar-month package per signup, ordinary expiry | C, P, both hosts | Duplicate signup delivery, delayed handling anchored to user creation and expired grant |
| FRE-003 to FRE-006 | Database freemium predicate and no-other-completed-job bypass | C, T host facts, P serialization | Paid/future/expired packages; two starts before completion; no lifetime marker |
| FRE-007 to FRE-012 | Subsequent strict charging, exact exhaustion, negative balance and missing package | C, P, T | All pgTAP-derived examples, no mutation on rejection |
| FRE-013 to FRE-014 | Preserve separate historical-package and unexpired-paid predicates | C entitlement projection, T | Expired paid, future paid, multiple free rows and query-error fallback |
| FRE-015 to FRE-019 | Web batch 20/45 limits and exact exception lists; API strict deferral | C, T | List-set fixtures; first draft/first target only; 20, 21, 45, 46; unknown duration |
| FRE-020 to FRE-023 | Both transcript display cuts, fade, unlimited downloads and banner progress | C projection, T | Ordered line samples at 1260/1320 seconds, preview count, fade and progress; no server truncation |
| FRE-024 | Billing entitlement selects chat-limit policy; paid bypass, absent caps unlimited | C projection, T | Four configured caps and unparseable values; AI counters and races remain AI-015 to AI-019, not invented in billing |
| FRE-025 | Partial/resume feature explicitly excluded by PRD discovery; stale placeholder not ported | C transition contract | No remainder charge or partial-state capability; explicit negative behavior test |
| CPN-001 to CPN-004 | Locale currency, exact rate, quoted versus provider money and PAYG rounding | C, I, P | tr/other locales, rate 44, 6-hour TRY decimal, returned provider amount preserved separately |
| CPN-005 to CPN-007 | PAYG controls/range, locale discount and special-offer precedence | C, T eligibility port | Whole 5..9999, total-based locale discount, special TRY price bypasses coupon/conversion |
| CPN-008 to CPN-014 | Exact code/product match, percentage-only semantics, expiry and checkout reread | C, P, H | Null amount/expiry, exact expiry instant, incompatible product, repeated code and lookup error |
| CPN-015 to CPN-019 | Display/reference prices, analytics conversion, invoice VAT and special-client alert | C projections, T data port | Fixed badge/legacy display preserved pending decision, exact tax split, configured eligibility without copied customer IDs |

P2 evidence does not mark WEB-BIL rows passed just because a Rust projection is tested. API-BIL-001 to API-BIL-010 are actual billing boundary contracts, including replacing database reads and internal trigger hooks; API-BIL-011/012 are billing policy integration contracts whose full AI/transcription endpoints belong to later phases. EML-BIL-001 to EML-BIL-003 require payload and delivery-intent tests in P2, while actual production email delivery remains a host integration claim. EML-BIL-004 and WEB-BIL-018/019 are proposed exclusions already inventoried, not automatically approved drops: final parity checker requires the named mu-approved keep/throw row.

## 2. Crates, domain and interfaces

Dependency graph, arrows mean depends on:

```text
billing-core -> std, one pure calendar/decimal dependency if justified
billing-postgres -> billing-core, sqlx
billing-http -> billing-core, axum, serde
billing-iyzico -> billing-core, reqwest/rustls, signing/JSON
billing-lemonsqueezy -> billing-core, reqwest/rustls, signing/JSON
billing-fakes -> billing-core
 taskr-billing -> billing-core, billing-postgres
 taskr-server -> taskr-billing, billing-http, chosen gateway
 taskr-cli -> billing HTTP client contract
 transgate-billing -> billing-core, billing-postgres
 transgate-server -> transgate-billing, billing-http, both gateways
```

All module packages live below `modules/billing/`, with gateway crates in `gateways/`. `taskr-billing` lives under `crates/`; `transgate-billing` under `transgate/billing/`. No module imports `taskr-core`, including its BoxFuture alias. Use a module-local identical standard-library future alias; do not extract a universal kernel just for this. `taskr-core` stays untouched, dependency-free and I/O-free. No nested workspace, extra facade framework, general event bus or gateway plugin registry.

Key types and design signatures, not executable implementation:

```text
AccountId(stable UUID); PaymentId/PackageId/UsageId(stable signed 64-bit ID)
SubjectRef(host namespace, opaque stable ID)
Minutes(i64 signed); RequestedMinutes(nonnegative checked integer)
Instant(UTC microseconds); CalendarInterval(Month | Year)
Money(decimal coefficient + scale, Currency); Rate(exact decimal)
Package(id, account, payment?, sequence?, kind, original_name, included, start, end)
Usage(id, account, package?, subject?, minutes, charged_at, provenance)
BalanceSnapshot(has_open_packages, minutes, package_balances)
FreemiumFacets(db_no_open_paid, historical_single_free, no_unexpired_paid)
Quote(product, hours?, currency, rate_snapshot, coupon_reference?, applied_discount, amount)
PackageSchedule(count, interval, frequency, expiry, minutes, display_name, kind)
Payment(account, reference, quote, schedule, status, provider_binding, created_at, observed_at?)
Subscription(account, provider_reference, plan_binding, cancellation_state)
ChargeOutcome(Charged | Deferred | NotApplicable | Rejected(reason) | FailedDeferred(reason))
Plan(account_revision, request_digest, ledger_changes, payment_changes, packages, events, outcome)

balance(snapshot, at) -> BalanceSnapshot
breakdown(request, packages, at) -> Breakdown
plan_charge(snapshot, trusted_subject_facts, command, at) -> Plan
plan_payment(snapshot, VerifiedPaymentObservation, at) -> Plan
quote(catalog, coupon, eligibility, requested_order, at) -> Quote
entitlements(snapshot, package_query_outcome, at) -> FreemiumFacets
presentation.update(intent) -> effects; presentation.view() -> BillingVm
```

Signed i64 totals avoid smallint overflow; old job IDs and minute values are carried unchanged, never narrowed on import. New measured durations must be finite and positive before ceiling; zero strict deductions retain the documented failure. Negative historical rows are not accepted through a public credit/debit API. Use checked arithmetic rather than wrapping or saturating money/minutes. The calendar dependency is pure, has UTC explicit semantics, and must work under wasm; no clock reads occur in domain functions. Keep exact decimal scale through rate and discount calculation, quantize at documented steps and provider boundary.

Ports: `BillingStore` provides an atomic account operation, clock provides an injected instant, `PaymentGateway` performs initialization/result lookup/notification verification and subscription cancellation. `BillingIdentity` resolves an authenticated actor and owner/admin capabilities. `BillingAudience` supplies customer/special-offer/loyalty eligibility without putting names or emails in the reusable catalog. `BillingNotifications` receives typed committed events. Fakes implement the same gateway and notification ports under explicit test/development composition.

The gateway result has three states: definitive success, definitive failure, and unknown/retryable observation. Network failure is not a failed payment. Verified observations identify the merchant environment, order/payment and subscription, authoritative monetary values when available, and authenticated provenance. A browser redirect token or raw parsed JSON never constructs an observation directly.

HTTP contract: account balance/packages/entitlements, catalog, coupon/quote, checkout creation, payment status/history, invoice projection, cancellation and admin invoice lookup. Provider callbacks are separate raw-body routes. Account identity comes from verified authentication, never a body `user_id`. Client commands require an idempotency key; successful replays return the saved result. Cross-account identifiers return no other account's data. Provider callback routes use provider authentication instead of a browser session. Pure error variants map to 402/409/400/401/403/404/502/503 explicitly. Existing confirmation and subscription callback paths receive compatibility adapters; do not invent a fake legacy API-key system in P2.

A taskr user can invoke billing status, purchase a tiny sample package through hosted checkout, and consume demo work under its freemium limit. This is an additive billing sample command/use case, not an unexpected charge for creating normal tasks. The taskr slice must exercise checkout -> authenticated webhook -> grant -> debit -> changed balance, persist across restart, and reject a too-large strict debit. The Transgate runtime uses its full catalog and policies for real billing routes; tests invoke the same host service used by the router, not an unused dependency.

## 3. Atomicity, concurrency and recovery

### Account transaction

Every mutating entry point starts a Postgres transaction, ensures `billing_accounts` exists, then locks that account row `FOR UPDATE`. An empty account is still lockable. Account creation and signup delivery use a unique account ID and retry insertion conflicts. Under READ COMMITTED, load billing snapshots and host facts after acquiring the lock, not before it. Capture one clock instant for the accepted command; provider event and arrival times remain distinct data.

The Postgres adapter exposes `PgBillingTransaction` holding the actual sqlx transaction, account and revision. Its module operations read/write only billing tables. A host repository can temporarily borrow its connection to read/write its own tables, returning an immutable `SubjectFacts` value. This is adapter-level local transaction composition, not a core dependency on sqlx or a module join to host jobs. The host owns the transaction's final commit. Normal standalone billing commands use a wrapper that commits internally.

For StartJob: acquire account lock, load/lock host job rows in stable ID order, validate ownership and expected status, collect other completed-job facts, compute web batch guard once, compute all per-job billing plans against the evolving snapshot, apply plans, perform host compare-and-swap transitions, insert receipt/outbox, commit. Any failed CAS, insertion or domain rejection rolls the whole batch back. The host repository calls domain transition code; it does not decide freemium eligibility. Completion, hard deletion and any operation affecting the completed-job predicate follow the same account-first lock order. No raw caller-supplied completed count, skip flag, minutes override or precomputed plan is trusted at the HTTP boundary.

P2 implements this seam and a host contract suite using real Postgres host-fixture tables owned by the tests. It does not ship fictional job endpoints or claim P3 completion. Transgate's running checkout/history/cancellation services are its real P2 consumer; the billing side of StartJob/ProviderAnswered/DeleteSubject is a usable application contract with executable atomicity evidence. The later actual Job repository must satisfy the same suite before being wired. This separates a finished billing contract from an unimplemented transcript workflow.

Plans contain private validated changes and a snapshot revision, not public vectors accepted from an API. Applying requires the same locked account and expected revision. Every affected account/package/payment reference is revalidated by adapter constraints. The module owns commit effects and dedup semantics even when the host owns final commit. A plan computed before the lock is never accepted.

### Deduplication and conflicting requests

`billing_commands` has unique `(account_id, operation, idempotency_key)`, normalized semantic digest, outcome and completion time. The first committed outcome is replayed; a different semantic payload under the same key returns conflict without mutation. Store the typed canonical request plus digest, not a hash of JSON whitespace. Failed transaction attempts leave no successful receipt. Business rejection may be recorded as a committed no-op receipt; transport/DB errors are retriable and not a durable business rejection.

Charge identity additionally uses `(account, subject, charge_phase)` so a different client key cannot rebill the same start or deferred result. An initial null duration creates a deferred start receipt, not a debit and not a tombstone blocking its later callback. Completion with an insufficient deferred charge records `FailedDeferred` permanently for that phase. Replaying it after a top-up must not silently collect a new payment. Invalid or missing provider duration leaves the deferred phase unresolved; a valid later answer can resolve it. Shorter known duration updates the host duration without editing usage; longer duration does not collect more.

Webhook receipt identity and payment identity are different. Receipt uniqueness is `(gateway, merchant_environment, provider_event_id)` when supplied; otherwise authenticated object reference plus event kind plus stable delivery identity. Store a semantic digest and bounded audit metadata. Different whitespace is a replay; conflicting amount/account/subscription/plan claims are quarantined, never last-writer-wins. The same order can legitimately receive failure then success: those are distinct observations, not a key conflict. Payment uniqueness is `(gateway, merchant_environment, provider_order_reference)` and package grant uniqueness is `(payment_id, sequence)`. The first subscription charge must map initial checkout and order notifications to the same local payment, not create a second renewal.

### Payment and grant transaction

An authenticated observation resolves a saved payment intent/subscription, then locks its account. Completion is terminal against late failure; failure may recover to completion. First completion and all scheduled packages plus a single logical `PaymentCompleted` outbox event commit atomically. Invalid interval or any package insert failure leaves the payment uncompleted and the receipt unapplied so it can be retried after correction. Concurrent confirmation and webhook see the same completed result and grant once. A subscription renewal is keyed by that subscription and order, not the customer's oldest payment. Independent renewals may arrive in any order; each legitimate order grants once with its own schedule anchor.

This chooses synchronous grant over discovery's suggested asynchronous `GrantPackages` outbox consumer. It prevents a completed payment from temporarily having no usable balance and reduces recovery states. The outbox remains mandatory for external effects. A future split can move the same idempotent domain command behind an inbox without changing its identity.

### External work without a database transaction

Checkout first persists an internal intent with a server-computed quote and schedule and a unique operation reference. It then releases the transaction, initializes the gateway over HTTP, and reacquires the account lock to save provider references and a payable pending payment. Keep `intent_created_at`, `checkout_created_at` and `completed_at` separate. To preserve BIL-045 and BIL-067, anchor package windows to the successfully initialized checkout's payment-created instant, not automatically to the earlier intent time or later confirmation time.

If initialization definitively fails, record the operation failure and return the current checkout-error contract; it is not a completed/failed monetary payment in history. If the request times out after possible acceptance, mark initialization uncertain. Do not blindly repeat a non-idempotent provider POST. Reconcile by saved correlation/reference where supported; otherwise retain a reviewable uncertain operation and require a deliberate fresh checkout, with each old external payment still reconcilable. Conversation IDs are correlation, not a claim of provider idempotency. A crash after provider success but before saving its token is an explicit fixture case. Provider callbacks received before token attachment enter a verified inbox and retry resolution; an unknown token never causes a package grant by itself.

A small billing worker claims inbox/outbox/reconcile operations with leased rows using `FOR UPDATE SKIP LOCKED`, commits the claim, performs HTTP, then commits outcome under the account lock. Lease expiry permits recovery after crash. Retried external notifications are at-least-once; use a stable event ID at accepting sinks. Do not claim exactly-once email when a remote send succeeded but acknowledgement was lost. Missing live configuration fails startup for the selected gateway, never substitutes a fake. Expiry alone needs no cron. Do not auto-cancel all old pending payments or discard late valid success without a product decision.

Cancellation uses a persisted operation, owner authorization, an out-of-transaction Iyzico request and a subsequent confirmed state update. An unknown response stays cancellation-pending and can be reconciled. Keep existing packages; a renewal already charged before cancellation still grants. Marking cancellation complete before a provider acknowledgement is prohibited. Support notification is an outbox effect. Imported subscriptions missing a provider subscription reference retain a clearly identified unresolved cancellation binding, not a fabricated reference or oldest-customer match.

### Concrete races

- Two strict 15-minute requests against 20 minutes: first commits, second reloads 5 and rejects, with no partial job mutation.
- Two distinct first jobs with no completed jobs: both may qualify when serialized, because running is not completed. Web negative-balance preflight may reject the second request after the first overdraw; a single accepted batch is guarded once and preserves its per-job bypass. Direct database-eligibility contract tests still prove FRE-006. No one-use marker is introduced.
- Completion racing a new start: whichever holds the account lock first defines whether the other completed job exists. Completion facts cannot arrive on an eventual event stream if they are used for this decision.
- Duplicate payment success: one payment, one package set, one logical completion event. Failed then success grants once; success then failure remains completed. An authenticated new amount for an already completed order creates an audit conflict, not more minutes.
- Host hard-delete rejected by transcript FK/host rule: billing usage deletion rolls back too. Successful hard-delete removes effective usage for that subject, preserving the legacy balance effect without a cross-context foreign key.

## 4. Fidelity rules and explicit decisions

### Windows, calendars and negative balances

Open means `start <= t <= end`, including both adjacent monthly packages at their shared boundary. Do not add a tick or adopt half-open intervals. All calculations use UTC instants and a calendar operation which clamps an invalid day to month end. Generate each next start from the previous start, not from the original anchor: January 31, February 28, March 28 in a non-leap year. Each end is independently start plus expiry. Test leap day, a yearly step, midnight and non-UTC Postgres sessions. The next-payment display uses created plus count intervals, while the modal copy uses 30 or 365 days; these are separate source rules, not one date helper. [Billing discovery](../../../../transgate/docs/discovery/billing.md) records BIL-005, BIL-045, BIL-063/064 and DEF-012 to DEF-015; source file paths below are authoritative where a relative link is unsuitable.

The default strict allocator must preserve BIL-012's prefix formula except for the explicitly recorded equal-expiry defect: order by `(end_at, package_id)` and compute row-wise prefix totals. The bypass always chooses one earliest package using that same tie break. The balance still includes negative remaining minutes; never clamp the balance to zero.

A new source-derived counterexample needs lead review before contract finalization. With open package A remaining -16, B remaining 100, A ending first, a request of 10 yields prefix -16 then 84. BIL-012 assigns zero to A and 26 to B. The SQL return is 74 but the derived balance becomes 58. If A ends later, another anomalous allocation can occur after an earlier positive package. This follows from the final SQL, not an executed experiment: `/home/mu/Business/Transgate/frontend/supabase/migrations/20251230190000_fix_request_breakdown_user_id_again.sql:45-72` and `20260115162255_fix_freemium_first_job_bypass.sql:74-89`.

Until an explicit defect decision, implement a clearly named legacy prefix breakdown with this vector pinned, and expose actual derived post-write balance separately from the legacy arithmetic deduction return. Do not advertise an unconditional invariant that sum(deductions) equals request: it is true for ordinary nonnegative package snapshots, not this legacy case. Recommended correction if approved: keep signed aggregate balance for admission, allocate exactly requested minutes over positive remaining capacities, and leave historical debt on its original package. That changes the example to derived balance 74. Neither this correction nor historical debt forgiveness is silently authorized by the equal-expiry correction.

### Freemium and deferred work

Keep three named entitlement facets, with source semantics:

- Database bypass: no currently open non-freemium package, plus no other currently completed job.
- Web start/transcript display: exactly one package ever visible and its name is freemium, irrespective of dates.
- Chat/badge: no paid package with end >= now, irrespective of start; package query error counts as freemium on this path only.

Preserve original package name and a classified kind. Unknown imported names other than exact `freemium` remain paid for these predicates; SQL NULL-name behavior must be explicitly represented rather than accidentally treated as a new known plan. The current-plan box still recognizes only PREMIUM/BUSINESS. Manual grants do not become recurring subscriptions.

The web guard computes the batch sum after per-file rounding, applies the exact 48-locale/37-language sets, and consults only the first draft and first translation target. A change of draft order can change eligibility, so the host must preserve the source ordering contract instead of sorting for policy convenience; lock acquisition order and user-facing batch order are separate. Web readiness blocks unknown durations. API URL start needs a strictly positive balance; its later callback always charges strictly, regardless of completed count. Shorter provider answers update duration only; missing/zero answers do nothing. Soniox uses `metadata.duration_ms`, then maximum token `end_ms`, as directly read in `frontend/src/utils/soniox.ts:207-275`; the jobs topic's `audio_duration_ms` wording is not used to invent another field.

The transcript projection implements both load truncation (<=1260 seconds plus three preview lines) and display truncation (<=1320 seconds with last-three fade), keeps exports/downloads available, and caps banner progress at 100 percent. FRE-024 emits the four configured caps and whether checks/increments apply; it does not implement a second AI counter store. Production observed cap values are seed configuration, not universal module constants.

### Money, coupons and display

Keep charged-order arithmetic in one pure quote function, with an explicitly separate legacy display projection where the ledger documents a difference. PAYG: validate whole hours 5..9999, multiply by 1.49 USD, apply id/pt locale half-price floor at whole-total cents, round as source, apply matching unexpired percentage coupon and half-up cent rounding, then convert to TRY using the snapshotted exact rate. Special-offer eligibility takes precedence: TRY 14.99 per hour, rounded, no coupon, locale discount or rate conversion. Subscription prices remain provider plan bindings; four catalog prices are 14/21/120/180 USD and minutes 1200/2400 per month. Gateway returned charge and local quoted/calculated amount are different fields, so a TRY plan discrepancy is visible rather than fabricated as equality.

Do not store binary floating-point money. Keep exact decimals internally and legacy raw numeric values in migration records; the 6-hour TRY default evaluates to exact 393.36. Provider serialization quantizes to supported minor units using documented rounding. A non-integer configured FX rate can expose additional fractional cents, so pin that edge and record the rounding correction in DEF-011. Coupon percentage is an exact decimal fraction, not an integer-only percentage. A null amount is zero; discount_type remains preserved data and does not activate fixed/product discounts. Coupon validity is exact code/product match and now <= expiry, with null expiry unlimited. Checkout rereads it. No usage counter, minimum purchase beyond PAYG range, new case-folding or plan coupon is introduced.

Retain the reference to an existing unexpired but inapplicable coupon under CPN-013, while also recording `applied_discount=0`; dropping that reference is an optional data correction, not necessary to compute a correct price. Plan coupons never alter price. Unknown/expired code drops the checkout reference. Special-offer operation must not accidentally record a discount as applied.

Preserve the documented id/pt shown-total discrepancy and fixed Save %10 badge in the compatibility presentation until lead records the intended correction; proposed correction is to show the charged whole-total calculation. The 25/33-percent loyalty alert mismatch is likewise data/presentation, not permission to change a buyer's actual discount. Eligibility data comes through host configuration/import; no real customer identifiers enter test fixtures or this proposal. TRY invoice projection retains inclusive 20-percent VAT and exact subtotal/VAT sum; other Iyzico currencies retain zero displayed VAT. This is application parity, not tax advice or a new invoicing policy.

### Named corrections in the proposed contract

Lead should record `changed` rows with old/intended/decision and targeted tests for DEF-001/002 (atomicity and unauthorized writes), DEF-003 (verified renewals), DEF-004/005 (success recovery and unknown lookup), DEF-006/007 (authorized real cancellation and subscription identity), DEF-008 (durable initialization recovery), DEF-009 (server PAYG range), DEF-011/012 (exact money and UTC), DEF-016 (equal expiry) and DEF-030 (one logical completion event). These are source-documented defects with concrete failing examples. They do not authorize unifying freemium, moving windows, withholding an unpaid deferred transcript, altering refund policy, changing eligibility lists or removing historical rows.

The negative-prefix issue above is additional evidence for a new ledger decision. If lead considers it an unambiguous PRD-permitted defect, record that classification and the changed outcome before implementation; otherwise compatibility remains the default.

## 5. Gateway execution and fixture provenance

### Iyzico

Implement actual HTTP for hosted PAYG initialization/result retrieval, hosted recurring initialization/result retrieval, subscription detail/order verification and subscription cancellation. The source mappings in BIL-069 to BIL-071 govern legacy fields; endpoint and authentication fixtures use current official documentation. Keep callbacks based on configured public origin rather than trusting request Host. Hosted form content is untrusted provider HTML at a narrow shell boundary; core stores typed checkout action, not script execution logic. Placeholder buyer information remains an explicit compatibility concern, not proof of support for foreign cards.

The request signer uses HMAC-SHA256 over nonce + URI path + exact transmitted body; IYZWSv2 carries the base64 authorization fields and the request includes the matching nonce header. Freeze nonce in fixture tests and serialize the body once. Test GET/no-body separately. [Official authentication documentation](https://docs.iyzico.com/en/getting-started/preliminaries/authentication/hmacsha256-auth).

Subscription webhooks use the V3 subscription-specific field order and merchant secret, not the generic hosted-checkout signature formula. Normalize authenticated `subscription.order.failure` and legacy `.failed`. Because the signature does not establish an arbitrary amount supplied in JSON, resolve the saved subscription and authenticate order details before trusting money or plan changes. A missing signature is never silently accepted; a supported fallback requires a fully authenticated provider lookup of the exact order and matching subscription/account, not merely looking up a customer. [Official webhook documentation](https://docs.iyzico.com/en/advanced/webhook).

A subscription-detail lookup and cancellation are real documented endpoints. Persist the returned subscription reference from first checkout; later renewals bind to that reference. Source-only imported customer IDs cannot safely distinguish multiple subscriptions. If the provider response cannot establish exact order amount, retain an explicit unknown observed amount and use the saved subscription schedule only after order authenticity is proved; do not relabel the oldest payment amount as a verified charge. [Subscription transactions](https://docs.iyzico.com/en/products/subscription/subscription-implementation/subscription-transactions).

### LemonSqueezy

Minimal proposal: a one-off PAYG checkout with a configured non-recurring variant and custom price, allowing the same whole-hour selection rather than inventing fixed bundles. Do not sell a yearly subscription as a one-off package by accident. Checkout creation sends a private intent correlation, locks enabled variant and quantity to the expected purchase, disables provider-side discount entry where the core already priced a coupon, and requests test mode according to explicit adapter configuration. `custom_price` is a positive cent integer and store/variant relationships are required. [Create checkout](https://docs.lemonsqueezy.com/api/checkouts/create-checkout).

Verify X-Signature against exact raw request bytes with HMAC-SHA256 and constant-time comparison before interpreting an order notification. Reject missing/invalid signatures and test/live environment mismatch. [Signing requests](https://docs.lemonsqueezy.com/help/webhooks/signing-requests). Return success only after durable inbox capture or committed handling; provider retries are finite, so local durable recovery is required. [Webhook delivery](https://docs.lemonsqueezy.com/help/webhooks/webhook-requests).

A paid order binds its store, variant, quantity, currency and intent to the persisted quote; custom account data alone is insufficient authorization. Pin subtotal, discount, tax and total independently rather than requiring tax-inclusive total equal the pre-tax custom price. Retain the provider receipt link as data. No subscription, portal, license or refund workflow is added. [Order object](https://docs.lemonsqueezy.com/api/orders/the-order-object).

Currency routing, how to expose the new gateway and which invoice action the UI should show remain product choices. Implement explicit gateway selection in the application contract with capability validation and no automatic fallback from a failing Iyzico subscription. A configured USD PAYG LemonSqueezy test path proves the new capability without inventing a country routing policy. An Iyzico-specific TRY special offer must not silently become a USD LemonSqueezy purchase.

### Required fixture bundle

Each adapter owns `fixtures/manifest.json`, small synthetic request/response JSON and raw webhook-body files. Every manifest entry states: source URL or exact repository file/line, retrieval date, provenance (`documented-example`, `source-derived`, `synthetic-edge`, later `sandbox-recorded`), redactions/substitutions, HTTP method/path/status, expected normalized output, and content hash. Documentation examples are not called live recordings. No personal data, real bearer values, plan secrets or customer IDs are copied. Source plan reference codes are configuration bindings; synthetic equivalents suffice for protocol fixtures.

Local HTTP fixture tests must instantiate the production reqwest adapter against a loopback server and inspect actual outbound headers/body, not just deserialize JSON. Cover checkout creation and lookup for both Iyzico modes; Iyzico V3 success/failure and subscription cancellation; LemonSqueezy JSON:API checkout and signed paid order; invalid signatures, wrong merchant, wrong currency/variant/account correlation, malformed JSON, non-2xx, timeout, duplicate delivery and failure-to-success recovery. Use an independently calculated fixed signature vector as well as mutation tests so a shared wrong signer/verifier cannot validate itself. Capture fixture-server observed requests as proof during implementation.

Fakes are deliberately selected by name in tests/dev. Real adapter selection with missing configuration or provider failure must fail clearly, never return a fake success. Fixture evidence establishes local protocol behavior; sandbox credential validation, live store currency/tax behavior and international-card success remain unverified operational facts, explicitly stated in the P2 milestone.

## 6. Storage and migration

Use one `billing_` table prefix with module-owned migrations. SQL contains constraints/indexes and read-only projections, no grant, coupon, allocation or lifecycle business triggers. A dedicated sqlx migration-history table avoids colliding with taskr's existing unprefixed `_sqlx_migrations`: do not invoke two unrelated migration folders against the same history with overlapping version IDs. Lead chooses the supported sqlx migrator configuration or application migration orchestrator before adding production migrations.

| Table group | Essential fields/invariants |
|---|---|
| billing_accounts | Stable user UUID, revision, lifecycle; lock row independent of packages |
| billing_payments | Historical numeric ID, account, quote/observed money, original metadata, dates, status, gateway references; scoped unique non-null provider references |
| billing_subscriptions | Stable internal ID, account, environment/provider reference, plan binding, cancellation operation/state |
| billing_packages | Historical ID, account, included minutes, UTC windows, original name/kind, optional payment, grant sequence; unique payment/sequence and one new signup grant/account |
| billing_usage | New writes only, positive checked minutes, account/package composite FK, subject and phase, charged time, operation receipt |
| billing_legacy_usage | Original historical IDs/nullable fields/nonpositive values/owner anomalies, immutable import provenance; no runtime INSERT permission |
| billing_coupons, billing_info | Preserved IDs and original fields, exact discount fraction and filter; invoice identity retains nullable owner and unused Paraşüt contact field as data |
| billing_commands, billing_inbox, billing_outbox, billing_operations | Scoped dedup keys/digests, typed outcome, lease/retry times, external reference and bounded sanitized audit fields |

Use a shared/reserved usage sequence above the maximum historical ID so imported and new usage IDs never collide. The read repository projects legacy and new usage together, preserving legitimate negative values and null linkage behavior. Only a separate import role may insert legacy rows; runtime roles have no ability to route new invalid input through that path. New package/payment links enforce same-account ownership, even though import preservation must retain old malformed relationships separately. An import manifest maps exceptional raw payment/package rows to preserved source records without pretending they satisfy new invariants.

A migration-compatible projection is a deliberate design task, not blanket trust in imported ownership. Historical cross-account usage affects the legacy SQL grouping itself; report both raw legacy derivation and normalized owner-based derivation. If they differ, keep all raw rows and mark affected accounts as parity exceptions requiring a disposition before production cutover. Do not erase rows, silently mint credits, or claim zero-mismatch parity. Seeded anomaly tests exercise this without waiting for a production snapshot. Runtime requests cannot opt into legacy grouping or imported provenance.

Copy all payment/package/coupon/billing-info data and usage IDs, raw metadata, token/customer/plan fields, nullable fields and original status. Interpret naive payment created timestamps as UTC. Preserve unknown enum/name values in import types without accepting them in new commands. Copy canceled timestamps with their original precision; do not invent a time lost to the old date column. Map existing grant sequence deterministically from original packages, reporting duplicate/missing sets; do not automatically backfill packages for historically completed-but-ungranted payments. Future verified recovery of such a payment requires an explicit import/reconciliation disposition so it does not unexpectedly grant another set.

No foreign key from the billing module to taskr/Transgate jobs, auth schema or other bounded-context tables. The host passes stable identity and subject references. Hard-delete behavior is implemented by an explicit same-transaction host call; account deletion contract deletes payments/packages/usage and detaches billing_info, preserving PRV-005/006/007 defaults. P2 supplies the billing-side operation and test, not the whole P3 identity deletion saga. A terminal account tombstone with minimal routing identity can prevent late webhook resurrection; privacy retention of that routing identity is a cross-context decision, not permission to keep payment personal data indefinitely.

Seed migration tests include manual grants, future/expired packages, duplicates, unknown names, canceled payment states, imported nonpositive or cross-account usage, pending checkouts and customers with multiple subscriptions. The report compares counts, sums, IDs and balances at one fixed T, plus package windows and exact rows. Equal-expiry allocation differences are named defect exceptions, never balance mismatches suppressed wholesale. The negative-prefix allocation vector remains separate. Live snapshot import, delta synchronization and final provider dashboard/callback validation are P9/cutover evidence, not a condition for developing the whole P2 module.

## 7. Verification and evidence gates

No verification below has been run by this advisor. The implementing developer first writes failing domain tests, then implements the rule. Lead registers evidence only after observing the intended tests pass.

| Layer | Required concrete observations |
|---|---|
| Domain | All 123 IDs mapped; exact six breakdown vectors; ordinary debit conservation; legacy negative-prefix exception; ties; inclusive boundaries; iterative months; quote/coupon/entitlement/presentation fixtures |
| Postgres | Independent connections synchronized with barriers, two strict deductions and two first starts, completion race, grant/confirm race, cross-account denial, injected failure halfway through allocations/package grants and host CAS rollback |
| Recovery | Crash before/after external call, lease expiry, callback before checkout token save, same-key changed payload conflict, deferred failure replay after top-up, duplicate outbox claim and retry |
| HTTP | Real router authentication, 402/409/errors, webhook raw-byte authentication, customer owner checks, admin lookup permission, legacy callback adapters |
| Gateway | Production HTTP adapter against loopback fixture server; request bytes/signature vectors, both checkout forms, retrieval, cancellation, paid order and failure responses |
| taskr consumer | User-created free grant -> strict sample usage -> rejected overuse -> checkout -> signed fixture webhook -> paid balance -> additional usage; restart proves persistence |
| Transgate consumer | Full catalog and quote -> checkout/confirm -> history/invoice -> recurring order -> cancel; proper entitlements and current-plan output; duplicate callback leaves balances unchanged |
| Host transaction contract | Same production billing planner/adapter with host-owned Postgres fixture rows proves job status/debit atomicity and completed-count ordering; no P3 completion claim |
| Import | Seeded legacy row preservation, fixed-time minute parity, anomaly classification, UTC conversion, ID/sequence continuity; no fabricated snapshot result |
| Boundary | Resolved transitive Cargo graph rejects all module-to-host paths and requires both consumers; behavior smoke proves more than linkage |

Use exact ledger references of the existing format, for example `cargo:billing-core:rules::bil_005_inclusive_end` and `cargo:billing-postgres:concurrency::two_strict_debits_serialize`. Real test names are fixed in the implementation contract and checked against emitted Cargo `test NAME ... ok` lines. Each rule gets at least one specific behavioral assertion; shared table-driven tests are acceptable if named cases still identify covered requirements. Multi-layer obligations use `; `-separated references. No `cargo:...:all`, ignored test, zero selected test, comment-only test or existence-only assertion is evidence.

P2's explicit rule inventory is exactly BIL-001..079, FRE-001..025 and CPN-001..019, with cross-topic contract tests additional rather than claimed as whole later-topic completion. Preserve the 117 billing pgTAP assertion links in `pgtap-map.md`; map weak legacy tolerances to exact Rust assertions and record the removed resume placeholder as excluded legacy knowledge. Do not claim to have run the old SQL tests from mapping them.

Lead must resolve two checker issues:

1. Add an explicit phase selection such as `gate-modules.sh --module billing` and corresponding checker arguments. Default `gate-modules.sh` still requires billing and CMS. The selected command requires all billing crates, both runtime consumers, mandatory Postgres database availability, adapter fixture proof and named smoke tests. It also runs dependency checks over every existing module so selecting billing cannot hide an illegal CMS dependency. Unknown/empty selection fails. Report `P2 billing module gate passed; final module gate still fails absent P6 CMS` accurately.
2. `assemble-discovery.py::lint_rules` currently rejects any source status other than `ledger`, while `check-prd.py::coverage` accepts only `tested`/`changed`. Extend valid lifecycle statuses without weakening executable evidence. Source topic rows are updated first, canonical rules/parity and pgTAP map are regenerated and integrity-checked. A billing-only ledger mode selects the exact declared IDs and rejects missing/duplicate/unverified rows; it must not select merely whichever rows already have tests. Full ledger/parity mode remains strict about every unfinished phase.

Checker regressions must cover absent billing, absent CMS in final mode, fake consumer dependency with no behavior evidence, renamed/transitive host import, missing DB, skipped Postgres test, unknown module selector, source/generated drift and a `changed` row lacking a decision reference. Current checker does not enforce the last condition; add that rather than relying only on the status label. Lead owns script changes, root manifests/lockfile and final evidence, not the OMP developer independently editing gate semantics.

P2 cannot mark full WEB-BIL desktop/phone parity green from VM tests. Those rows remain inventoried until P5 renders the live steps. Likewise all final P3/P4/P6/P9 requirements remain visible. This is phase-scoped completion of billing rules and contracts, not weakened final readiness.

## 8. Ordered slices and ownership for one developer

| Slice | Developer-owned files | Frozen contract and stop condition |
|---|---|---|
| 1. Pure rules | modules/billing/core/src/{domain,policy,presentation}, core tests | Typed minute/money/time values, exact scope matrix, named defaults and selected defect decisions; failing tests before rules |
| 2. Transaction storage | modules/billing/postgres/{src,migrations,tests} | Account lock and PgBillingTransaction seam; private revision-bound plan; isolated migration history; rollback/concurrency proof |
| 3. Payments and operations | core application code, postgres operation/inbox/outbox code | Durable checkout intent, payment/order identity, atomic grant and cancellation state; no HTTP inside tx |
| 4. Gateways | modules/billing/gateways/{iyzico,lemonsqueezy}, modules/billing/fakes | Real HTTP adapters and documented fixture manifests; identical normalized results; explicit fake-only selection |
| 5. HTTP and hosts | modules/billing/http; crates/taskr-billing; transgate/billing; narrow server and CLI composition edits | Authentication/identity conversion at hosts, both behavior smokes, no taskr-core dependency and no new P3 engine |
| 6. Import contracts and ledger evidence | module import/test fixtures; discovery/billing.md evidence cells when lead delegates | Every ID named, every pgTAP obligation retained, synthetic migration parity and anomaly report |

Lead owns root Cargo.toml/Cargo.lock, scripts/check-prd.py, scripts/gate-modules.sh, scripts/assemble-discovery.py, generated ledgers, milestone/book/WORKING-ON updates and final gate execution. The developer requests small integration patches at slice boundaries; it does not invent incompatible package names or edit another writer's files. No branches/commits, optional refactors or framework extraction are part of these slices.

Before the developer starts, lead publishes one reconciled implementation contract stating: negative-prefix allocation decision; exactly which documented defects are authorized corrections; P2 projection versus final shell evidence; gateway capability defaults; transaction ownership and migration-history strategy. An advisor sketch is insufficient authorization for a business change.

## 9. Unresolved business choices only

Most discovery questions already have a zero-change default: inclusive windows, iterative month stepping, checkout-created anchor, distinct freemium definitions, repeated bypass before another completion, delivery after failed deferred charge, no new refund, display-only cuts and unrestricted downloads. They should not block P2 unless mu wants to change them.

The genuinely unresolved choices are:

1. Negative-prefix allocation: preserve the source overcharge behavior or approve exact-request debit after an overdraft. Section 4 supplies a concrete source-derived counterexample and proposed correction.
2. LemonSqueezy product exposure: approve one-off PAYG with custom price, its currency/store bindings, and the customer-facing gateway selector. No automatic subscription fallback is proposed. Until selection is decided, both gateway implementations and an explicit test-mode PAYG path can be completed.
3. LemonSqueezy billing history: choose provider receipt action versus a locally rendered invoice. Core retains receipt and tax data so implementation does not fabricate an Iyzico VAT invoice for a merchant-of-record order.
4. Display-only defects: approve matching id/pt display to existing charged amount, actual savings badge and loyalty alert wording. The legacy projection remains the default until decided.

Operational unknowns are evidence limits, not business questions: Iyzico sandbox/test-store credentials, live subscription reference/price mapping, merchant webhook signature setup and the production snapshot. They do not justify a fake adapter, invented rows, or stopping pure/domain/Postgres/protocol work. International-card success cannot be promised from a local fixture.

## Reading record and limitations

Read: the brief; PRD 1-5 and 7-8 (plus scenario boundaries); progress board/P1 milestone; billing/storage/concurrency portions of thoughts; modules README; complete billing discovery in targeted sections including rule, defect, parity and migration tables; cross-ledger jobs/API/AI/privacy/data-model references; checker/assembly scripts; targeted existing server/Postgres source. Source checks covered the current allocation/deduction migrations, Soniox duration handling and checkout amount ordering. Official gateway references were consulted on 2026-10-24. No claim is made of whole-source inspection, compilation, running tests, provider acceptance or production parity.

Additional discovery inconsistency for lead: JOB-008 still says INSERT-to-not_started bills, while billing's final trigger inventory and BIL-025 say UPDATE only. The proposal follows the final trigger definition and requires reconciliation of that cross-topic row before eventual jobs evidence is marked green. This is a source correction, not permission to charge inserts.
