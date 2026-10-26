# Iyzico subscription identity evidence

Collected 2026-10-24, read-only. Sources: public Iyzico docs (English and Turkish subscription pages, webhook page), installed `iyzipay` 2.0.64 SDK, Transgate source. No live or sandbox request, no secrets, no build or test.

## 1. Facts with citations

| # | Fact | Source |
|---|---|---|
| F1 | Subscription detail is `GET /v2/subscription/subscriptions/{subscriptionReferenceCode}`. The SDK signs `{}` as the body for it | docs "Subscription Transactions / Get Subscription Detail" (https://docs.iyzico.com/en/products/subscription/subscription-implementation/subscription-transactions); SDK `lib/resources/Subscription.js:38-41`, `lib/IyzipayResource.js:80-86` |
| F2 | The docs give a schema only, no example response, in both English and Turkish | same page; Turkish https://docs.iyzico.com/urunler/abonelik/abonelik-entegrasyonu/abonelik-islemleri |
| F3 | Documented shape: `data.totalCount`, `data.currentPage`, `data.pageCount`, `data.items[]`; each item is a subscription with `referenceCode`, `parentReferenceCode`, `pricingPlanReferenceCode`, `customerReferenceCode`, `subscriptionStatus` (ACTIVE, PENDING, UNPAID, UPGRADED, CANCELED, EXPIRED), `trialDays`, `trialStartDate`, `trialEndDate`, `createdDate`, `startDate`, `endDate`, `orders[]` | docs schema `GetSubscriptionDetailResponse` |
| F4 | Each order: `referenceCode` ("reference code of the subscription period (order)"), `price`, `currencyCode`, `startPeriod`, `endPeriod`, `orderStatus` (WAITING, SUCCESS, FAILED), `paymentAttempts[]` with `conversationId`, `createdDate`, `paymentStatus`, `paymentId` (only on SUCCESS), `errorCode`/`errorMessage` (only on FAILED) | docs schema |
| F5 | Order `referenceCode` is what the webhook calls `orderReferenceCode` (the Retry section uses the webhook's `orderReferenceCode` as the order `referenceCode`) | docs "Retry Subscription Payment", `POST /v2/subscription/operation/retry` |
| F6 | Subscription checkout form result is `GET /v2/subscription/checkoutform/{token}`. Its `data.referenceCode` is "reference code of the created subscription" (Turkish: "Oluşturulan aboneliğin referans kodu"). `parentReferenceCode` is a merchant-side correlation key for updates | docs "Retrieve Checkout Form Result"; SDK `lib/resources/SubscriptionCheckoutForm.js:17-20` |
| F7 | The checkout form result contains no order reference, no orders array and no payment id | same schema |
| F8 | Subscription webhooks are sent "not only for the initial transaction but also for the recurring payments". Events: `subscription.order.success`, `subscription.order.failure`. First delivery 10 to 15 s after the attempt, retry every 15 min, stop after 3 attempts | https://docs.iyzico.com/en/advanced/webhook |
| F9 | Webhook body: `orderReferenceCode`, `customerReferenceCode`, `subscriptionReferenceCode`, `iyziReferenceCode`, `iyziEventType`, `iyziEventTime`. No amount, currency, period or payment id | same page; Transgate samples `frontend/src/pages/api/subscription-callback.ts:44-65` |
| F10 | SDK samples (`samples/IyzipaySubscriptionSamples.js:342-372, 415-424`) call retrieve, search and checkout form retrieve but contain no response bodies | installed SDK |
| F11 | Search: `GET /v2/subscription/subscriptions` with optional `page`, `count`, `subscriptionReferenceCode`, `parentReferenceCode`, `customerReferenceCode`, `pricingPlanReferenceCode`, `subscriptionStatus`, `startDate`, `endDate`; same paged response shape | docs "Search Subscriptions"; SDK `lib/requests/SearchSubscriptionRequest.js:7-18` |

## 2. Not supported by any primary source

- Sort order of `orders[]`. No statement.
- Completeness of `orders[]`: whether every past period appears, whether failed periods stay, whether future WAITING periods appear, or whether `orders` is ever truncated. No statement.
- Whether `data.items` on the detail endpoint can hold more than one record, and its page size. No statement.
- That the first order's `startPeriod` equals the subscription `startDate`, or that periods are contiguous. No statement.
- Any field that links the checkout form token or the checkout result to the initial order. None exists (F7).
- An example of a real response body for detail or checkout result. None (F2, F10).

## 3. Source conflict the adapter must not paper over

Transgate's working renewal code reads the detail response as `result.data` being the subscription itself (`subscription-callback.ts:21-38` type, `:117` `resolve(result.data as SubscriptionResponse)`, then `subscription.customerReferenceCode` at `:131`). The docs put the subscription under `data.items[]` (F3). One of them is not the live shape, and neither is a recording. The adapter should parse the documented shape, keep the source-derived shape as a second labeled fixture only if the lead accepts it as evidence, and treat any other or ambiguous shape (zero items, more than one item, item `referenceCode` not equal to the requested subscription) as "cannot verify", which leads to a durable unresolved record, never a grant.

## 4. What this means for the first-checkout and order alias

- The initial checkout can be bound to a subscription (F6: store `data.referenceCode` as `subscription_ref`), but not to its first order (F7).
- Iyzico does send a webhook for the initial order (F8). So every new subscription produces one `subscription.order.success` whose order is the same purchase the confirm already granted. A rule that grants on every new verified order reference double-grants every new subscriber.
- The detail `orders[]` cannot prove which order is the initial one, because neither ordering nor completeness is documented (section 2). Picking the earliest `startPeriod` or the earliest `paymentAttempts.createdDate` from that list, or the first notification to arrive, is exactly the inference the contract forbids.

Safe rule supported by the evidence:

1. The initial confirm grants as designed and stores `subscription_ref` from F6. `first_order_ref` stays null.
2. A verified order notification for a bound subscription whose `first_order_ref` is null is held in `billing.unresolved_notifications` with reason `initial_order_unproven`. No grant, 2xx after the row commits.
3. An admin resolves it with a recorded choice: "alias of the initial payment" (sets `first_order_ref`, no grant) or "separate order" (grants once under `(iyzico, orderReferenceCode)` and sets `first_order_ref` to the initial alias only if the admin also identifies it). The admin can consult the detail response; the decision is human, recorded, and replayable.
4. Once `first_order_ref` is set, a verified success with a different order reference for that subscription is provably a different order and grants once. The same reference as `first_order_ref` is a no-op.
5. Unbound subscriptions (all legacy subscribers until P9) are held with reason `subscription_unbound`, as already agreed.

Cost: one manual resolution per new subscription. Production shows 14 PREMIUM and 8 BUSINESS open plan packages (`transgate/docs/production-facts.md:31`), so the volume is small.

Alternative needing a lead decision, not adopted here: grant plans from the webhook instead of the confirm (confirm only binds the subscription). Each order reference then grants exactly once with no alias at all, but packages arrive 10 to 15 s after the redirect instead of at confirm, and a missed webhook (3 attempts) leaves a paying customer unresolved until manual replay. That is a visible behavior change.

A candidate automatic rule ("the order whose `startPeriod` equals the subscription `startDate` is the initial order") is not supported by the docs and must not be used until sandbox or production responses prove it.

## 4a. conversationId as an identity proof: not documented, not usable

Investigated whether `orders[].paymentAttempts[].conversationId` carries the `conversationId` we send at subscription checkout initialize.

| # | Fact | Source |
|---|---|---|
| C1 | SDK init accepts any `conversationId` and sends it unchanged in the body | `lib/requests/CreateSubscriptionCheckoutFormInitRequest.js:9-16`, `lib/requests/BaseRequest.js:8-10` |
| C2 | SDK checkout form retrieve sends only `checkoutFormToken` as a path variable, no `conversationId` | `lib/requests/RetrieveSubscriptionCheckoutFormPathRequest.js:8-10`, `lib/resources/SubscriptionCheckoutForm.js:17-20` |
| C3 | Docs, init request: "Optional correlation value to match request/response pairs." Init response: "Echoes the value you sent in the request." | subscription-transactions page (English); Turkish page items 1 and 2 |
| C4 | Docs, checkout form result: the result endpoint takes its own optional `conversationId` query parameter, and the response "echoes the value you sent in the request", meaning that retrieve request, not the init | same pages |
| C5 | Docs, detail `paymentAttempts[].conversationId`: "ID sent for request/response correlation. Echoed back in the response." (Turkish: "İstek/yanıt eşleştirmesi için gönderilen ID değeri. Gönderilen bilgi yanıtta aynen geri döner.") It does not name which request, and an automatic renewal attempt has no merchant request at all | same pages |
| C6 | No text says `conversationId` must be unique, or that the initial order's payment attempt carries the init `conversationId` | same pages |
| C7 | Transgate sends the fixed value `"123456789"` on every subscription init (`frontend/src/pages/checkout/index.tsx:354`) and every retrieve (`checkout/confirm.tsx:301`) | Transgate source |

Conclusion: the docs define `conversationId` only as a per-call echo. Equal field names do not establish that a payment attempt carries the init value, so this is not an identity proof and the adapter must not use it to resolve the alias. Even if a sandbox later showed the link, every legacy subscription shares `"123456789"` (C7), so it could never identify legacy initial orders.

Low-cost step that changes no outcome: the new runtime sends a unique `conversationId` per subscription init (our payment id) and stores it. If sandbox or production responses later prove the link (an observed detail response where the initial order's attempt carries that value and renewal attempts do not), the lead can turn it into an automatic alias rule with that recording as evidence. Until then it is data only.

## 4b. Rule wording: no first-observed heuristic

The section 4 rule is clarified so arrival order never decides identity. Counterexample it must survive: the initial order's webhook fails all three deliveries, the first renewal a month later is the first notification to arrive, then an admin replays the initial one. A "first observed order is the initial" rule would mark the renewal as the alias and grant the replayed initial order as a second set.

- While a bound subscription has `first_order_ref = null`, every verified order notification for it is held unresolved. None is auto-granted and none is auto-marked as the alias, whatever arrived first.
- `first_order_ref` is set only by an explicit admin identification of the initial order (or later, by a lead-approved provider proof such as 4a). Marking a held order as "separate order" grants it once and leaves `first_order_ref` null unless the admin also identifies the initial order.
- Only after `first_order_ref` is set does a verified success with a different reference grant automatically. The same reference is a no-op. A replayed held notification goes through this same path, so it cannot grant twice.
- Every held or granted order stays unique under `(iyzico, orderReferenceCode)`, so replay of any single order is idempotent.

## 5. Current Transgate behavior implication (P9 evidence item, not P2 scope)

Given F8, today's handler (`subscription-callback.ts:77-79`, `iyzicoRenewal.ts:19-41`) inserts a "renewal" payment with `token = orderReferenceCode` for the initial order too, and its `pending -> completed` update fires the package trigger again. Unless the webhook was not configured at the time, new subscribers likely received two package sets at signup. P9 should count plan payments with `analytics.payment_origin = 'renewal'` created within minutes of an `initial` payment for the same `customer_id`. This is an inference from documentation, not an observed production fact.
