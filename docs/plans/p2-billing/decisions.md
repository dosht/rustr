# P2 billing decisions

Lead decisions under `docs/prd.md` section 2. A recorded design decision is not implementation evidence; rules stay unverified until their tests and runtime checks pass. Explicit Needs mu policy defaults remain unchanged.

## D1 Negative package prefix overcharge

Source: Astra's independent proposal, section 4; final SQL in `/home/mu/Business/Transgate/frontend/supabase/migrations/20251230190000_fix_request_breakdown_user_id_again.sql:45-72`. Discovery defect: DEF-034. Affects BIL-012, BIL-020, BIL-021 and signed balances after first-job bypass.

Current behavior: an early open package has -16 minutes remaining and a later one has 100. Aggregate balance is 84. A request for 10 enters the signed prefix expression and allocates 26 from the later package, so persisted balance becomes 58 although the function's arithmetic return is 74. This is a source-derived counterexample; no production experiment was performed.

Intended behavior: a successful strict debit writes exactly the requested positive minutes. Existing negative package balances reduce admission balance while those packages are open. Their debt lapses with the package under the retained window-based balance rule; the fix does not introduce a permanent debt-transfer mechanism.

Decision: fix as an unambiguous overcharge defect under the PRD's explicit exception. First check the signed aggregate balance at the command instant. Allocate exactly the requested minutes over positive remaining capacities ordered by `(end_at, package_id)`. Do not allocate from negative or zero capacities; do not erase, transfer or amortize their historical usage a second time. The example writes 10 and returns the actual derived balance 74. Preserve inclusive windows and first-job overdraft on its earliest package. Equal-expiry tie correction remains DEF-016. No historical usage row is rewritten.

Outcome over time, identified by Opus's critique: legacy leaves the later package at 74, so balance becomes 74 when the -16 package expires. The corrected allocator leaves the later package at 90, so balance becomes 90 after that expiry. Legacy also lets the overdraft lapse if no strict debit happened before expiry; its collection is timing-dependent and double-counted during the overlap. This is an explicit customer-favorable outcome change, not merely a different arithmetic return. No freemium eligibility change or historical rewrite is authorized.

Required evidence: a failing-before/passing-after allocator regression for this vector during implementation; reversed expiry ordering; zero and negative capacities mixed with positive ones; exact exhaustion and rejection with no writes; Postgres persisted balance matching the returned successful outcome. Existing six positive-capacity breakdown fixtures retain their exact allocations/carry values. Any migration comparison involving the defective legacy allocation is reported as an explicit rule difference, not hidden as zero-mismatch parity.

Status: decided, not implemented or verified.

## D2 Exact pricing with explicit binary-rounding differences

Sources: `money-evidence.md`, CPN-002/003/004/006/010, DEF-011. Current behavior uses JavaScript binary floating point before cent-floor and half-up operations. The local Node comparison found 3824 differing combinations out of 319840; for 54 hours with the locale discount and no coupon, source charges USD 40.22 instead of the exact formula's 40.23, or TRY 1769.68 instead of 1770.12 at rate 44. These are price changes, not merely prettier numeric strings.

Intended behavior: evaluate the captured prices, percentage discounts and operation order exactly; keep locale cent-floor and coupon half-up, then quantize the provider-facing amount to two decimal places. Do not redesign discounts, rounding stages or coupon applicability.

Decision under PRD section 2: fix the demonstrated binary-rounding defect in new quotes. Use exact decimal arithmetic, not a binary-float compatibility engine. Record and retain the legacy comparison differences instead of asserting zero price mismatches. Preserve historical stored amounts verbatim. Keep the documented display-versus-charged formula distinction (DEF-010), ignored coupon-type semantics and other Needs mu defaults. Quoted and independently observed provider amounts remain separate; unsigned webhook fields are never observed-money evidence.

Required evidence: source-derived legacy oracle plus independent exact expected values, including all five examples in `money-evidence.md`, fractional configured exchange rates, applicable/nonapplicable coupons and special-offer precedence. Changed ledger rows must link this decision and executable tests. Provider serialization must match the frozen exact quote; rounding happens once per documented stage.

Status: decided, local source-formula difference measured; Rust implementation and provider-fixture verification still required.

## D3 Payment and trust-boundary corrections

Decision under PRD section 2, current/intended behavior recorded in the named discovery defect rows:

- DEF-001: serialize account facts, deductions and grants; host transition and debit remain one transaction.
- DEF-002: only trusted runtime billing writes usage; reject negative runtime usage and wrong-owner package references. Preserve the source's zero-minute bypass, so a blanket positive-only constraint is not the rule. Import-origin permissions are separate; historical anomalies remain reportable.
- DEF-003: require V3-authenticated renewal events. Subscription/customer existence is not proof that a claimed order succeeded. Merchant signature activation is a cutover prerequisite, not a reason for a fail-open fallback.
- DEF-004: a newly authenticated pending/failed/canceled-to-completed transition grants its whole schedule once in the same transaction. The canceled recovery is the explicit correction recorded below, not source grant parity. Already-completed historical rows are not automatically backfilled by missing package links.
- DEF-005: transport uncertainty leaves payment status unchanged; definite provider observations drive transitions. Manual authenticated reconciliation only, no automatic historical cleanup/sweep.
- DEF-006: fix session/owner authorization now. Retain manual support cancellation request/email, without claiming it proves provider cancellation. Replacing that workflow with a provider cancel call remains a mu choice.
- DEF-007: new renewals use a trusted subscription-specific binding, never the customer's oldest payment. Verified unbound or ambiguous first-order notifications remain durable and replayable without a grant. P9 establishes legacy bindings and provider alias evidence before cutover.
- DEF-009: enforce the advertised whole-hour range 5..9999 in the use case.
- DEF-012: UTC microsecond instants and zone-independent queries; historical naive payment timestamps are interpreted as UTC during migration.
- DEF-016 and DEF-034: deterministic row-wise allocation and exact debit conservation, with D1's explicit expiry outcome.
- DEF-030: one completion event per genuine transition; concurrent confirmations do not duplicate events or grants.

DEF-008's original payable-form claim was corrected from source; keep Iyzico initialization then persistence then form exposure, with no generic intent framework. DEF-011's exact new quote correction is D2; stored historical quote and actual independently observed provider amount remain distinct.

Retain all explicit Needs mu defaults: package windows/anchor/month stepping, freemium definitions and eligibility, deferred delivery, refund/delete policy, display-only cuts, display-versus-charge formulas, coupon semantics and manual subscription cancellation. No provider sandbox/live verification has been performed.

Status: authorized implementation contract, not passing evidence. Each affected rule needs the recorded decision and consumer-visible regression coverage.

## D4 Initial subscription order alias requires an explicit resolution

Primary evidence: `opus/provider-identity-evidence.md`. Iyzico documents initial-order webhooks as well as recurring ones. Checkout retrieval identifies a subscription, not its initial order. Neither order-array completeness/chronology nor a checkout-to-payment-attempt conversation-id relationship is guaranteed by the available primary documentation. Picking the first received notification, oldest visible array entry or an equal-looking field is not proof.

Decision: retain the captured initial grant on successful confirmation. Store its trusted subscription binding. Until `first_order_ref` is established, hold all verified order notifications for that subscription durably as `initial_order_unproven`; do not automatically grant later arrivals either, because delivery can be reordered. An authenticated admin resolution explicitly identifies the initial order after checking provider evidence and records actor, order reference and rationale. The adapter must retrieve and verify that the specified order belongs to the bound subscription/customer before accepting the resolution; absence or ambiguity fails closed. Human identification of the first order is recorded as a human decision, not an automatically proved chronology.

The binding is immutable/idempotent: repeating the same resolution is harmless; a conflicting initial reference is rejected. Replay through the normal payment transition handles the initial alias without a second grant; subsequent distinct authenticated orders grant once. No arbitrary account-to-subscription mapping API or customer-only/oldest-plan guess is permitted. Unbound legacy subscriptions still require verified P9 mapping.

This adds a manual reconciliation step per subscription until provider sandbox evidence supports an automatic alias rule. It preserves initial confirmation timing and avoids deliberately double-granting. It is a documented integration limitation, not a claim of fully automated or live-verified Iyzico correlation. The alternative of granting only on webhooks would delay initial entitlement and is not selected.

Required evidence: notification before confirmation, reordered later/initial notifications, all held while alias unknown, unauthorized resolution, wrong customer/subscription/order, duplicate/conflicting resolution, replay, one initial grant and one grant for a genuine distinct renewal.

Status: safe P2 execution/recovery decision; provider sandbox/production correlation and legacy cutover remain prerequisites.

## Review clarifications

- Preserve source-authorized observations from historical `canceled` payments; do not invent a new terminal status rule. A newly authenticated completion grants once, including recovery from a canceled record, but an already-completed record is never backfilled merely because links are absent. Runtime manual cancellation still does not write payment status `canceled`.
- A held renewal uses its first verified receipt instant as the payment/grant anchor when replay creates the payment, not the admin's later resolution time.
- Auth/configuration/system error envelopes on a provider result lookup are uncertainty about the lookup, not evidence that payment failed.
- LemonSqueezy inclusive-tax comparison is settled by primary documentation in `gateway-evidence.md`: configured quote equals subtotal minus discount in either tax mode; tax and total remain separate observations.
- The captured source `payment_status` enum is pending/completed/failed/canceled (`data-model.md` enum census, baseline declaration at lines 193-198). The earlier contract phrase about unknown historical statuses was too broad. Keep these four; report a divergent future snapshot explicitly instead of adding an unsupported runtime status model.

## D5 Provider uncertainty is not a failed payment

Iyzico checkout retrieval documents intermediate payment states as well as SUCCESS and FAILURE. Only SUCCESS and FAILURE are authoritative terminal observations for the PAYG adapter. Intermediate, unknown, malformed and unavailable observations leave the saved business status unchanged. A subscription result error envelope without an authoritative payment outcome is likewise not confirmed, not a definite card decline. No undocumented decline-code list is introduced.

This narrows the legacy handling that treats non-success as failure. It is an explicit defect correction: a lookup or unfinished authentication cannot prove a financial failure. The HTTP/presentation result must expose the uncertainty rather than claiming success or a completed failure. Later authenticated observations and manual reconciliation remain available.

LemonSqueezy's documented Order object does not include quantity in `first_order_item`; the separate Order Item resource does. Missing quantity is retrieved and identity-checked through the authenticated provider API before a grant, never assumed to be one. Provider lookup failure is retryable and grants nothing. The same signed order amount must satisfy both the quoted amount and the documented inclusive/exclusive tax equation.
