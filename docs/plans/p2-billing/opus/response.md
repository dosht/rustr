# P2 billing: Opus response

Written 2026-10-24 after the lead's ruling on `opus/critique.md` and the partial `astra/critique.md` (conclusions only; its detailed revision was not yet saved). No code, builds or tests.

## 1. Renewal authentication: lookup fallback withdrawn

I accept the lead's ruling. Matching a customer or subscription reference alone does not prove that a given order succeeded, so a forged `subscription.order.success` with a fresh order reference would still grant. My `lookup` mode without an orders field is withdrawn.

Contract for P2:

- The renewal route requires `X-IYZ-SIGNATURE-V3` and verifies it with the configured merchant id and secret, using the published subscription sample formula (`gateway-evidence.md`), with the prose inconsistency documented in the fixture provenance. Missing settings fail startup; a missing or wrong signature answers 4xx and grants nothing.
- Signature proves the identifiers and event type, not money. Package spec and amount come from our stored subscription binding, never from the body.
- Event names accepted: `subscription.order.success`, `subscription.order.failure` (docs) and `subscription.order.failed` (source). Fixtures cover all three.
- No live-provider claim: the verifier is proved by a known-answer vector and the local fixture server only.

## 2. Subscription binding: no oldest-plan guess

I accept the ruling. The oldest-payment-by-`customer_id` fallback is withdrawn.

- New subscriptions bind at the initial confirm: store the subscription reference returned by the subscription checkout form result (`checkout/confirm.tsx:257-268` sample shows `referenceCode`) on the payment as `subscription_ref`.
- A verified renewal whose `subscriptionReferenceCode` matches a binding creates or updates its payment keyed by `(iyzico, orderReferenceCode)` and grants from that binding's spec.
- A verified renewal with no binding (every legacy subscriber until P9 maps them) is held unresolved: one row in `billing.unresolved_notifications` (gateway, identifiers, event type, received time, signature outcome), answered 2xx so Iyzico stops retrying, and visible to an admin. No payment, no packages, no guess. Resolving it is a P9 cutover task (verified mapping of each legacy customer to its subscription reference), after which an admin use case replays the held row through the normal path.
- Evidence item still open, now a P9 prerequisite: whether Iyzico sends `subscription.order.success` for the first order of a new subscription. If it does, the first order would arrive as a distinct order reference and grant a second set at signup. P2 treats every verified order reference as its own order (current behavior) and the fixture suite pins that; P9 checks production before cutover.

This table is the only notification store I keep: it exists because a verified paid order with no binding must not be lost after Iyzico's three retries, and it needs no worker or lease.

## 3. Intents and command receipts

My position after the rulings, for the lead to close:

| Item | Keep? | Reason |
|---|---|---|
| Iyzico durable checkout intent and uncertain-init state | No | The form reaches the browser only after the payment insert succeeds (`checkout/index.tsx:247-250`, insert throws at `dao/Payments.ts:14-19`). Init then insert, with no transaction open during HTTP, preserves BIL-067 and leaves only an orphan token nobody sees. An intent row per attempt would need its own cleanup policy |
| LemonSqueezy pending payment before checkout creation | Yes | Our `payment_id` is the correlation key in `custom_data`; it must exist first. A definite creation error marks it `failed` |
| `billing_commands` with client idempotency keys | No | Job start is once-only through the job status compare-and-swap (BIL-026); duplicate checkouts are harmless today; requiring keys from every client is a new API rule no ledger row asks for |
| `(account, subject, phase)` charge receipt and persisted `FailedDeferred` | No | Same compare-and-swap guarantees one start charge; deferred charge failure is logged today (BIL-036). Adds data, fixes nothing listed |
| Plan revision and request digest | No | Plan and apply run in one transaction under the account row lock |
| General inbox and leased worker | No | Replaced by the single `unresolved_notifications` table above, plus the outbox relay |
| Manual `reconcile_payment` admin use case | Yes | Recovery for a transport error that left a payment `pending` (DEF-005), with no scheduler and no change to legacy pending rows |

If Astra's detailed revision brings a concrete flow where a callback can arrive before its correlation key, or a double charge the job compare-and-swap does not stop, I will revisit the matching row.

## 4. D1 wording

Accepted as the lead recorded it: the ledger shows 74 versus 90 after the negative package expires, existing window balance semantics kept, no double collection.

## 5. Nothing else changes

Gate naming and discovery contradictions are closed by the lead. The rest of `critique.md` section 11 stands, with items 5 and 7 replaced by sections 1 to 3 above.

## Round 2: response to the completed Astra critique

Read `astra/critique.md` (complete), `astra/response.md` and `money-evidence.md`.

### 6. Draft order: conceded

`JobRepository.ts:123-129` selects drafts with `.order("id")` and `transcription.ts:408-409` takes `draftJobs[0]`. My critique section 5 ("first draft in the submitted order") is wrong. The web guard uses the lowest draft id; lock and charge order are ascending id too. Tests cover both permutations of the request array with the same result.

### 7. Initial order alias: conceded, with a concrete correlation rule

My response section 2 pinned "every verified order reference is its own order", which would deliberately grant twice if the first order notification is the initial checkout. Withdrawn. Rule:

- Before the write transaction, retrieve the bound subscription with our API credentials (signed, no transaction open). If its orders list contains the notified order reference and that order is the subscription's earliest order, it is the initial purchase: link the reference to the initial payment (`first_order_ref`), grant nothing more, answer 2xx. If it is a later order, it is a renewal: grant once under `(iyzico, orderReferenceCode)`.
- If the orders list is absent, unparseable, or does not contain the reference, hold the notification in `billing.unresolved_notifications` for manual replay. No arrival-time matching.
- Fixtures: initial alias, later renewal, missing orders field, reference not listed. The live order shape is unverified and stays a cutover check.

### 8. taskr consume identity: conceded

The taskr sample has no job compare-and-swap. `taskr-billing` owns a `sample_work` row (host table, unique id, status `pending` or `done`). Consume runs in one transaction: account lock, lock the work row, CAS `pending -> done`, charge with `job_ref = work id`. A retry finds `done` and returns the saved result without a second charge.

### 9. Import provenance and owner anomalies: conceded, and my read rule corrected

- Runtime cannot set `origin`: the column defaults to `runtime`, the runtime database role gets column-level INSERT without `origin` and no UPDATE on usage, and only the P9 import role writes `import` rows. The P2 Postgres suite proves the runtime role cannot insert a negative or `import` row.
- My proposal's "filter usage by `account_id` on both sides" was wrong: it drops a cross-account row's effect on its package. Normalized consumption is the sum of every usage row referencing the package (DEF-002's "group by package only"), new writes require the package owner to equal the locked account, and the P9 report lists accounts where legacy grouping (which double counts) differs.

### 10. Outbox relay: conceded

Short claim transaction with a lease column, commit, then call the mailer or analytics with no lock held, then mark published. At least once, as both said.

### 11. Money: one remaining objection

`money-evidence.md` shows 3824 of 319840 combinations differ, and in every example it lists the exact amount is one cent higher than what customers pay today (TRY: 44 kuruş higher). Exact rational pricing therefore raises some prices. That is not a defect that hurts the customer, so I do not think the PRD section 2 exception covers it; it is a pricing change for mu.

Recommended default, which keeps every charged price unchanged and still stores exact money:

- The quote evaluates the legacy operation order in `f64` exactly as today, then quantizes once, half up to 2 decimals (`Math.round(amount * 100)`, the same quantization `money-evidence.md` used). From that point on, the amount is an exact decimal: storage, provider request, TRY display, VAT split.
- This gives zero differences against the evidence fixture by construction, and removes the stored float artifacts (`393.35999999999996` becomes `393.36`).
- Exact rational pricing stays implemented as the documented alternative behind no flag, as a pure function with its own rational tests, so switching is a one-line decision if mu approves the 3824 changes.

If the lead has already decided to accept the price change, I withdraw this; it only needs to be an explicit lead or mu decision recorded in `decisions.md`, not a side effect of choosing a number type.

### 12. Cancellation

Agreed with Astra: owner check now, manual support request and email kept, no claim that it stops renewals, provider cancel only if the lead records that decision.

### Open after round 2

Only the money default (section 11) is still contested between the advisors. Everything else in `astra/critique.md` section 10 I accept, with the alias rule, taskr identity and import role as written above.

## Closure

Lead decision D2 in `decisions.md` records and approves the measured price changes, so section 11's objection is satisfied: exact decimal quotes, historical amounts kept verbatim, legacy differences kept as evidence. My section 11 default is withdrawn.

Alias rule wording aligned with Astra: the alias is proved only when the authenticated subscription orders show the notified order is the earliest one; otherwise the notification is held unresolved. No remaining design objection from Opus.
