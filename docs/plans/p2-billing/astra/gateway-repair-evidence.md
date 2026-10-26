# Astra gateway repair handoff

Source ready, **not execution-verified**. No builds/tests/formatters, provider transactions, credentials reads, source Transgate edits or hidden subagents. Lead owns integrated compilation, tests and runtime. Public documentation was read; local HTTP fixtures were written, not executed. Source-ready messages sent to Sonnet w23:p1T and lead w23:p1E.

## Changed files

- `modules/billing/gateways/iyzico/{src/client.rs,tests/protocol.rs,fixtures/PROVENANCE.md}`
- `modules/billing/gateways/lemonsqueezy/{src/lib.rs,tests/webhook.rs,fixtures/PROVENANCE.md}`
- This note. No manifest, lockfile, core, HTTP, host or ledger edits.

## Iyzico

Only case-insensitive SUCCESS/FAILURE establish PAYG outcomes. Intermediate/unknown statuses and rejected lookup envelopes are `GatewayError::Uncertain`; initialization remains `Rejected`. Subscription detail shares this lookup boundary. No subscription decline-code list is inferred. A returned PAYG token must match. Decimal wire formatting strips redundant zeros, retaining `.0` for integral values without f64 conversion.

Exact regressions in `tests/protocol.rs`:

- `bil_060_only_success_and_failure_are_final_payment_statuses`
- `def_005_lookup_error_envelopes_never_establish_payment_failure`
- `a_returned_token_must_match_before_any_payment_outcome_is_accepted`
- `provider_prices_follow_sdk_decimal_format_without_changing_the_quote`

Existing malformed/timeout/init-rejection test retained; `def_007_the_subscription_form_result_yields_the_subscription_reference_and_a_get_signs_empty_braces` now expects uncertainty on failed lookup. Lead must migrate ledger references from the removed misleading name `bil_060_a_payment_status_in_any_letter_case_is_a_success_and_anything_else_a_definite_failure`.

Sources: [CF-Retrieve](https://docs.iyzico.com/en/payment-methods/direct-charge/checkoutform/cf-implementation/cf-retrieve), [retrieve-payment](https://docs.iyzico.com/en/advanced/retrieve-payment), SDK evidence in `opus/gateway-review.md` G6. Current retrieve-payment documentation enumerates SUCCESS/FAILURE/INIT_THREEDS/CALLBACK_THREEDS; wallet-state tests retain the approved review/repair contract and remain uncertain regardless of provider version.

## LemonSqueezy

Adopts Sonnet's saved async `OrderWebhook::verify` / `WebhookError::Unavailable(String)` interface. `LemonClient::webhook()` shares the configured HTTP client, timeout, API base and credentials. Standalone `LemonWebhook::new` removed; LSP found only gateway-test callers, migrated. Host factory API and callback URL forwarding remain unchanged.

Signature verification precedes JSON/network. Missing quantity triggers authenticated `GET /v1/order-items/{id}`. Present quantity must be a positive integer and is never replaced. Retrieval binds resource type/id/order/variant/price to the signed event/configured sale, and checks price × quantity against subtotal. Malformed/mismatched/unavailable retrieval returns `Unavailable`; core maps this to retryable 504 before any transaction. Discount/tax/tax-mode fields are required, not defaulted.

Exact regressions in `tests/webhook.rs`:

- `documented_order_without_quantity_retrieves_and_binds_the_order_item` — authenticated loopback GET, quantities 1 and 2 preserved.
- `retrieved_item_must_match_signed_identity_variant_price_and_subtotal` — identity/amount mismatch, invalid quantity and overflow.
- `unavailable_order_item_lookup_is_retryable_and_a_later_delivery_can_succeed` — 401/404/429/500, malformed response, timeout/refusal and recovery.
- `missing_malformed_or_wrong_signatures_are_rejected_without_reading_the_payload` — zero outbound requests.
- `present_quantity_is_validated_without_lookup_and_never_silently_replaced`
- `missing_or_malformed_money_fields_are_rejected_before_item_lookup`
- `lookup_requires_signed_item_identity_and_the_configured_sale`
- `primary_document_tax_examples_preserve_inclusive_and_exclusive_amounts` — inclusive 745/124/745 and exclusive 1500/300/1800 subtotal/tax/total with item retrieval.

Existing raw UTF-8 KAT, paid-order mapping and unsupported-event tests migrated to async. Obsolete constructor tests/expectation-getter assertions removed; no compatibility shim. Fixtures/KATs remain synthetic, not live captures.

Sources: [Order](https://docs.lemonsqueezy.com/api/orders/the-order-object), [Order Item](https://docs.lemonsqueezy.com/api/order-items/the-order-item-object), [retrieve Order Item](https://docs.lemonsqueezy.com/api/order-items/retrieve-order-item), tax examples in `gateway-evidence.md`. Order documentation omits first_order_item.quantity; the separate Order Item documents it.

## Remaining verification

No known source/protocol blocker for local verification. Core owns stored-quote checks, quantity-one policy, negative/discount rejection and tax-mode total consistency. Sonnet owns the gateway-specific return route.

Lead: integrated gate plus `cargo test --locked -p billing-iyzico -p billing-lemonsqueezy` (not run here). Runtime smoke should cover a signed order lacking quantity, loopback retrieval, unavailable lookup then redelivery, and invalid signature with zero GETs. Live provider behavior remains unverified; no ledger row marked verified.

## Integrated compile feedback

Sonnet reported an Axum `Handler` bound error at Lemon `tests/webhook.rs:210`.
The fixture handler now consumes the request into parts and scopes its header
closure and mutex guards inside a synchronous block, carrying only the owned
reply across the delay await. The previous closure borrowed the non-`Sync`
request body. Repair saved; recompilation remains with Sonnet/lead.
