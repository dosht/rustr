# Gateway protocol evidence for P2

Public provider documentation read by lead on 2026-10-24. This is not sandbox or production verification. Fixtures derived from these documents must be labeled documentation-derived, not recorded real transactions.

## Iyzico

Source: <https://docs.iyzico.com/en/advanced/webhook.md>.

- Current notification signature header is `X-IYZ-SIGNATURE-V3`; older versions are deprecated. The merchant must ask Iyzico to enable signature delivery. Missing or invalid signatures must not be accepted just because deployed activation is unknown.
- Subscription sample code in Node, PHP and Java computes HEX(HMAC-SHA256(secret, merchantId + secret + eventType + subscriptionReferenceCode + orderReferenceCode + customerReferenceCode)). The preceding prose lists secret before merchantId, unlike the sample code. Pin the actual published sample algorithm and document the inconsistency; do not invent a raw-body HMAC for this gateway. Merchant ID comes from trusted adapter configuration; eventType maps to the payload's `iyziEventType`.
- Official example failure event is `subscription.order.failure`. The current Transgate source ledger says `subscription.order.failed`. Both must be examined explicitly in adapter design, not silently conflated with every non-success notification.
- Subscription webhook JSON omits amount and currency. Signature verification authenticates the listed identifiers and event type, not arbitrary package metadata. Resolve purchases/subscriptions from stored trusted state and verify through the provider API where the contract requires it.
- HPP signature is a different concatenation: secret + iyziEventType + iyziPaymentId + token + paymentConversationId + status. Do not reuse the subscription algorithm for HPP.
- Documentation states delivery after 10-15 seconds and retries every 15 minutes up to three attempts. Durable processing is required; unlimited provider retries cannot be assumed.
- The official .NET example <https://github.com/iyzico/iyzipay-dotnet/blob/master/Iyzipay.Samples/Webhooks/WebhookSignatureValidation.cs> covers HPP/direct only, not subscription. It cannot be cited as a second independent subscription formula proof.

### Installed SDK request protocol

Read-only source: `/home/mu/Business/Transgate/frontend/node_modules/iyzipay/lib/IyzipayResource.js:18-49,80-86,107-115` and `utils.js:16-35`.

`Authorization: IYZWSv2 <base64>` encodes `apiKey:<key>&randomKey:<nonce>&signature:<hex>`. The signature is HMAC-SHA256(secret, nonce + generated_path + JSON.stringify(body)); `x-iyzi-rnd` carries the same nonce. Serialize once and sign the exact transmitted bytes. This installed SDK signs `{}` for operations without a request model, including subscription retrieval, not an empty string. A documentation/source-derived known-answer fixture should fix key, nonce, path and body and assert the literal signature, rather than recomputing the expected value with the implementation under test.

Installed resource paths:

- Pay-as-you-go start: POST `/payment/iyzipos/checkoutform/initialize/auth/ecom`.
- Pay-as-you-go result: POST `/payment/iyzipos/checkoutform/auth/ecom/detail`.
- Subscription start: POST `/v2/subscription/checkoutform/initialize`.
- Subscription checkout result: GET `/v2/subscription/checkoutform/{checkoutFormToken}`.
- Subscription retrieval: GET `/v2/subscription/subscriptions/{subscriptionReferenceCode}`.

These are source-observed protocols, not proof that any live merchant account accepts a request. Path identifiers must be encoded as a single segment, never allowed to alter the request URI.

## LemonSqueezy

Sources:

- <https://docs.lemonsqueezy.com/api/checkouts/create-checkout>
- <https://docs.lemonsqueezy.com/help/webhooks/signing-requests>
- <https://docs.lemonsqueezy.com/api/orders/the-order-object>

`POST /v1/checkouts` accepts `custom_price` as positive integer cents, store/variant relationships, `checkout_data.custom`, `test_mode`, and `product_options.enabled_variants`. This supports arbitrary whole-hour pay-as-you-go without converting the catalog to fixed bundles. The configured variant must be a one-off product; minimal integration does not add subscriptions. Restrict enabled variants and disable provider-side discount entry when the application has already calculated the price, so the purchased package does not diverge from the trusted quote.

`X-Signature` is HEX(HMAC-SHA256(signing_secret, exact_raw_request_body)). Verify before JSON processing and compare in constant time. Missing, malformed or wrong signatures reject without granting packages. Custom data is correlation data, not authorization to choose a user or price independently of a stored checkout.

An order has distinct `subtotal`, `discount_total`, `tax`, `total`, `tax_inclusive`, currency, store ID, `first_order_item.variant_id`, `test_mode` and status. Paid is a specific status, not any order-created payload. Receipt URL is `urls.receipt`. Compare the trusted configured item quote to `subtotal - discount_total` in both tax modes, not a tax-subtracted inclusive amount. Provider discounts are disabled for an already-priced application quote. Retain tax and actual charged total separately and check their mode-specific consistency.

No provider credentials were read and no checkout or payment was created while collecting this evidence.

### Tax-inclusive clarification from primary documentation

<https://docs.lemonsqueezy.com/help/payments/sales-tax-vat> explicitly shows the same configured USD 15 price as subtotal 15, tax 3, total 18 in exclusive mode, versus subtotal 15, tax 2.50, total 15 in inclusive mode. Therefore subtracting inclusive tax from subtotal before comparing with `custom_price` rejects a legitimate paid order. A USD 7.45 inclusive fixture has subtotal 745, tax 124, total 745; its quote comparison is 745, not 621. Test both modes with nonzero tax. This is documentation evidence, not a live transaction.
