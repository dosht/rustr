# P2 integration repair contract

Lead owns integrated tests, runtime smoke, root manifests/lockfile, gate/ledger/docs integration. The user subsequently authorized Opus implementation; ownership is split below. Source handoff compiled after fixture repairs; the first real workspace test run stopped at `crates/taskr-server/tests/billing.rs:214` with `payments_provider_token` duplicate (HTTP 409 instead of 200). Many earlier suites passed; this is not a complete green run. Do not rerun this known failure merely to confirm it.

## Sonnet: core and host integration owner

Own `modules/billing/{core,http}`, `transgate/billing`, `crates/taskr-billing`, existing taskr/transgate server compositions/tests, except the two identity files assigned below. PostgreSQL belongs to Opus w23:p1R; gateway crates belong to Astra w23:p1V; the two JWT identity files belong to Opus w23:p1W. Sonnet owns all shared core interface changes and receives exact proposed seams from these owners. Items 1 and 4 below are implemented by the PostgreSQL owner; item 2 by the identity owner. Other items remain Sonnet's. Skip integrated tests/formatters until the repair handoff; one final `cargo test --locked --workspace --all-targets --no-run` is allowed after all three peers confirm source readiness.

Required corrections from `astra/host-review.md`:

1. Restore BIL-042 cascade-by-authorized-subject behavior for imported cross-account rows. Use a narrow deletion seam that discovers affected package owners and locks every affected account in deterministic order before host deletion/ledger mutation. Do not silently retain the anomalous consumption. Cover imported negative and positive usage and concurrent affected-owner debit. Imported anomalies remain reportable before deletion; runtime cannot insert/update import data.
2. Bound JWKS cached-hit trust in both actual verifiers. Refresh expired sets, reject removed keys, fail explicitly on refresh failure. Local rotation/removal evidence through both verifier paths; do not add a second auth convention.
3. Supply the missing trusted signup/contact producer. Minimal choice: configured trusted auth-directory lookup on authenticated Transgate access, obtaining real account creation time and verified contact facts before provisioning. A receiver invoked only by tests is insufficient. Never use JWT iat/request time for signup, never grant another trial on refresh. Preserve original creation time; contact refresh must not let stale signup replay replace newer verified facts. Compose and fixture-test the actual path.
4. Validate `billing_runtime` group attributes/effective SET ROLE authority too, not only the login. Reject contaminated existing groups at startup/provisioning. Runtime remains restricted; no owner-pool shortcut.
5. Required Transgate mail configuration must gate readiness/startup. Prove actual configured HTTP delivery failing and recovering after restart, with outstanding target marks and stable keys. Do not claim taskr mail/outbox delivery: that sample has no such product policy or relay. Keep hosts on separate smoke databases so one host's relay cannot consume another host's events.
6. Dispatch captured server analytics through a real configured host port/HTTP adapter from the transition/outbox, with stable dedup identity and local fixture proof. BIL-072 is not satisfied by returning AnalyticsIntent alone. Client GA/countdown rendering remains P5.
7. Add a gateway-specific LemonSqueezy return URL and an authenticated GET payment-status return route. It reads the trusted stored payment for the caller, pending until the webhook and completed afterward; it never calls Iyzico or grants on redirect. No HTML/UI invention is required for this P2 JSON HTTP contract. Return must be cross-account safe and get the actual payment id into the URL.
8. Enforce LemonSqueezy total consistency in core: inclusive total = subtotal - discount; exclusive total = subtotal - discount + tax. Keep quote comparison subtotal-discount. Reject negative/invalid amounts and nonzero unsupported provider discounts. Add the primary-doc nonzero-tax cases.
9. Make fixed fixture provider-token/order identities unique per test namespace across the entire shared database, including repeated suite runs. Repair the observed taskr-server collision without weakening production uniqueness or serializing away isolation failures.
10. Remove permanent tautological/source-shape/wiring tests in new code, replacing ledger references with meaningful behavior coverage where needed. Especially method-name/default-string/absence-of-a-function tests are not proof of business behavior. BIL-066 must reference actual buyer/sales/support delivery, not only lease behavior. Do not mark a row verified yourself.

### Shared async order verification interface

The public Order object does not document `first_order_item.quantity`; the separate Order Item resource does. Therefore require actual retrieval when the signed event omits quantity, not a default of 1 and not blanket rejection of documented valid events.

Sonnet changes only the core interface and its non-gateway callers/fakes:

`OrderWebhook::verify<'a>(&'a self, headers: &'a Headers, body: &'a [u8]) -> BoxFuture<'a, Result<OrderNotification, WebhookError>>`.

Add `WebhookError::Unavailable(String)` for authenticated provider lookup uncertainty; map to a retryable 503/504 outcome, not 401 or a grant. Core awaits verification before opening a transaction. Migrate HTTP/Postgres scripted fakes to ready futures. Lead already ran LSP references at ports.rs:326; references are core app, Lemon adapter/tests, HTTP tests and Postgres test support. Notify gateway owner when saved. No compatibility shim.

## Astra: gateway repair owner

Own only `modules/billing/gateways/{iyzico,lemonsqueezy}/**` and your repair evidence note. Sonnet owns core ports/app and all host/HTTP callers. No root manifests/lockfile edits. No builds/tests/formatters mid-flight; report complete source to Sonnet/lead before the one integrated compile/test pass. Use LSP for symbol references and surgical Edit, not sed/Python text replacement.

Required corrections from `opus/gateway-review.md`:

- Iyzico PAYG result: SUCCESS -> success, FAILURE -> definite failure, documented intermediate/unknown statuses -> uncertainty with unchanged business status. Validate returned token when present against the requested token. Do not classify auth/config/system lookup envelopes as payment failure. A subscription lookup with no authoritative outcome remains explicitly not confirmed/uncertain; do not invent a decline-code list. Init rejection remains distinct. Update tests/provenance and remove tests pinning non-final outcomes as failure.
- Preserve exact quote but serialize Iyzico prices like the observed SDK: strip redundant trailing zeroes, retain at least one fractional digit for integral values. This changes representation, not amount.
- Lemon signature verification still precedes parsing/network. Migrate to the shared async OrderWebhook interface. Require documented discount_total, tax, tax_inclusive rather than passing defaults. If quantity is absent from first_order_item, GET the documented `/v1/order-items/{id}` using configured API credentials; validate response type/id/order_id/variant_id/price against the signed event and configured sale, then require its positive quantity. Do not trust an arbitrary returned order/item identity. Use the configured API URL/credentials already on LemonClient; no hard-coded live endpoint and no request for an unsigned/invalid event. Network/malformed lookup produces retryable unavailable, no grant. If a signed payload does contain quantity, validate it rather than silently replacing it.
- Primary sources: https://docs.lemonsqueezy.com/api/orders/the-order-object, https://docs.lemonsqueezy.com/api/order-items/the-order-item-object, https://docs.lemonsqueezy.com/api/order-items/retrieve-order-item, and the tax examples in `gateway-evidence.md`. Do not claim first_order_item.quantity is required by the Order docs; that omission caused the original defaulting bug.
- Add local real-HTTP fixtures for omitted quantity resolved by order-item GET; wrong order/item/variant/amount; unavailable lookup; invalid signature causing zero outbound lookup; inclusive/exclusive monetary fields. Adapt existing direct webhook tests to async. Keep KATs/provenance synthetic and independently sourced.

## Opus: PostgreSQL repair owner

Pane w23:p1R owns `modules/billing/postgres/**` and `opus/postgres-repair-evidence.md`: items 1 and 4, meaningful concurrency/authority regressions, and any still-open prior PostgreSQL review findings. It sends required core deletion-seam signatures to Sonnet, not concurrent core edits. Sonnet hands off any in-flight PostgreSQL test-support changes explicitly. No builds/tests/formatters mid-flight.

## Opus: JWT identity repair owner

Pane w23:p1W owns only `crates/taskr-server/src/identity.rs` and `transgate/billing/src/identity.rs`, their internal regression tests/synthetic fixtures and `opus/identity-repair-evidence.md`. Preserve public constructors and callers. Prove expiry, rotation, removal, refresh failure and fresh-cache behavior through actual verification paths with deterministic time control and a loopback JWKS server. Sonnet keeps signup/contact composition outside these files or coordinates changes through this owner. No builds/tests/formatters mid-flight.

## Completion handoff

Each owner reports files, exact test names, any remaining source/protocol prerequisite and whether compilation was actually executed. No placeholders, no fake live fallback, no hidden policy change. Source-complete is not verified. Lead executes the full workspace, per-package P2 gate, both actual processes, restricted DB role, provider/mail/auth/analytics loopback paths and Docker health after the source stabilizes.
