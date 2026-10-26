# Astra critique and recommended combined P2 contract

Status: complete, awaiting response. This reviews the Opus proposal and the lead's integration notes, gateway evidence and D1 decision. It also incorporates `../opus/critique.md` and `../opus/response.md`, received during review. Those responses supersede several original Opus positions; disagreements below distinguish closed points from remaining objections. No implementation, builds, tests, formatters, provider calls or agents were run.

## 1. PRD defect allowance and paid-without-packages

Resolved: Opus now accepts the PRD section 2 exception. Its original proposal sections 0, 3.2 and 9 preserved a known paid-without-packages defect by requiring the old state to be pending. That restriction must go.

Counterexample: a renewal records a definite failure, then the same authentic order succeeds. The original proposal moves failed -> completed, emits no completion event and grants nothing. The same outcome follows a transient first-confirm lookup failure followed by success. This is an established defect, not an alternative subscription policy.

Combined rule: a genuine observed transition from pending or failed into completed atomically updates payment, grants the complete package schedule and appends one completion event. Completed is terminal against later failure. Retrying a completed payment is a no-op even if no package links exist. I accept Opus's warning about historical manually funded/completed payments: never interpret `completed && no linked packages` as authorization to backfill a grant. Preserve imported anomalies and report them separately.

Lead should record the selected corrections in the contract, rather than make the developer infer authorization from either advisor's defect list. Explicit Needs mu defaults remain intact. The same reasoning supports equal-expiry correction, owner checks and transport-error-as-unknown. Whether provider cancellation replaces the current manual support operation is addressed separately below.

## 2. Exact money, with an honest legacy oracle

Resolved: Opus now accepts exact decimal runtime arithmetic. A decimal string created after f64 arithmetic only freezes its errors; it does not make the calculation exact. In particular, using f64 again for VAT or discounts defeats the original goal.

Lead has now measured the difference in `../money-evidence.md`: Node v26.2.0, 319840 combinations, 3824 differences, maximum 44 minor units. At 54 hours with the locale discount and no coupon, legacy charges USD 40.22 versus exact USD 40.23; after TRY conversion these are 1769.68 versus 1770.12. This is a real charged-price change, not sub-half-cent serialization. My 6-hour 393.36 example was not proof over the input space. I recommend exact decimal pricing only with a recorded lead decision naming these changed outcomes; preserve all historical amounts. Keep the legacy comparison as a differential fixture and independently test rational expectations. Do not require a false full legacy-equals-exact oracle. Non-integer FX rates and boundary coupon fractions need additional synthetic cases.

I also accept reading old coupon `real` values through their source-compatible float4 decimal text, then parsing exactly. Casting float4 to float8 first manufactures extra digits and may change the quote. Preserve raw imported money. New quotes use checked exact arithmetic, source operation order and explicit rounding stages. Keep quoted amount, snapshotted rate and nullable observed provider amount separate; replacing a historical TRY plan quote with an unobserved charge is not a fix.

The oracle is evidence, not a second runtime pricing implementation. Transgate owns catalog prices, plan bindings, lists and eligibility data; the reusable core owns the quote algorithm and typed policy input. I withdraw my proposal's implication that Transgate's concrete catalog and lists belong inside the generic module.

## 3. D1, equal-expiry ties and zero-minute bypass

Resolved: lead D1 supersedes my original preserve-negative-prefix recommendation. The runtime allocator first checks the signed aggregate, then allocates exactly requested positive minutes across positive capacities ordered by `(end_at, package_id)`. Zero/negative capacities still affect admission but cannot cause another debit. It returns the actual resulting derived balance.

The -16 then +100 example must produce one 10-minute row and balance 74, not a 26-minute row and balance 58. Reversed expiry order, mixed zero/negative packages, rejection without writes and the six original positive-capacity vectors are mandatory evidence. The proof obligation is simple: signed aggregate >= request implies positive capacity >= request, so admitted strict allocation can finish without overdrawing a positive package.

I accept Opus's correction about later expiry. After the negative package expires, legacy leaves balance 74 and D1 leaves 90. The negative package ceases to affect balance under the retained window rule. That customer-visible difference is now explicit in the updated lead decision; no persistent debt-transfer or historical rewrite is introduced.

Opus's original equal-expiry reproduction is superseded too: two equal-expiry packages of 10 must not reject a request of 5 or write only 10 for a request of 15. D1 and DEF-016 are one allocator implementation with distinct decision records.

I concede the zero-minute bypass objection. The final five-argument function inserts the supplied bypass duration without a positive-row filter, whereas the strict path filters out zero allocations and then raises if none were written. Therefore new storage must allow zero for this bypass, while rejecting negative new usage. A plain `minutes >= 0` constraint is necessary but insufficient: domain and adapter validation must permit zero only for the explicit bypass outcome, not turn strict zero into a success. My original universal positive-only usage constraint is withdrawn.

## 4. Atomic grants, outbox and transaction ownership

Resolved: both proposals persist completion, all grant rows and one logical completion event in the same transaction. This is simpler and stronger than first committing completed and later granting from an outbox event. A failure on package 12 rolls back payment and packages 1..11. The outbox is for effects after commit.

I concede that a stored account revision is unnecessary when planning and applying occur entirely within one correctly locked transaction. Use a private plan accepted only by that locked account unit; do not expose apply-plan as an HTTP operation. No detached/offline plan application is needed in P2. A semantic payload comparison for replay/conflict is a separate concern from optimistic revision control.

The host opens/commits a transaction when it needs atomic host changes. Billing exposes a narrow adapter unit over the same connection, reads only its own schema, computes business outcomes in the core and never commits a host-owned transaction. On ordinary billing-only flows, the billing service owns the transaction. No module depends on host crates or tables.

The relay must not hold a transaction or row lock while calling a mailer or analytics HTTP endpoint. Opus's original `FOR UPDATE SKIP LOCKED` description needs that qualification. A short claim transaction and bounded lease/retry fields on the outbox are enough; a generic worker framework is unnecessary. At-least-once external delivery is the honest guarantee: a send can succeed and its local acknowledgement can be lost. A table of sent IDs alone does not eliminate that ambiguity.

## 5. Host atomicity, first-job races and the actual batch order

Resolved: one global order is account first, then host rows in ascending ID, then other mutable billing rows in a consistent order. READ COMMITTED snapshot reads occur after acquiring the account lock. Completion, deletion and every change affecting the completed-job predicate must follow the same protocol.

Counterexample to the original Opus ordering: Start reads other-completed count as zero, then waits for the account lock held by Completion. Completion commits a completed job. Start finally acquires the lock but uses its earlier zero count and incorrectly bypasses. Serializing debit alone does not serialize eligibility. Facts must be reread under the lock, not merely passed through a transaction object.

New source correction to both critiques: the legacy first draft is NOT the first ID in the HTTP request. `frontend/src/dao/JobRepository.ts:123-129` queries drafts with `.order("id")`. `transcription.ts:408-409` uses `draftJobs[0]`. Therefore the combined contract sorts the eligible draft snapshot by ascending ID before selecting the language/first translation target. My prior warning to preserve an unspecified source order is resolved, and Opus critique section 5's submitted-order rule is wrong.

Concrete counterexample: draft 10 is English, draft 20 Turkish, combined duration 30 and UI locale en. A submitted array `[20, 10]` still selects draft 10 in today's query and fails the exception guard. Using submitted order would allow it. Tests must cover both permutations and retain the same result. Charge/lock order is deterministic ascending ID; the web guard runs once over the batch, not once per job after each debit.

Two separate cases must not be conflated:

- Direct database eligibility: two 36-minute first jobs can both bypass while no other completed job exists, producing -52 from the 20-minute free package.
- Public web requests: the 20/45-minute guard and negative-balance preflight still apply. A subsequent web request after the first overdraw may fail. A 72-minute combined web batch fails even with an exception. Passing direct eligibility tests cannot prove that this batch is a valid user flow.

Retain all-or-nothing batch mutation. A third unaffordable job rolls back the first two status changes and ledger rows. The host-fixture Postgres contract suite proves this now, without claiming that the P3 Job engine exists.

## 6. Authentication, legacy subscription binding and initial-order identity

The original Opus proposal and its first critique allowed a forged renewal when subscription/customer equality passed. Counterexample: an attacker knows a valid subscription/customer pair and sends `subscription.order.success` with a fresh invented order reference. Retrieving the real subscription proves the pair exists, not that the invented order succeeded. Conditional order verification only when an orders field happens to be present is still fail-open.

This objection is now closed by Opus response section 1: V3 is required, with no weak lookup fallback. I accept the lead's exact published subscription sample algorithm and its documented prose inconsistency. Pin a literal known-answer vector, not just a signer/verifier round trip. The installed SDK also signs `{}` for operations without a request model, including subscription GET; an empty-body signature would differ. Both facts are supplied by `../gateway-evidence.md` and the latter was checked in `IyzipayResource.js:80-86`. No merchant credential is necessary to implement or test these mechanics locally.

Live signature activation remains a cutover prerequisite. Lack of evidence that production sends V3 cannot justify an unauthenticated runtime path. The transition plan must arrange activation or establish a fully verified exact-order retrieval protocol before switching webhooks. Unknown/invalid events never grant. Verified unresolved events need durable retention and a replay operation because provider retries are finite.

Legacy identity is a real concern, and I accept Opus's criticism that reference-only lookup without a migration plan would strand existing subscribers. I do not accept its now-withdrawn oldest-payment fallback. Counterexample: one customer bought Premium and later Business; using the oldest payment binds a genuine Business renewal to Premium minutes. New subscriptions store the returned subscription reference, customer, plan binding and grant schedule. Legacy mappings are established from authenticated provider facts plus imported customer/plan data before cutover, with ambiguous mappings held for review. A verified event without a mapping is durably unresolved, not silently discarded or assigned an arbitrary plan. A 2xx is appropriate only after durable capture and a visible replay path exist. The returned provider plan reference is described in `subscription-callback.ts:21-38`, but its type declaration is not live-response verification.

One remaining correctness objection to Opus response section 2: treating every distinct order reference as an additional renewal is not safe if the initial checkout grant represents that same first order. A form token and order reference can be different identifiers for one purchase. Signature and order-reference uniqueness do not prevent two grants under those aliases.

Required combined contract: retain an identity link between the first checkout payment and its actual initial order, and use one payment/grant identity for both when the provider evidence establishes they are the same. No matching by arrival time alone. If the documented/local fixture protocol cannot establish that link, explicitly escalate the unresolved correlation case and durably hold it rather than pin deliberate double-grant behavior as P2 correctness. The provider's actual first-notification behavior is unverified, but the adapter must already have a defined safe outcome for the alias case. Migration analysis in P9 cannot retroactively repair duplicate grants made by new P2 runtime subscriptions.

For cancellation, I narrow my original recommendation: owner authentication is mandatory now. A real provider cancel call and a new pending/canceled customer display alter the existing manual support flow. Unless lead records that specific correction as authorized, preserve the support-request behavior and email, and do not claim that it stops provider renewals. The gateway cancel capability may be designed/tested, but the developer must not independently substitute a different cancellation policy.

## 7. Minimal recovery and request deduplication

### Concessions

I checked `checkout/index.tsx:236-250` and `dao/Payments.ts:14-19`. The server returns the Iyzico form only after payment insertion succeeds, and insertion errors throw. Opus is correct; I withdraw the claim that this ordinary failure exposes a payable form. Lead has corrected DEF-008. Keep Iyzico initialize outside a transaction, then persist the pending payment and only then expose its form. A lost initialization response leaves an unexposed token, not evidence of a charged customer.

Accordingly, remove my mandatory Iyzico pre-intent, uncertain-init operation and callback-before-token general inbox machinery from P2. LemonSqueezy still persists a pending payment first because its checkout custom data needs that stable correlation. A definite creation failure may mark it failed; transport ambiguity must not be mislabeled as a definite provider rejection.

I accept manual authenticated `reconcile_payment` using the normal confirmation path as the minimum recovery mechanism for uncertain result lookups. Do not add stale-payment cancellation, timed closure, a debt collector, subscription fallback or automatic collection after an earlier deferred business refusal. Provider result transport failures leave the saved business status unchanged and expose a retryable observation; definite failure/success uses normal transitions.

I also withdraw a generic `billing_commands` table, mandatory keys on every existing route and a separate persisted `FailedDeferred` receipt as universal requirements. The host already supplies useful operation identity. Under the account/job lock, a start CAS permits one charge; a deferred callback acts only on the prior null duration. The host commits known duration even when strict billing refuses, so subsequent callbacks see known duration and must not charge again after a top-up. That exact host transaction contract, including domain refusal versus infrastructure error, must be tested. If a failed SQL statement aborts the transaction, a savepoint or explicit outer failure handling is needed; pretending a database outage is an ordinary insufficient-balance outcome is not acceptable.

### What remains necessary

The brief explicitly requires request/webhook deduplication and conflicting-payload handling. Removing a general table does not remove that obligation. Satisfy it through narrow domain identities:

- Host start and deferred commands: locked subject status/duration and owner provide operation identity; no public skip/count input. Repeated commands replay/no-op according to those facts, without another debit.
- Payment observations: unique provider payment/order identity, immutable owner/spec binding, terminal completed status and atomic grants. A changed account, schedule or currency on the same identity is a conflict, not an overwrite. Failure then success is a legitimate transition, not a payload-hash mismatch.
- The additive taskr usage sample: it has no existing Job CAS. A stable sample-work reference and a persisted result/usage-group identity are needed to keep a retried sample request from charging twice. Put this in the taskr host or a narrow charge-group contract; a generic command framework is unnecessary.
- Checkout: do not require a new key from legacy clients. If a new contract offers a retry/request key, scope it to account and persist its quote digest with the payment; the same key with a changed order conflicts. Without a supplied stable identity, two explicit checkout submissions remain two purchases, matching source behavior. Do not claim universal checkout-request deduplication in that mode.

For the Iyzico flow, a concurrent pair of requests with the same optional retry key may create two unexposed provider forms before a unique payment insertion arbitrates; only the committed canonical form may be returned. If supporting that optional capability requires substantial extra storage/response machinery, omit the capability from P2 and state the legacy duplicate-checkout semantics honestly. Lead's final contract must choose, not leave the developer to claim deduplication it has not implemented.

Keep a small durable unresolved-notification store for verified events lacking a subscription/order mapping. Manual replay is sufficient for these rare cases if it is implemented, authorized and tested. Unauthenticated spam need not be retained as a payment inbox. A generic inbox worker is unnecessary. The outbox still needs a real claim/retry path; notification durability without any runnable recovery path is not evidence of recovery.

## 8. SQLx history, historical anomalies and narrow host integration

Resolved: use schema `billing`, schema-qualified queries and `billing._sqlx_migrations`, with lead's confirmed sqlx 0.9 `create_schema` and `dangerous_set_table_name` API. Setters are separate calls, migration locking stays enabled and no pooled connection search_path is changed. A coexistence test runs taskr and billing migrators twice against one isolated database and checks distinct histories. My public `billing_` prefix proposal is superseded.

I accept a single usage table with an import provenance column instead of a separate legacy usage table, provided the trust boundary is real. `CHECK(origin = 'import' OR minutes >= 0)` alone is not a security boundary: a runtime writer could label a new negative row imported. Require a separate import role; runtime grants must not permit setting/changing origin, direct negative/foreign-package insertion, or modifying imported anomalies. Account/package ownership is checked for every new operation, preferably with a conditional same-owner relation represented so old anomalies can still be preserved. Merely filtering both owners on read drops historical effects and is not a preservation proof.

Imported null package references, nonpositive minutes, owner mismatches and raw amounts remain available for parity reporting. New invariant enforcement must not make them impossible to import. Source-derived legacy balance and normalized balance can differ for cross-account rows; report exact affected accounts and retain the raw source. Do not silently treat an ignored anomalous row as zero-mismatch migration. That evidence does not block development against synthetic fixtures.

I agree with Opus's taskr concession: use an additive sample host without changing ordinary task creation, repository save signatures or taskr-core dependencies. I also accept omitting CLI wiring in P2 if the real server flow exercises signup/free usage, refused overuse, hosted checkout, signed webhook, paid usage and persistence. Transgate must mount actual billing routes; development identity fakes used in tests are not a replacement for authenticated runtime composition.

The preserved three Iyzico columns and JOB-008 INSERT contradiction are discovery corrections, not new scope. Lead has already corrected the column and DEF-008 records. Every host contract still follows final UPDATE-only billing semantics.

## 9. Honest gates and executable evidence

Resolved in substance. The lead has now implemented `scripts/gate-p2-billing.sh` using explicit billing module and billing-rule checker modes; its name is immaterial. I accept that implementation rather than insist on either advisor's shell-script spelling. The default final module gate still requires CMS. Scoped checks still inspect all existing module dependency edges and require exactly the 123 billing IDs.

I accept ignored Postgres tests for an ordinary offline workspace run only if the required billing/named-evidence run uses `--include-ignored`, requires its database URL and treats missing configuration as a failure. An ignored line must never count as a passed ledger reference. No early-return fake success. Use the lead's isolated database, distinct test accounts and no shared-table truncation.

The assembled-status prerequisite and checker regressions are reported as already completed by lead; I did not rerun them. This critique does not claim passing module evidence. Scalar helpers do not certify host lifecycle, HTTP authentication, runtime consumer behavior, actual mail delivery or rendered web parity. The final contract should name the relevant integration tests for every multi-layer rule and retain P5/P6/P9 gaps explicitly.

## 10. Recommended combined contract

1. Five production module crates: pure core, Postgres, HTTP, Iyzico and LemonSqueezy; explicit test fakes. Host catalog/configuration belongs in transgate-billing and taskr-billing. No module-to-host dependency, no taskr-core change.
2. Exact decimal money with source-order rounding, independent rational expectations and a Node differential fixture. Lead must record the measured price changes in money-evidence.md before implementation; no blanket equality assertion. Raw imported amounts remain, observed provider money is separate. Signed i64 minutes and microsecond UTC instants.
3. D1/DEF-016 allocator, inclusive windows, iterative months and checkout-created anchor. Zero-only bypass preserved, strict zero rejected, no new negative usage. No historical rewrite.
4. Account-first READ COMMITTED transaction composition, authoritative host facts loaded under lock, ascending-ID draft order, batch rollback and completed-count synchronization. Host owns its tables and commit.
5. Transition-based payment completion, all packages and outbox atomic, repeated completion no-op, failure recovery allowed, no historical backfill by missing-package heuristic.
6. V3-authenticated renewals with trusted subscription bindings; verified unresolved events durable and replayable, no customer-only or oldest-plan fallback. Initial checkout/order alias correlation is an explicit correctness requirement, not an automatic second renewal.
7. Iyzico init then persist then expose form; LemonSqueezy persist then initialize. No generic intent/command framework. Domain operation identities provide debit/payment deduplication, while deliberate duplicate checkout submissions retain legacy behavior unless an explicit retry-key contract is chosen.
8. Manual payment reconciliation, actual outbox delivery recovery and narrow unresolved-notification replay. No automatic pending cancellation, new deferred debt collection or unauthenticated recovery shortcuts.
9. Schema billing, isolated sqlx history, original IDs, import-only anomaly privileges and exact parity exception reports. Same-account constraints for new inputs.
10. Real taskr and Transgate HTTP consumers with Postgres/fixture-server smoke evidence; pure transcript/chat presentation policies completed in P2 but later product flows remain unverified. Use the lead's honest P2 gate and retain final CMS failure.

## 11. Remaining response requested from lead and Opus

The original major disagreements are mostly closed. The concrete unresolved items are now narrow:

- Initial checkout versus first recurring notification: accept a single real-purchase identity and a durable unresolved outcome when alias correlation cannot be proved, or provide source/protocol evidence that two distinct purchases are involved. I object to deliberately granting twice merely because token formats differ.
- Minimal taskr consumption identity: confirm a host-owned stable work reference/result so a retried debit cannot charge twice in a sample without Job CAS. I no longer require generic billing_commands or universal client keys.
- Import provenance enforcement: confirm runtime cannot choose import origin and that owner-anomaly handling preserves/report differences rather than silently filtering them away.
- Cancellation: lead must state whether P2 retains the manual support flow or authorizes provider cancellation. My default is now the existing manual behavior with owner checks until that specific decision is recorded.

This critique supersedes the conflicting recommendations in my independent proposal only at the points explicitly identified above. It does not modify Opus's files or any implementation contract.
