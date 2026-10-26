# Opus review of `modules/billing/core`

Read-only review, 2026-10-24, of the core slice as saved at review time (`app.rs`, `ports.rs`, `model.rs`, `ledger.rs`, `charge.rs`, `freemium.rs`, `grants.rs`, `calendar.rs`, `pricing.rs`, `decimal.rs`, `presentation.rs`, `notify.rs`, `types.rs`, `Cargo.toml`) against `implementation-contract.md`, `decisions.md` D1 to D4 and the BIL/FRE/CPN rule meanings. No build, test or edit. Postgres, HTTP, gateway and host files are out of scope. Findings are ordered by impact; each has a counterexample and a narrow fix.

## High

### H1. `billing-core` does not compile on its own: `uuid` lacks the `v5` feature

- Where: `types.rs` `AccountId::from_subject` (line 31, `Uuid::new_v5`); `Cargo.toml` line 12 enables only `serde` and `v4`.
- Counterexample: `cargo test --locked -p billing-core` (the per-package run `check-prd.py` and the module gate use) resolves features for this package only. `uuid` without `v5` has no `new_v5`, so the crate fails to compile. A full workspace build can hide it because `taskr-postgres` enables `v5` and features unify.
- Fix: add `"v5"` to the `uuid` features in `modules/billing/core/Cargo.toml`.

## Medium

### M1. A replayed renewal starts its packages at replay time, not when the order notification arrived

- Where: `app.rs` `notification_in`, the new renewal `NewPayment` uses `created: now` (line 980). `replay_unresolved` (line 1030) and `resolve_first_order` (line 1054) replay held rows through that path.
- Rule: BIL-055 and BIL-045: a renewal's packages start at the renewal payment row time, which in the source is the callback receipt time (`iyzicoRenewal.ts:19-41` inserts the row when the callback arrives). Under D4 every renewal before the first-order resolution is held, so this path is the normal one for the first renewals of every new subscription.
- Counterexample: monthly plan, renewal callback verified and held on 1 Nov 10:00 (`initial_order_unproven`). Admin resolves on 6 Nov 09:00. Replay inserts the payment with `created = 6 Nov 09:00`, so the package runs 6 Nov to 6 Dec instead of 1 Nov to 1 Dec. The customer has no paid window from 1 Nov to 6 Nov (or only the previous package if it is still open) and the 1 Dec renewal then overlaps by five days.
- Fix: carry the held row's `received_at` into replay (`process_notification(..., replay_of, received_at)`) and use it as `created` for a newly inserted renewal payment. Live notifications keep `now`. Add a test: held on day 1, resolved on day 6, package starts on day 1.

### M2. LemonSqueezy amount check likely holds every paid order from a tax-inclusive store

- Where: `app.rs` `order_in` lines 1265-1267: `net = subtotal - discount - (tax if tax_inclusive)` compared with the quote.
- Rule: contract "Separate subtotal, discount, tax and total; don't compare a pre-tax quote to tax-inclusive total." Our checkout sets `custom_price` to the quote, so the item price LemonSqueezy charges is the quote in both tax modes. In a tax-inclusive store the tax is inside that price; in a tax-exclusive store it is added on top.
- Counterexample (if `subtotal` is the item price, which is how the order object describes it): quote 7.45 USD, tax-inclusive store, `subtotal 745`, `tax 124`, `total 745`. The code computes `net = 621`, which is not 745, and holds the order as `amount_mismatch`, so a customer who paid gets nothing until an admin acts. Every order from such a store behaves the same.
- Fix: compare the quote to `subtotal - discount` in both modes (discount is already required to be 0), keep `tax` and `total` as observed data, and pin both modes with documentation-derived fixtures. I did not re-read the LemonSqueezy order docs for this review; if they define `subtotal` as excluding inclusive tax, the current formula is right and only needs that citation in a test name.

### M3. First-order resolution accepts when the provider detail has no customer

- Where: `app.rs` `resolve_in` lines 1118-1122 compare customers only when both `provider_customer` and the stored `customer_ref` are present.
- Rule: D4: "The adapter must retrieve and verify that the specified order belongs to the bound subscription/customer before accepting the resolution; absence or ambiguity fails closed."
- Counterexample: the detail parser returns `customer_ref: None` (field missing or renamed in the live shape, which `provider-identity-evidence.md` section 3 says is unverified). The order reference is listed, the customer check is skipped, the resolution is stored permanently and held orders start granting.
- Fix: require `provider_customer` to be `Some` and equal to the stored binding's `customer_ref` (also `Some`); otherwise return `Uncertain` and store nothing.

## Low

### L1. Strict debit with no open package returns the wrong refusal

- Where: `ledger.rs` `plan_strict` lines 136-139 use `balance_or_zero`, so an empty `open` with `requested > 0` returns `InsufficientBalance { balance: 0 }`.
- Rule: BIL-022: with no open package the source skips the balance check (`NULL < 0` is not true) and raises "Failed to deduct minutes" (`20260115162255...sql:40-50, 81-86`).
- Impact: none through `charge::strict` (it returns `NoAvailablePackage` first), but a BIL-020 to BIL-022 test written against `plan_strict` or `plan_debit` pins the wrong rule.
- Fix: in `plan_strict`, return `NothingDeducted` when `open` is empty, before the balance check.

### L2. Special-offer quote drops a coupon id the source stores

- Where: `pricing.rs` `price_order`, special-offer branch line 252 sets `coupon_id: None`.
- Rule: CPN-013 and the source: `configureOrder` sets `coupon_id: coupon?.id || null` for every pay-as-you-go order, including the special offer, while the amount ignores the coupon (`checkout/index.tsx:296-302`).
- Counterexample: a special-offer buyer submits an unexpired coupon; the source stores its id with an undiscounted TRY amount, the new code stores none.
- Fix: in the special-offer branch set `coupon_id` from the unexpired coupon (still `discount_applied: false`).

### L3. Loyalty alert text is derived from the percent, which applies DEF-023 without a decision

- Where: `pricing.rs` `special_client_alert` line 509 builds the message from `offer.percent`.
- Rule: DEF-023 (config 33 percent, text 25 percent) is a display defect that stays as today pending mu; the contract keeps "loyalty alert wording".
- Fix: let `SpecialClientOffer` carry the captured message text as host data and show it verbatim; keep `percent` as data only.

### L4. `Canceled` payments ignore every observation

- Where: `grants.rs` `transition`, the catch-all arm (line 145).
- Rule: the source renewal update only excludes `completed` and the same status, so a `canceled` row could still move. Production has exactly one `canceled` payment.
- Impact: negligible, but undocumented. Either add a test that pins "canceled is terminal" as a recorded choice, or mirror the source.

### L5. Explicit Iyzico error envelopes on the result lookup count as a payment failure

- Where: `app.rs` `confirm_checkout` line 727 maps `GatewayError::Rejected` to `Observation::Failed`.
- Rule: this matches the source (any non-success envelope became `failed`), but D3/DEF-005 says only definite provider observations move a payment. An envelope for an authentication, configuration or system error on the lookup call says nothing about whether the customer paid; with a wrong API key every confirmation would mark payments failed (now recoverable via DEF-004, but still a false record and a `PaymentFailed` event).
- Fix (needs lead agreement since it narrows the source mapping): adapters return `CheckoutResult::Failure` only for a payment-status answer, and map other error envelopes to `GatewayError::Uncertain`.

### L6. Held-row dedup must not depend on the reason

- Where: `app.rs` `hold` and `replay_unresolved`; the dedup key lives in the Postgres adapter (`hold_notification`).
- Counterexample: a notification held as `subscription_unbound` before confirmation, replayed after confirmation, is held again as `initial_order_unproven`. If the adapter's uniqueness includes `reason`, a second held row appears and the first stays `held` with a stale reason; both replay later.
- Fix: key held rows on (gateway, environment, event type, subscription ref, order ref) and update the reason on re-hold. Cross-check with Astra's Postgres review.

### L7. Possible clippy failure from a deprecated `time` helper

- Where: `calendar.rs` line 67 `time::util::days_in_year_month`.
- Check: recent `time` 0.3 releases deprecate that helper in favor of `Month::length(year)`. If the locked version marks it deprecated, `cargo clippy -- -D warnings` in the Rustr gate fails. Verify against `Cargo.lock`; switch to `month.length(year)` if so.

## Checked and correct

- D1 allocator (`ledger.rs` `breakdown`, `plan_strict`): signed admission, exact allocation over positive capacities in `(end_at, id)` order, no second drain of negative packages, aggregate admission implies enough positive capacity. The six original vectors keep their values because their capacities are positive.
- Bypass keeps the zero-minute write and targets the earliest open package with an id tie break; strict zero still fails with `NothingDeducted` through the paid path, as in the source.
- `decide_job_start`, `plan_batch`: freemium-only plus no other completed job gives the bypass; NULL duration defers; batch decided in ascending id with an evolving snapshot, all or nothing; FRE-012 errors (`NoActivePackage` first job, `NoAvailablePackage` later) match.
- `web_guard`: lowest draft id supplies the language and target, inclusive 20 and 45, exact locale match, lower-cased language match.
- Three freemium facets stay distinct; a null package name is never "paid", matching SQL `name <> 'freemium'`.
- `expand_grant`: iterative clamped stepping from the previous start, end from its own start, count 0 gives nothing, invalid interval fails with the source text.
- `transition`: pending or failed to completed grants once; late failure ignored; repeats ignored.
- `price_order`: locale discount on the full total with cent floor, coupon half up, TRY conversion then one quantization, special offer precedence in TRY, plans never take a coupon, server range 5..9999 (DEF-009). `Decimal::round_div` half up matches `Math.round` for negatives too.
- Transaction order in every write path: account lock, then payment or subscription row; no provider call inside a unit of work; the opener commits.
- Freemium provisioning anchors to the directory's trusted `created_at` and never grants twice.
- `confirm_checkout` leaves state unchanged on `Uncertain`; reconciliation reuses it; outbox relay claims, delivers without a lock, records per-target delivery, acknowledges.
