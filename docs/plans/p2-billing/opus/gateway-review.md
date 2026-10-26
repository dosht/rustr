# Opus review of the gateway adapters

Read-only review, 2026-10-24, of `modules/billing/gateways/iyzico/src/{client,webhook,sign,lib}.rs`, `lemonsqueezy/src/lib.rs`, both crates' `tests/` and `fixtures/PROVENANCE.md`, against `implementation-contract.md`, `decisions.md` D3/D4, `gateway-evidence.md` (including the tax addendum), `signature-vectors.json` and `opus/provider-identity-evidence.md`. Where core decides what an adapter result means, the relevant `billing-core/src/app.rs` lines were re-read. No build, test or edit. Hosts are Astra's.

## Medium

### G1. A declined subscription checkout can no longer be recorded as failed

- Where: `iyzico/src/client.rs` `call` (lines 105-111) turns any `status: failure` envelope into `GatewayError::Rejected`; the subscription branch of `result` (lines 279-293) has no payment-status field to read; core `confirm_checkout` (`app.rs` lines 727-732) now maps `Rejected` on the lookup to `Uncertain`.
- Effect: for a plan checkout there is no path to `CheckoutResult::Failure`. A card declined inside Iyzico's subscription form returns, as far as the docs and source show, an error envelope, so the payment stays `pending` and the customer gets the "provider uncertain" answer instead of the failure page (BIL-060, WEB-BIL-013/014). This follows from my core-review L5 recommendation, which I still think is right for configuration and system errors, but it removed the only failure signal plans have.
- Fix, lead's choice: (a) classify Iyzico error codes that mean a payment decline as `Failure`, only with a primary-doc code list cited in a test; or (b) record in `decisions.md` that a plan confirmation without a success answer is "not confirmed" (host shows a neutral "we could not confirm this payment" page, admin reconciliation available), so nobody later reads the missing failure page as a regression.

### G2. Non-final `paymentStatus` values are recorded as a definite failure

- Where: `iyzico/src/client.rs` `result`, pay-as-you-go branch (lines 266-277), via `billing_core::grants::observation_from_status` (anything but `SUCCESS` is `Failed`).
- Evidence: Iyzico's CF-Retrieve documentation lists `paymentStatus` values `SUCCESS`, `FAILURE`, `INIT_THREEDS`, `CALLBACK_THREEDS`, and the retrieve-payment docs add `BKM_POS_SELECTED`, `CALLBACK_PECCO` (https://docs.iyzico.com/en/payment-methods/direct-charge/checkoutform/cf-implementation/cf-retrieve, https://docs.iyzico.com/en/advanced/retrieve-payment). `INIT_THREEDS` and `CALLBACK_THREEDS` are 3-D Secure steps, not outcomes.
- Counterexample: a confirm that reads `CALLBACK_THREEDS` marks the payment `failed` and emits `PaymentFailed`; the money may still settle. DEF-004 means a later reconcile can still grant, but the failed record and event are false, against D3 ("definite provider observations drive transitions").
- Fix: in the adapter, `SUCCESS` (any case) gives `Success`, `FAILURE` (any case) gives `Failure`, every other value gives `GatewayError::Uncertain` naming the status. Add the four documented non-final values to `tests/protocol.rs`. This narrows the source rule "anything else is failed" for documented intermediate states only; note it next to DEF-005.

### G3. LemonSqueezy buyers are sent back to the Iyzico confirmation route

- Where: core `start_checkout` builds one `callback_url = <public base>/checkout/confirm` for every gateway (`app.rs` around line 561); `lemonsqueezy/src/lib.rs` `create_body` uses it as `product_options.redirect_url` (line 85).
- Effect: LemonSqueezy redirects the buyer's browser with a GET and no Iyzico token. The legacy confirm route answers a non-POST with the failure result (`checkout/confirm.tsx:124-126`), so a customer who has just paid lands on a failure page if the host keeps that behavior. Packages still arrive through the webhook.
- Fix: give each gateway its own return URL. For LemonSqueezy use a return route that shows the payment's current status by our payment id (pending until the webhook, then completed), never a token lookup. Astra should check the host side.

## Low

### G4. Missing LemonSqueezy order fields default to passing values

- Where: `lemonsqueezy/src/lib.rs` `verify` lines 273-279: `quantity` defaults to 1, `discount_total` and `tax` to 0, `tax_inclusive` to false.
- Effect: a signed payload missing `quantity` or `discount_total` passes core's quantity and discount checks by default. The signature makes this hard to exploit, but the contract asks the adapter to check these fields, not assume them.
- Fix: require `first_order_item.quantity`, `discount_total`, `tax` and `tax_inclusive` (all documented order attributes); a missing one is `Rejected` as unreadable.

### G5. The tax-mode consistency check from the evidence is not implemented

- Where: core `order_in` (`app.rs` lines 1290-1296) compares the quote to `subtotal - discount_total` (correct per the addendum) but never checks `total`.
- Evidence: `gateway-evidence.md` line 45: "Retain tax and actual charged total separately and check their mode-specific consistency."
- Fix: inclusive: `total == subtotal - discount_total`; exclusive: `total == subtotal - discount_total + tax`; otherwise hold as `amount_mismatch`. Test both modes with the addendum's numbers (745/124/745 inclusive, 1500/300/1800 exclusive).

### G6. Price strings differ from the only client format proven in production

- Where: `iyzico/src/client.rs` `payg_body` line 168 sends `Decimal::to_string()` (`"14.90"`, `"616.00"`).
- Evidence: the installed SDK sends `parseFloat(price).toString()` plus `.0` for integers (`iyzipay/lib/utils.js:40-49`), so production has only ever sent `"14.9"` and `"616.0"`. The docs' examples use `"1.0"`, `"1.2"`. I found no statement that trailing zeros are rejected, so this is a risk, not a defect.
- Fix: format provider prices like `formatPrice` (strip trailing zeros, keep at least one decimal) and assert `"14.9"`, `"616.0"`, `"393.36"` in the protocol test.

### G7. The pay-as-you-go result is not tied back to the token that was asked about

- Where: `iyzico/src/client.rs` `result`, pay-as-you-go branch.
- Fix: when the answer carries `token`, require it to equal the requested token, otherwise `Uncertain`. Cheap defense against a misrouted or cached answer.

### G8. Provenance wording does not match the adapter

- Where: `iyzico/fixtures/PROVENANCE.md` lines 16-17 say "the code treats an error envelope as uncertain, not as a failed payment". The adapter returns `Rejected` (asserted in `tests/protocol.rs` line 198-202); it is core that treats `Rejected` on the lookup as uncertain.
- Fix: say which layer does what, so a later adapter change does not rely on a property the adapter never had.

## Checked and correct

- `IYZWSv2`: `HMAC-SHA256(secret, nonce + path + body)`, base64 of `apiKey:..&randomKey:..&signature:..`, `x-iyzi-rnd` carries the nonce; GET signs `{}` and sends no body; the bytes signed are the bytes sent (`call` serializes once). The literal vector in `sign.rs` matches `signature-vectors.json`, and the fixture server recomputes the signature from the received bytes.
- V3 subscription signature: published sample order `merchantId + secret + eventType + subscriptionRef + orderRef + customerRef`, hex, constant-time `verify_slice`, either hex case; the prose order is shown to produce a different digest and is not accepted. Missing header, empty, non-hex, wrong secret, other merchant, any changed identifier and non-JSON bodies are rejected before any field is used. Both failure names map to `Failure`; other signed types are acknowledged and ignored. No amount is read from the body.
- Path identifiers are percent-encoded as one segment; the test proves `a/b?c` cannot change the URI and is still signed correctly.
- Transport errors, timeouts, refused connections, HTTP 5xx, non-JSON bodies and missing `status` are `Uncertain`; checkout init failure envelopes are `Rejected` and create nothing.
- Subscription result: `data.referenceCode` is required as the subscription reference (matches `provider-identity-evidence.md` F6), missing gives `Uncertain`.
- D4 detail parse: exactly one documented `data.items[]` entry or the source-derived direct `data`, never both; zero or several items, another subscription's reference, non-list `orders` all give `Uncertain`; orders are listed as given, never reordered or picked. With core's fail-closed customer check, a resolution needs both the order listed and the bound customer named.
- LemonSqueezy: `X-Signature` HMAC over raw bytes, verified before JSON parsing, pinned by the non-ASCII literal vector; whitespace and Unicode-normalization changes fail. Checkout body uses `custom_price` in positive whole cents, `enabled_variants` restricted to the configured variant, `checkout_options.discount: false`, `checkout_data.custom` correlation, `test_mode` from configuration; non-cent, zero, negative amounts and plan checkouts never reach the provider. 4xx with errors is `Rejected`; 5xx, malformed, missing id or url and timeouts are `Uncertain`. Core compares the quote to `subtotal - discount_total` as the tax addendum requires.
- Neither adapter has a fake fallback: incomplete configuration is refused at construction, and LemonSqueezy's `result` lookup is explicitly `Uncertain` (orders confirm only by signed webhook).
