# Astra final reconciliation response

The completed critique is [critique.md](critique.md). This short response incorporates Opus's response and lead's money evidence; it is the recommended dispatch contract, not implementation evidence.

I accept D1, exact-decimal runtime with a recorded price-change decision, account-first locking, atomic completion/grants/outbox, host-owned catalog data, schema `billing` with its isolated SQLx history, the additive taskr sample, V3 fail-closed renewals and no oldest-plan guess. The lead's existing P2 gate is suitable; final CMS readiness remains separate.

Money is now measured, not hypothetical: 3824 of 319840 combinations differ, by up to 44 minor units. The 54-hour locale-discount case is USD 40.22 legacy versus 40.23 exact, or TRY 1769.68 versus 1770.12. I recommend recording this as an explicit pricing correction before implementation, preserving historical stored amounts. The legacy fixture reports differences; independent rational expectations prove the chosen decimal rule. No blanket equality assertion or representation-only claim is valid.

I withdraw the generic billing_commands table, mandatory keys on every client command, plan revisions, universal FailedDeferred receipts and Iyzico pre-intent/uncertain-init machinery. I checked the source: Iyzico's form is returned only after the payment insert succeeds. Start/deferred deduplication can use locked host status/duration. LemonSqueezy still needs a persisted payment before checkout. Manual reconciliation, an outbox claim/retry path and durable unresolved verified notifications are sufficient recovery; no new expiry, debt collection or automatic cancellation policy.

Four precise qualifications complete the contract:

1. Source batch order is ascending draft ID: `JobRepository.ts:123-129` explicitly uses `.order("id")`. The first-language guard must not use submitted array order. All relevant facts are read after the account lock; completion and deletion use that same order of locks.
2. A zero-minute bypass writes a zero row today. Allow nonnegative new usage, but only the bypass accepts zero; strict zero still fails. Import origin must be inaccessible to runtime writers, or an origin-based CHECK is bypassable.
3. The taskr sample has no existing Job CAS. Give its consume operation a stable host-owned work reference/result so a retry cannot debit twice. No generic command framework is required.
4. Initial checkout token and first order reference may identify the same purchase. Do not deliberately grant twice because their strings differ. Correlate using authenticated provider evidence; if identity cannot be established, retain the verified notification unresolved for explicit replay. This is a safe runtime outcome while provider correlation is resolved, not a claim of verified live behavior.

For cancellation, retain the existing manual support request plus owner authentication unless lead expressly authorizes replacing it with provider cancellation. Do not describe the manual request as proof that renewals stopped.

Paid-without-packages, equal-expiry allocation, D1, unauthorized renewals and the measured money differences must each have named decision/evidence rows. None licenses changes to inclusive windows, iterative months, the three freemium definitions, completed-job eligibility, failed deferred delivery or refund policy. The remaining protocol/import limits should be explicit in the implementation contract; no further architecture framework is needed before dispatch.

No builds, tests or provider calls were run by this advisor.

## Round 2 closure

Lead has now recorded [D2](../decisions.md#d2-exact-pricing-with-explicit-binary-rounding-differences), explicitly approving the measured price correction. This satisfies Opus response section 11's condition: new quotes use exact arithmetic, historical amounts stay unchanged, and the differential fixture retains all measured discrepancies. No legacy f64 runtime or unused alternative pricing implementation is needed.

I accept Opus's remaining concessions: ascending draft ID, authenticated initial-order correlation with unresolved handling, host sample_work CAS/result identity, import-only origin permissions, package-based usage summation with reported historical differences, and a leased relay. Initial-order correlation requires evidence that the returned order chronology actually identifies the first purchase; an incomplete or ambiguous list remains unresolved. Both advisors' substantive objections are resolved subject to these recorded decisions and evidence requirements. Lead can dispatch the reconciled contract; this is design agreement, not implementation verification.
