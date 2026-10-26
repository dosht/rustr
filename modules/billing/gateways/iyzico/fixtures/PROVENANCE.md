# Iyzico test fixtures: provenance

No file or test in this crate is a recording of a real Iyzico account, sandbox
or production. No provider call was made while writing it.

| Fixture | Where it lives | Derived from |
|---|---|---|
| Signature vectors (V3 subscription notification, IYZWSv2 GET authorization) | literal constants in `src/sign.rs` tests | `docs/plans/p2-billing/signature-vectors.json`, calculated by the lead with Python `hmac`/`hashlib`/`base64` from the documented and installed-SDK algorithms, independently of the Rust code. Synthetic public inputs. |
| Hosted-checkout and subscription request/answer shapes | loopback server in `tests/protocol.rs` | `docs/plans/p2-billing/gateway-evidence.md` (documentation- and installed-SDK-derived request protocol) and the discovery ledger rules BIL-070..071 |
| Subscription detail shapes (`data.items[]` and the source repository's direct `data`) | `tests/protocol.rs`, `parse_detail` tests | the same document; the two shapes are both handled explicitly and an answer that matches both or neither is uncertain |
| Signed order notifications | `tests/webhook.rs` | the V3 algorithm above, signed with a test secret that is not a credential |
| Final versus intermediate payment status | `bil_060_only_success_and_failure_are_final_payment_statuses` in `tests/protocol.rs` | [CF-Retrieve](https://docs.iyzico.com/en/payment-methods/direct-charge/checkoutform/cf-implementation/cf-retrieve), [retrieve-payment](https://docs.iyzico.com/en/advanced/retrieve-payment), and `docs/plans/p2-billing/opus/gateway-review.md` G2: only `SUCCESS` and `FAILURE` are final; documented intermediate and unknown values remain uncertain (DEF-005) |
| Exact decimal wire representation | `provider_prices_follow_sdk_decimal_format_without_changing_the_quote` in `tests/protocol.rs` | installed SDK `formatPrice`, as recorded in `docs/plans/p2-billing/opus/gateway-review.md` G6: `14.90` → `14.9`, `616.00` → `616.0`, `393.36` unchanged; no binary-float conversion |
| Misrouted token and error envelopes | `a_returned_token_must_match_before_any_payment_outcome_is_accepted`, `def_005_lookup_error_envelopes_never_establish_payment_failure` | synthetic adverse responses, not captured declines; returned PAYG token must match the requested token |

What this does **not** prove, and only an owner-run live or sandbox exercise can:

- that Iyzico's sandbox accepts these exact request bodies;
- the real response of a failed lookup: the adapter's lookup boundary converts
  rejected envelopes to `GatewayError::Uncertain`; initialization still returns
  `GatewayError::Rejected`. Without an authoritative success, a subscription
  lookup is not confirmed. No provider decline-code list is inferred;
- whether `orders[]` on the real subscription detail lists the initial order
  first (the first-order decision therefore never depends on order);
- delivery timing and retries of real notifications.

## P3 erasure-only termination (D10)

`IyzicoClient` implements the billing core's separate `SubscriptionTermination`
port; ordinary cancellation remains manual support. Protocol source:
[Subscription Transactions — Cancel and Detail](https://docs.iyzico.com/en/getting-started/preliminaries/api-reference-beta/subscription/subscription/subscription-transactions).
The authenticated request is `POST /v2/subscription/subscriptions/{reference}/cancel`;
the body is optional, so the adapter signs/transmits `{}`. Only a 2xx response with
the documented success envelope reports `CancelSucceeded`. An explicit failure
envelope may be followed by authenticated subscription detail; core alone decides
whether the bound customer and `CANCELED`/`EXPIRED` status prove termination.

The three `d10_*` cases in `tests/protocol.rs` use the existing independent
IYZWSv2 fixture verifier. They cover encoded request identity, wrong environment
without dispatch, authenticated detail with terminal/nonterminal status and
wrong customer, ambiguous detail, lost/malformed/5xx/contradictory-401 answers,
and absence of hidden repeat POSTs. HTTP auto-redirects and retries are disabled.
These additions were not executed by the transport owner; the lead owns the
integrated run. Real sandbox/production cancellation acceptance remains unproved.
