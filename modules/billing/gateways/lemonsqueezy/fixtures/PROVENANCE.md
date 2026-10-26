# LemonSqueezy test fixtures: provenance

No file or test in this crate is a recording of a real LemonSqueezy store. No
provider call was made while writing it.

| Fixture | Where it lives | Derived from |
|---|---|---|
| Webhook signature vector over raw UTF-8 bytes, including a multi-byte body (`İstanbul`) | literal constant in `tests/webhook.rs` | `docs/plans/p2-billing/signature-vectors.json`, calculated by the lead with Python `hmac`/`hashlib` independently of the Rust code. Synthetic public inputs. |
| Checkout creation request (`custom_price`, `discount: false`, `checkout_data.custom`) and answer | loopback server in `tests/checkout.rs` | `docs/plans/p2-billing/gateway-evidence.md`, built from LemonSqueezy's public API documentation (custom price checkouts, order webhooks, tax-inclusive pricing) |
| Order webhook payloads (`order_created`, `subtotal`, `discount_total`, `tax`, `total`, `tax_inclusive`, `test_mode`, `first_order_item`) | `tests/webhook.rs`, the postgres and host runtime tests | the same documentation; the net amount is `subtotal - discount_total` in both tax modes |
| Missing webhook quantity resolved by authenticated order-item GET | loopback server in `tests/webhook.rs` | [Order object](https://docs.lemonsqueezy.com/api/orders/the-order-object), [Order Item object](https://docs.lemonsqueezy.com/api/order-items/the-order-item-object), [retrieve Order Item](https://docs.lemonsqueezy.com/api/order-items/retrieve-order-item). The documented `first_order_item` **does not include quantity**; the retrieved Order Item does. Synthetic IDs 5001/8001 and variant 7, not real transactions. |
| Nonzero inclusive/exclusive tax | `primary_document_tax_examples_preserve_inclusive_and_exclusive_amounts` | [Sales tax/VAT](https://docs.lemonsqueezy.com/help/payments/sales-tax-vat) and `gateway-evidence.md`: inclusive subtotal/tax/total 745/124/745; exclusive 1500/300/1800. Adapter preserves fields; core enforces quote and tax-mode consistency. |

Signature verification precedes JSON and all HTTP. The verifier is created by
`LemonClient::webhook`, sharing its configured HTTP client, timeout, API base and
credentials. No unauthenticated standalone verifier or hard-coded lookup endpoint
remains. A missing quantity triggers `GET /v1/order-items/{id}`; a present quantity
must be a positive integer and is never overwritten. Retrieval binds resource
type/id/order/variant/price to the signed event and configured sale, and checks
price × quantity against subtotal. Unavailable, malformed or mismatched lookup
answers return `WebhookError::Unavailable`, allowing a retry without granting.
`discount_total`, `tax` and `tax_inclusive` are required, not defaulted.

What this does **not** prove, and only an owner-run test-mode store can:

- that the store accepts the checkout body as built, and that `custom_price`
  is honored for the configured variant;
- the real field values of a tax-inclusive order versus a tax-exclusive one;
- the delivery order and retries of real webhooks;
- that `custom_data` round-trips unchanged to the order webhook.
