# P2 billing implementation contract

Status: approved for implementation by lead after both independent proposals, reciprocal critiques and round-2 closure. Independent proposals are evidence and alternatives, not additional requirements. This contract and `decisions.md` resolve conflicts; agreement is not verification.

## Deliverable and ownership

Implement all 123 BIL/FRE/CPN rules in `transgate/docs/discovery/billing.md`, with explicit recorded defect corrections, plus actual Postgres, HTTP and provider adapters and two working host consumers. No P3 job engine, P5 rendered billing screens or P6 CMS is claimed. Pure presentation policies belong in the billing core now; later views render them.

Developer owns new `modules/billing/core`, `postgres`, `http`, `gateways/iyzico`, `gateways/lemonsqueezy`, `transgate/billing`, `crates/taskr-billing`, and narrowly necessary existing taskr-server/transgate-server source and crate-manifest composition. No changes to taskr-core, ordinary task creation, existing task repositories or shell billing UIs. taskr's sample is additive HTTP billing behavior, not a new restriction on adding tasks. A CLI is unnecessary for P2 runtime proof.

Lead owns root Cargo.toml/Cargo.lock, gate scripts/checkers, CI, Docker integration, canonical discovery ledgers, board, milestone and book. Developer writes `docs/plans/p2-billing/implementation-evidence.md` with exact rule-to-test references, source decisions, new package names and handoff commands. Request integration edits via lead, do not silently take ownership.

Shared checkout only. No branches, worktrees, commits, production calls, provider transactions, env/secret reads or source Transgate edits. No hidden Task agents. Use the process-local OMP model roles; existing Opus/Astra visible sessions provide review. Write domain tests before their implementation. Do not run integrated builds/tests/formatters while source edits are in flight; hand off complete source and test names so lead can run one integrated pass and send targeted failures back. Never report unexecuted tests as passed.

## Crates and domain boundary

- `billing-core`: pure types, rules, use cases and presentation policies. A module-local BoxFuture alias is fine. No host dependencies or I/O. Pure, established decimal/calendar libraries are allowed; do not build a custom financial or Gregorian calendar framework. taskr-core remains dependency-free.
- `billing-postgres`: SQLx 0.9, schema-qualified queries and migrations. Use schema `billing`, history `billing._sqlx_migrations`, `create_schema` and `dangerous_set_table_name` as documented in integration notes. Keep migration locks enabled.
- `billing-http`: Axum 0.8 contract/router and identity/admin boundary, invoking core use cases. Caller identity, trusted catalog and provider observations are never accepted from client-claimed account ids, amounts, completion counts or bypass flags.
- `billing-iyzico`, `billing-lemonsqueezy`: real reqwest/Rustls HTTP adapters, typed errors, raw-byte webhook verification and source/documentation-derived fixtures. No actual provider account is available; never label synthetic fixtures as captured transactions.
- Fakes: deliberately selected test adapters, behind a testing feature or dedicated fake module. Missing production configuration never selects a fake. Local real-adapter protocol fixtures must be usable for the smoke.
- `transgate-billing`: captured catalog, freemium lists, host configuration and composition. Provider plan codes, admin/special-customer lists and seller data are host configuration/data, not generic-module constants. No private customer lists or credentials copied from source.
- `taskr-billing`: a minimal separate sample account with 10 free credits, a metered sample operation, one paid package through LemonSqueezy and a signed order webhook. Consume 10, reject the next operation without a write, buy a package, then succeed. Normal tasks stay unchanged.

Use stable UUID-shaped account ids, signed i64 minute balances, stable numeric imported payment/package/usage ids and UTC microsecond instants. Preserve null/unknown historical names and gateway strings without treating them as new valid checkout choices. Preserve all four captured payment statuses, including `canceled`; the legacy `payment_status` enum does not admit arbitrary status strings. A future snapshot outside that captured domain is an explicit migration discrepancy, not permission to invent new runtime states. No module depends transitively on any taskr or Transgate crate.

## Transaction protocol

Account row lock first, then host aggregate rows in ascending stable id, then payment/subscription rows as needed. Explicit READ COMMITTED. Every operation that changes available packages, usage or the host completed-job eligibility predicate participates in the same account serialization. Host facts are loaded after locking, never supplied by an untrusted request.

The transaction opener owns commit/rollback. Provide a concrete Postgres transaction/connection integration seam so P3 can compose job status CAS and billing writes in one transaction without billing querying job tables. All-or-nothing batch charges and host status changes must be exercised with a minimal host table now. Source `JobRepository.ts:123-129` explicitly orders drafts by ascending id: the web language guard uses that first draft, not request-array order. Charge and lock order are ascending id too. Test both request-array permutations with the same language decision.

Compute and apply a pure allocation inside the locked transaction. No account revision/request-digest layer is needed for a plan that cannot escape this unit of work. No provider HTTP or email delivery while any database transaction is open. No generic client-command idempotency requirement or Iyzico checkout intent table. Host CAS protects one job start; callback/deferred deduplication must be an explicit host contract and proved by a repeat-delivery test, not assumed from an unguarded function.

taskr's additive sample has no Job CAS: its host owns a stable sample-work reference and persisted result. Under the account lock, lock/CAS that work row and debit in the same transaction; retries return the saved result without another charge, and changing the work payload conflicts. No generic command table is needed. Legacy checkout duplicate submissions remain separate purchases; do not add optional retry keys in P2.

Payment completion, all grant packages and one completion outbox event commit together. Unique payment sequence grants and provider order identities are database backstops. Completion is a transition from pending or failed; already-completed historical rows are not automatically backfilled. Late failure cannot undo completion. Distinct failure and later success observations for one order must not be collapsed into a single already-seen event.

The outbox delivers external effects after commit, at least once, with stable event ids. Claim rows in a short transaction, release before external delivery, then acknowledge; concurrent consumers must not both own an active claim. Do not hold a row lock across HTTP. No claim of exactly-once email delivery.

## Preserved policy and approved fixes

`decisions.md` is the outcome-change register. D1 fixes signed-prefix overcharge and equal-expiry row allocation together. Check the signed aggregate first; allocate exactly the requested minutes over positive capacities in `(end_at, package_id)` order. Negative balances remain effective until package expiry. The post-expiry 74-versus-90 outcome is recorded, not hidden. First-job bypass still writes the full amount to the earliest open package and can overdraw. Preserve its zero-minute write; strict zero-minute deduction still produces the captured no-deduction error.

D2 uses exact decimal arithmetic with captured cent-floor/half-up stages and explicit legacy differences. Preserve the legacy charged-versus-displayed distinction, coupon product matching/case/type behavior and coupon-id retention when an existing coupon does not apply. Read imported float4 coupon values through shortest decimal text, not float8 widening. Historical money is not rewritten. Quote and verified observed provider amount are separate.

Keep inclusive package endpoints, iterative clamped calendar stepping, checkout/payment-row creation anchor, all three freemium facets, completed-job eligibility (not lifetime-use eligibility), web 20/45-minute guard, strict deferred URL billing, delivery despite failed deferred charge, no automatic refunds, hard-delete usage release, display-only transcript cuts and unrestricted downloads. No automatic expiration/cancellation sweep for historical pending payments. No early frontend/web parity claims.

Fix clearly established security and integrity defects: serialized deductions, exclusive runtime ledger writer, authenticated cancellation/admin access, V3 renewal verification, exact-once completion grants including failed-to-completed, transport uncertainty not falsely recorded as payment failure, stable expiry allocation, server enforcement of advertised whole hours 5..9999, UTC instants and one event per transition. Preserve manual Iyzico cancellation workflow pending mu's decision on calling the provider; do not pretend the stamp proves a provider cancellation. New subscription binding must never guess the customer's oldest plan.

## Checkout, renewal and provider boundary

Iyzico checkout: call init without a DB transaction, then persist the accepted payment/token before returning the form. Insert failure returns no payable form. Preserve BIL-067. A transport error during confirmation leaves status unchanged and reports uncertainty; a definite provider outcome drives the state machine. A manual authenticated admin reconciliation reuses the same confirmation flow, no automatic scheduler or blind retry of checkout POST.

Use IYZWSv2 exactly as the installed SDK and `gateway-evidence.md` describe. Serialize once, sign and transmit the same JSON bytes. GET without a request model signs `{}` per the installed SDK. Implement actual hosted checkout initialization/retrieval and subscription initialization/retrieval/detail operations. No obsolete SHA1 fallback unless primary current protocol evidence requires it.

Renewal verification is fail-closed V3 using trusted merchant id/secret and the published subscription sample formula. Accept source `.failed` and documented `.failure` event names alongside `.success`. Signature covers identifiers/type, not arbitrary body amounts. No lookup fallback that checks merely customer or subscription existence. Missing/invalid signature cannot create a payment, binding or grant.

Store the subscription reference returned by initial confirmation, its trusted account/plan/package specification and relevant provider identity. Verified notifications without a trusted binding are persisted unresolved and acknowledged only after durable storage, never granted by guessing a customer/oldest payment. Provide authenticated listing and replay after a verified binding exists; do not expose a client API to invent bindings. V3 merchant enablement and legacy binding mapping are explicit P9 cutover prerequisites.

First-order webhook versus checkout confirmation must not silently create duplicate entitlement. Provider protocol evidence must establish a common order identity; if not available, hold ambiguous first-order notifications unresolved with an explicit reason, rather than invent a successful renewal or claim live verification. Durable unresolved events are the bounded notification store, not a general leased inbox framework.

LemonSqueezy: persist pending payment/quote before hosted checkout creation; correlate through custom_data but validate against trusted stored account, gateway, environment, store/variant, quantity, currency and amount. PAYG only, hosted checkout plus order webhook, no subscriptions/customer portal. Use custom_price cents for arbitrary supported hours. Do not double-apply provider coupons. Verify X-Signature against raw bytes in constant time. Separate subtotal, discount, tax and total; don't compare a pre-tax quote to tax-inclusive total. Receipt URL is distinct from the existing invoice presentation. Definite creation failure records failed; transport uncertainty is not a false provider rejection.

## Storage, authentication and runtime

Use one `billing` schema, no business triggers, account indexes and composite ownership checks for new writes. Preserve historical anomalies as explicitly imported data; do not normalize them invisibly. If using an origin marker, runtime DB permissions must prohibit setting/changing it, not merely omit it from the HTTP DTO. A separate import role owns those writes. Runtime usage allows zero only for the captured bypass. Sum consumption by referenced package; do not silently discard cross-account legacy usage to make normalized balances pass. P9 must retain raw ownership/reference anomalies and report exact source-versus-normalized differences. Prove runtime cannot insert a negative/import-origin row.

Retain the three live Iyzico fields identified in the corrected discovery ledger. Add provider environment/order/subscription identities needed to avoid cross-provider, cross-environment or duplicate grants. Avoid speculative tables without a concrete failure path in this contract.

Mount real billing routes in both servers, with authenticated account scoping and admin authorization. Reuse existing taskr identity behavior through a host adapter. Transgate must have a real configurable JWT verification path and explicit dev identity only when deliberately enabled; no anonymous bearer-string-as-user fallback. Keep identity outside billing business rules. Preserve process-liveness health and distinguish it from billing readiness/configuration. State any necessary runtime-mode change to lead so the foundation smoke and Docker setup remain honest.

## Required implementation evidence

Developer supplies exact executable test references for every BIL/FRE/CPN row. One meaningful test can cover several rows; do not add tautological construction, copied-constant, forwarding or bare-not-throw tests merely to increase row count. Presentation policy tests exercise decisions and transitions; HTTP rows exercise requests and authorization; Postgres rows exercise actual transactions.

Minimum hard cases: six original allocation vectors; equal expiry; D1 signed capacities before and after expiry; conservation over mixed capacities; strict exact-fit/rejection without writes; two concurrent paid debits; two first jobs preserving captured bypass eligibility; account isolation; host rollback and whole-batch rollback; repeated start/deferred delivery; failed deferred charge still committing delivery facts; freemium provisioning race; completion/grant rollback; failed-to-completed and late-failure ordering; duplicate confirmation/renewal; zero-minute bypass; non-UTC session; leap years, Jan31 clamping and microsecond boundaries; migration histories coexist/repeat; cross-account ledger misuse rejected.

Money proof uses independent exact expectations and classified legacy differences from `money-evidence.md`, not a false all-equal oracle. Gateway proofs use local TCP fixture servers and independent known-answer signature vectors; reject wrong signature, wrong owner/order/store/environment/currency/amount, missing binding and conflicting duplicate payloads. Cover malformed/failed provider envelopes, timeout/disconnection and no automatic fallback to a fake.

Postgres tests use `BILLING_TEST_DATABASE_URL`, marked ignored for ordinary offline runs if necessary. When explicitly run they must require the variable and fail if unavailable. Tests isolate random account ids, never truncate shared tables. Lead's disposable database is documented in integration notes. All module and named-evidence gate runs include ignored tests.

Handoff must allow lead to run both real servers against the disposable Postgres and local provider fixture servers, drive the taskr sample and Transgate provisioning/checkout/confirmation/renewal/history flows over HTTP, inspect committed balances/packages/events and exercise rejected paths. Tests alone do not replace this runtime smoke.

P2 phase gate is `BILLING_TEST_DATABASE_URL=... ./scripts/gate-p2-billing.sh`. The final module gate remains red for absent CMS; full Transgate parity and live-provider/P9 prerequisites remain separate. No stub or skipped path is reported as P2 implementation.

## Consumer composition closure

The actionable review in `astra/implementation-review-notes.md` is part of acceptance, not a claim about unfinished implementation:

- Trusted host customer lookup by id/email must work after the checkout JWT is gone. Buyer/sales/support recipients and eligibility are never copied from posted identity fields. Mount actual admin customer history lookup. Wire purchase and cancellation templates to a runnable configured delivery adapter; a logging sink is not delivery. A local HTTP mail fixture is sufficient evidence without live sends. Do not acknowledge the event while a required destination failed.
- Both binaries use their actual identity configuration. Preserve real UUID subjects; map taskr's explicit dev subjects deterministically in the host. Test expired/invalid tokens, cross-account access and non-admin lookup through the composed runtime.
- Runtime connects using the restricted database login, not the migration owner. Migration/import authority is separate. Permission tests and consumer smoke must use that effective runtime authority.
- Compare a sample-work request's immutable payload before returning a previously saved result. Replay returns the saved result, not a newly calculated balance.
- Freemium windows start at trusted signup/account creation time, not first billing access or JWT issued-at. Duplicate provisioning cannot replenish. A service-authenticated provisioning route needs a real host invocation path; delayed authenticated first access must obtain the trusted creation instant or refuse, never invent a new trial at now. Exercise January signup with April first access and duplicate delivery.
