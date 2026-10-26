# P5 billing: published contract and handoff

Owner: w23:p1R (Opus). Slice: billing purchase and invoice presentation, owner invoice context, the display-versus-charge distinction, billing HTTP changes, and jobs-postgres feedback/support persistence against p1X/p1T ports. Status: **contract published, source in progress, nothing executed.**

All money and document facts are computed by billing core on the server or in the shared Rust presentation. No client arithmetic. Billing core stays independent of Transgate: it returns typed codes, and the Transgate core (p1X) maps them to message keys.

## 1. Display amount versus charged quote (D8, P2 D2, DEF-010)

`POST /api/billing/quote` keeps every existing `Quote` field and adds one field:

```json
{ "...existing Quote fields...": "...", "display_amount": "7.40" }
```

- `display_amount` (USD decimal string, two places) is what the source summary shows as the total, computed by `billing_core::pricing::display_amount`:
  - Pay as you go: `payg_display_total(catalog, hours, ui_locale)`; then, when an unexpired coupon's `filter_product` is exactly `"Pay-as-you-Go"` (the source display product name), `round_half_up(total * (1 - amount), 2)`.
  - Plan: the plan's `usd_price`. The source applies a display coupon to plans only for a filter object without a product, which the stored coupon cannot distinguish from no filter, so no display discount is shown for plans (recorded deviation).
- `amount` and `currency` stay the charged quote: what the provider is asked to charge.
- The two are never forced equal and never recomputed by a client. Example: 10 hours in `id` shows 7.40 and charges 7.45.

## 2. Purchase presentation (`billing_core::presentation::purchase`)

A pure state machine; `update(event) -> Vec<PurchaseEffect>`, `view() -> PurchaseVm`. Planned public symbols:

- `PurchaseState::new(catalog: CatalogView, ui_locale: String) -> PurchaseState`
- `PurchaseEvent`: `SelectPayAsYouGo`, `SelectPlan { package_id: usize }`, `SetYearly(bool)`, `PresetHours(i64)`, `HoursTyped(String)`, `StepHours(i64)`, `CouponTyped(String)`, `CouponSubmitted`, `CouponAnswered(CouponAnswer)`, `TermsAccepted(bool)`, `QuoteAnswered(Result<QuoteView, PurchaseFailure>)`, `PayRequested`, `CheckoutAnswered(Result<CheckoutView, PurchaseFailure>)`.
- `PurchaseEffect`: `FetchQuote(OrderBody)`, `FetchCoupon(String)`, `StartCheckout(OrderBody)`, `Handoff(CheckoutHandoff)`, `Analytics(CheckoutStarted)`.
- `PurchaseVm`: plan cards and PAYG card, the selection, yearly toggle, hours with `minimum_warning`, coupon field state (`CouponNotice` code: `NotValid`, `Expired`, `LookupFailed`), terms flag, `display_total: Option<Decimal>` (from `display_amount`), `charged_quote: Option<ChargedQuote { amount, currency }>`, `try_display: Option<Decimal>` for the Turkish locale, `renewal: RenewalNotice`, `can_pay`, `processing`, and `failure: Option<PurchaseFailure>` (typed).
- `OrderBody` is the exact JSON the existing `/api/billing/quote` and `/api/billing/checkout` accept: `package_id`, `pay_as_you_go_hours`, `currency`, `locale`, `coupon_id`, `gateway`, `analytics`.
- `CheckoutHandoff`: `Form { content }` (trusted provider form) or `Redirect { url }`, from the real `CheckoutView`; a missing handoff is a typed failure, never a guessed success.

Exact field lists are published with the source and generated through the Transgate fixtures (p22/p1X), not hand-written in TypeScript.

## 3. Owner invoice context (D7) and invoice facts for the document leaf (D2)

`GET /api/billing/invoices` (authenticated, own account only) returns `billing_core::presentation::invoice::InvoiceContext`:

```text
InvoiceContext { bill_to: BillTo, invoices: Vec<InvoiceFacts> }

BillTo {
  name: Option<String>,        // source chain: billing full_name, else company (when no full name),
                               // else customer full name, else email; None = "Valued Customer"
  company: Option<String>,     // only when it is not already the name
  address_lines: Vec<String>,  // billing address split on newlines, blanks dropped
  country: Option<String>,
  tax_id: Option<String>,
}

InvoiceFacts {
  payment_id: i64,
  invoice_id: String,          // TRAN + id padded to 8 digits
  date: String,                // dd/MM/yyyy of the payment's creation
  package_name: String,        // metadata package name, else "Subscription Plan"
  package_minutes: Option<i64>,
  quantity: i64,               // 1
  currency: String,
  subtotal: Decimal,           // CPN-018: TRY amount / 1.2, half up to cents; else the amount
  tax: Decimal,                // TRY total - subtotal; else 0.00
  total: Decimal,              // the stored amount
  tax_kind: TaxKind,           // Vat20 (TRY) | None
  gateway: String,             // "Payment processed through <gateway>"
}
```

- Facts are computed by billing core from completed payments, the latest `billing_info` and the customer directory row. They are the only financial input to the invoice document.
- The admin lookup (`POST /api/billing/admin/customer-invoices`, admin only) gains the same `invoice_context` for the looked-up customer, beside its existing fields.
- The temporary admin Bill To text is not part of any billing request or response. The document leaf (p1W) takes it as a separate, unstored argument and, when nonempty, prints its nonblank lines instead of `BillTo`, as the source does.
- Fixed seller identity, labels and footer text are document layout (p1W), not billing facts.

## 4. Feedback and support persistence (jobs-postgres)

**Landed in the window** (moves done, compiled edits applied). Support ack/release are fenced by the claim's `attempts` (p1X's updated port): `ack_support(&ClaimedSupport) -> bool`, `release_support(&ClaimedSupport, error, retry_at) -> bool`, false when superseded. Test renamed to `the_support_outbox_leases_oldest_first_and_only_the_current_claim_settles`.

Implements p1X's `transgate_core::account::ports::{AccountTx, SupportOutbox}` (`transgate/core/src/account/ports.rs`). Because compiled P4 files and migrations are held and core links `account` only in the next window, the source is **staged outside the build** in `transgate/jobs-postgres/staged/`:

- `staged/migrations/0007_account_feedback_support.sql`: `job_feedback` (rating CHECK 0/5/10, FK to `jobs` ON DELETE CASCADE, many rows per job, insert-only for the runtime) and `support_requests` (optional `job_id` without FK, `attempts`, `claimed_until`, `last_error`; runtime may update only those three).
- `staged/src/account.rs`: `impl AccountTx for PgJobUow` (inserts on the uow's connection; a missing job is `Conflict`) and `impl SupportOutbox for PgJobStore` (one-statement oldest-first lease with `SKIP LOCKED`, ack deletes, release sets `claimed_until = retry_at` and the error).
- `staged/tests/account.rs`: `feedback_is_appended_per_job_and_goes_with_its_job_and_its_account`, `the_support_outbox_leases_oldest_first_and_deletes_on_ack`.

Exact compiled edits to apply at the window (with the moves `staged/migrations/0007... -> migrations/`, `staged/src/account.rs -> src/`, `staged/tests/account.rs -> tests/`):

1. `src/lib.rs`: `mod account;`; allow-list `("job_feedback" | "support_requests", "INSERT") => true`, `("support_requests", "UPDATE") => attempts | claimed_until | last_error`; `delete_allowed` adds `job_feedback`, `support_requests`.
2. `src/uow.rs`: `fn account(&mut self) -> &mut dyn transgate_core::account::ports::AccountTx { self }` in `impl JobUow`; `erase_account_rows` adds `DELETE FROM transgate_jobs.job_feedback WHERE account_id = $1` and the same for `support_requests`; the hard-delete comment names feedback among the cascaded children.

## 4b. Subscription tab (WEB-BIL-022/023, requested by p1X)

`GET /api/billing/subscription` returns `billing_core::presentation::subscription::SubscriptionFacts` or `null`: the first open `PREMIUM`/`BUSINESS` package (same choice as `current_plan`), `minutes_included`, `valid_until`, and, when it is the caller's, `payment: {payment_id, created, canceled_at, next_payment_at}` with BIL-063 `next_payment_date`. `SubscriptionState::new() -> (state, [Load])`, `update(SubscriptionEvent, now) -> Vec<SubscriptionEffect>`, `view() -> SubscriptionVm`. Effects: `Load`, `Cancel {payment_id}` (`POST /api/billing/cancel-subscription`), `Analytics(SubscriptionCancelled)` (`EVENT = "subscription_cancelled"`, `{plan, days_subscribed}`), `OpenPurchase`. The stored `canceled_at` from `CancelView` replaces the source's local optimistic instant. A failed cancel keeps the confirmation open with a typed failure; no payment refuses with `NoActiveSubscription` before any request.

## 5. Browser confirmation page (D8, agreed with p1T)

`billing_http::{ConfirmOutcome, ConfirmPage}` and `BillingHttp::with_confirm_page(Arc<dyn ConfirmPage>)`. `/checkout/confirm` calls `confirm_checkout` exactly once. When a page renderer is configured AND the request is `application/x-www-form-urlencoded` AND `Accept` contains `text/html`, it answers `text/html; charset=utf-8`, `Cache-Control: no-store`, with the renderer's body and the status the JSON answer would have had. Every other caller keeps the existing JSON. `ConfirmOutcome` serializes as `{"outcome": "confirmed", "view": ConfirmView}` (pending included, as stored) or `{"outcome": "failed", "status", "code", "message"}`. p1T's renderer embeds it in `<script type="application/json" id="tg-confirm">` and escapes `<`, U+2028 and U+2029; billing never renders HTML itself.

## 6. D17 checkout document: one form post, one start

Routes in `billing_http::provider` (mounted at the site root):

- `GET /checkout` answers `303 Location: /` with `no-store`; it never starts a checkout.
- `POST /checkout` exists only when the host called `BillingHttp::with_checkout_page(page: Arc<dyn CheckoutPage>, site_origin: &str) -> Result<BillingHttp, String>`; otherwise 404. `site_origin` must be `https://host[:port]` (no path); plain `http://` is accepted only for `127.0.0.1`, `localhost` or `[::1]` fixtures.

Request, checked in this order, every refusal answered as a failure document (never a provider start):

1. `Content-Type: application/x-www-form-urlencoded` (single part), else 415.
2. `Origin` exactly equal to `site_origin`, else 403.
3. No `Authorization` header (one credential method), else 400.
4. Body at most `CHECKOUT_FORM_MAX_BYTES` = 32768 bytes, else 413.
5. ASCII form with exactly two fields, each once, any order: `order` and `access_token`. Unknown or repeated fields are 400.
6. `access_token`: nonempty visible ASCII, at most `CHECKOUT_TOKEN_MAX_BYTES` = 8192 bytes; missing or bad is 401. It is verified by the existing `BillingIdentity::authenticate` (as a Bearer header built in memory), so 401, 410 and 503 keep their meaning. It is never logged, echoed or rendered.
7. `order`: base64url **without padding** (RFC 4648 section 5) of the JSON of `billing_core::presentation::purchase::OrderBody` (`package_id`, `pay_as_you_go_hours`, `currency`, `locale`, `coupon_id`, `gateway`, `analytics` object). Decoded size at most `CHECKOUT_ORDER_MAX_BYTES` = 16384 bytes. Unicode stays inside the JSON. Bad encoding or JSON is 400; billing then validates it exactly as the JSON checkout does.

Then `Billing::start_checkout` runs **once**. Answers:

- Provider form: `200`, `text/html; charset=utf-8`, `Cache-Control: no-store`, `Content-Security-Policy` = the renderer's policy, body = the renderer's HTML.
- Provider hosted URL (`https://` only, LemonSqueezy): `303 Location: <url>`, `no-store`, no document.
- Any refusal or failure: the renderer's failure document with the status the JSON route would answer (502 `checkout_failed` when the provider returned neither a form nor a URL).

Seam (`billing_http`):

```rust
pub enum CheckoutOutcome {            // Serialize, tag "outcome", snake_case
    Form { payment_id: i64, gateway: String, content: String, locale: String },
    Failed { status: u16, code: String, message: String, locale: Option<String> },
}
pub struct RenderedCheckout { pub html: String, pub content_security_policy: String }
pub trait CheckoutPage: Send + Sync { fn render(&self, outcome: &CheckoutOutcome) -> RenderedCheckout; }
```

The renderer (host) creates the nonce, puts it on the document's own scripts and on the provider form's scripts, and returns the matching policy, so the HTML and its header can never disagree. Billing never builds HTML or a policy.

Provider origins: Iyzico documents no CSP allowlist, and 3-D Secure pages come from card-issuer domains that are not a fixed set. Billing therefore does not publish an origin list it cannot source. The lead decided the policy (D19, checkout document only): `default-src 'none'`; `script-src 'nonce-...' 'strict-dynamic'`; `style-src https: 'unsafe-inline'`; `font-src https: data:`; `img-src https: data:`; `connect-src https:`; `frame-src https:`; `form-action https:`; `base-uri 'none'`; `object-src 'none'`; `frame-ancestors 'none'`. No `unsafe-eval`, no inline script without the nonce.

Limits: the three byte limits are one portable contract, `billing_core::presentation::purchase::{CHECKOUT_FORM_MAX_BYTES, CHECKOUT_TOKEN_MAX_BYTES, CHECKOUT_ORDER_MAX_BYTES}`. `billing_http` re-exports them under the same names; the client imports them from billing-core instead of keeping copies.

Purchase leaf (clean cutover, no alias): `PurchaseEffect::SubmitCheckout(OrderBody)` replaces the JSON checkout start; `Handoff`, `CheckoutHandoff` and `CheckoutAnswered` are removed. The shell sets `order.analytics`, base64url-encodes the JSON and posts the hidden form to the page origin's `/checkout`, target `_self`. `PurchaseEvent::CheckoutNotSubmitted(PurchaseFailure)` returns control when the shell could not post (nothing was sent). The JSON `POST /api/billing/checkout` stays for other API consumers.

Tests: `d17_the_checkout_document_post_starts_once_and_carries_its_own_policy`, `d17_a_checkout_document_post_refuses_cross_site_mixed_credentials_and_malformed_forms` (billing-http), `paying_needs_terms_and_a_quote_and_submits_the_checkout_form_once` (billing-core).

## 7. Confirmation and invoice history leaves (for p1X)

`billing_core::presentation::confirm`: `ConfirmPageState::new(Option<ConfirmPayload>) -> (state, Vec<ConfirmEffect>)`, `update(ConfirmEvent::Tick)`, `view() -> ConfirmVm { kind: ConfirmKind, countdown: Option<u32>, payment_id: Option<i64> }`. `ConfirmPayload` is the page's embedded `ConfirmOutcome` JSON (section 5); `None` is a page reached without a confirmation (a GET). `ConfirmKind::{Succeeded, Failed, Unconfirmed}`: a success or a definitive failure counts down `redirect_after_seconds` (5) and emits `NavigateHome` once; `pending`, or a 5xx other than 502 `checkout_failed`, is `Unconfirmed` with no countdown. `ConfirmEffect::Track(Vec<ConfirmAnalytic>)` is emitted once, only when the confirmation changed the payment.

`billing_core::presentation::invoices`: `InvoicesState::new(is_admin) -> (state, [LoadOwn])`, `update(InvoicesEvent)`, `view() -> InvoicesVm { loading, rows: Vec<InvoiceRow>, preview: Option<i64>, admin: Option<AdminVm>, failure: Option<PurchaseFailure>, document_failure: Option<PurchaseFailure> }`. `InvoiceRow { payment_id, invoice_id, date, amount_text, package_name, gateway }`; `AdminVm { email, looking_up, viewing_customer, not_found, temporary_bill_to }`. Effects: `LoadOwn` (`GET /api/billing/invoices`, answer `Loaded(Result<InvoiceContext, PurchaseFailure>)`), `LookUp { email }` (`POST /api/billing/admin/customer-invoices`, answer `LookupAnswered(Result<AdminLookupReply, PurchaseFailure>)`, 404 mapped to `Refused { code: "not_found" }`), `Document { facts, bill_to, temporary_bill_to, purpose: Preview | Download }` (p1W's `InvoiceDocument::capture`). When capture, render, fonts, PDF, preview or save fails, the executor sends `DocumentFailed { payment_id, failure: PurchaseFailure::Unavailable }`: the preview for that payment closes, `document_failure` is set beside the still usable rows, and the next `Preview` or `Download` clears it. `is_admin` is the server-verified capability; administrator events from anyone else do nothing, and the temporary Bill To reaches only the document effect.

## 8. TypeScript and fixtures (feature `ts`)

billing-core has an optional `ts = ["dep:ts-rs"]` feature. With it, `ts_rs::TS` is derived (never exported by billing) on `PurchaseVm`, `PlanOption`, `PlanCardView`, `PaygCardView`, `ChargedQuote`, `PurchaseFailure`, `CouponMessage`, `Currency`, `PlanInterval`, `RenewalNotice`, `SubscriptionVm`, `PlanSummary`, `SubscriptionFailure`, `InvoicesVm`, `InvoiceRow`, `AdminVm`, `ConfirmVm`, `ConfirmKind`, `SpecialClientAlert`; `Decimal` is `string` and `Micros` is `number` (both as serialized). transgate-core enables `billing-core/ts` from its own `ts` feature and exports through its generator.

Fixtures, from real transitions, in `billing_core::presentation::fixtures` (always compiled, like transgate-core's):

| Function | Type key | Names |
|---|---|---|
| `purchase()` | `PurchaseVm` | `quote_pending`, `pay_as_you_go`, `display_differs_from_charge` (shows 7.40, charges 7.45), `turkish_lira`, `yearly_plan`, `coupon_expired`, `ready_to_pay`, `processing`, `failed` |
| `subscription()` | `SubscriptionVm` | `loading`, `active`, `no_plan`, `confirming`, `cancelling`, `canceled`, `cancel_failed` |
| `invoices()` | `InvoicesVm` | `loading`, `owner`, `empty`, `failed`, `preview`, `document_failed`, `admin`, `admin_looking_up`, `admin_customer`, `admin_not_found` |
| `confirm()` | `ConfirmVm` | `succeeded`, `counting_down`, `declined`, `pending`, `unconfirmed`, `no_confirmation` |

Each returns `Vec<(&'static str, Vm)>`, the shape transgate-core's `named` takes. `catalog()` and `invoice_context_sample()` are the synthetic server answers they start from.

## 9. Billing codes to message keys (P5 D10)

Billing view models carry typed codes, never text. Keys are the source's own (`common` namespace, `useTranslation("common")`); "(fallback)" marks a key the source passed to `t()` without a catalog entry, so the source showed the key itself.

| Code | Namespace and key |
|---|---|
| `CouponMessage::NotValid` | `common` "Coupon is not valid" (fallback) |
| `CouponMessage::Expired` | `common` "Coupon has expired" (fallback) |
| `CouponMessage::Error` | `common` "Error" (fallback) |
| `RenewalNotice::WillNotRenew` | `common` "Pay-as-you-go will not renew" |
| `RenewalNotice::InDays(n)` | `common` "Renews on " followed by the shell's locale date of now plus `n` days, as the source did |
| `PurchaseFailure::Unauthenticated` | `common` "Please sign in and try again." |
| `PurchaseFailure::Erased` | the app's existing erasure state (core `AppVm::Erased`), no billing key |
| `PurchaseFailure::Unavailable` (purchase) | `common` key "Something went wrong while preparing your payment. No charge was made (U+2014) please try again.", where (U+2014) stands for the one character the source key has there |
| `PurchaseFailure::Refused { code }` (purchase) | `common` "We couldn't start your checkout" |
| `SubscriptionFailure::NoActiveSubscription` | `app` new key `p5.billing.no_active_subscription` = "No active subscription to cancel" (source threw this English text) |
| `SubscriptionFailure::Request(_)` | as `PurchaseFailure` above, except `Unavailable`/`Refused` use `app` "An unexpected error occurred. Please try again." |
| `ConfirmKind::Succeeded` | `common` "Checkout", "Successful!", "Your order has been successfully processed.", "Thank you for transcibing with us!", `redirectMessage` with `count` = `countdown` |
| `ConfirmKind::Failed` | `common` "Checkout", "Failed" (source literal, untranslated), "Oops! Something went wrong during the checkout process.", "Please try again later or contact customer support.", `redirectMessage` with `count` |
| `ConfirmKind::Unconfirmed` | `app` new key `p5.billing.payment_unconfirmed` = "We could not confirm your payment yet. Do not pay again; check your billing history shortly." (no source state) |
| `InvoicesVm.failure` | `common` "This feature is currently unavailable" |
| `InvoicesVm.document_failure` | `app` "An unexpected error occurred. Please try again." (the source had no document failure state) |
| `AdminVm.not_found` | `app` new key `p5.billing.customer_not_found` = "No customer found for that email" (source showed the API error text) |
| `InvoicesVm.failure` during a lookup | `app` new key `p5.billing.lookup_failed` = "Failed to load customer invoices" (source fallback text) |

Static labels on these screens are the source's `common` keys as written ("Billing History", "Invoice Preview", "Look up customer invoices by email", "Viewing invoices for", "No completed payments found", "Custom Bill To (tax info, address, company)", "Leave empty to use the customer's saved billing info", "One line per row. When filled, this replaces the invoice's Bill To details.", "Download", "Preview", "Close", "Load", "Clear", "Loading...", "Cancel Subscription", "Are you sure you want to cancel your subscription?", "Loading subscription information...", "Plan Valid Until:", "Next Payment:", "Your subscription is canceled, but you can still use your minutes.").

## 10. Special-client alert

Already served: `GET /api/billing/overview` answers `special_client_alert: Option<SpecialClientAlert { code, percent, message }>` (CPN-019), present only for an account in the configured special-client audience with a configured offer. The title is `common` "Special Offer for Valued Clients!"; `message` is host data shown verbatim (DEF-023: the 25 versus 33 percent disagreement stays mu's host-data question, so it is not re-derived from `special_client_discount_message`); the code is shown with a copy action and `common` "Use this code at checkout to get your discount!". The source shows it on the home page and inside the purchase modal; dismissal is session-local UI state. Core carries it from its balance/overview fetch into `SignedInVm`; billing adds no new route.

## Source files

- `modules/billing/core/src/pricing.rs`: `display_amount`; `CouponLookup::Expired`; `CouponMessage` public with `Serialize`/`Deserialize`; `CouponField::notice`.
- `modules/billing/core/src/presentation.rs`: `pub mod invoice; pub mod purchase;`.
- `modules/billing/core/src/presentation/invoice.rs` (+ `invoice_tests.rs`): `TaxKind`, `BillTo`, `InvoiceFacts`, `InvoiceContext`, `bill_to`, `invoice_facts`, `invoice_context`.
- `modules/billing/core/src/presentation/purchase.rs` (+ `purchase_tests.rs`): `PurchaseState`, `PurchaseEvent`, `PurchaseEffect`, `PurchaseVm`, `CatalogView`, `QuoteView`, `OrderBody`, `CouponReply`, `CheckoutHandoff`, `CheckoutStarted` (`EVENT = "checkout_started"`, `properties()`), `PurchaseFailure`.
- `modules/billing/core/src/app.rs`: `quote_view`, `invoice_context`, `AdminLookup.invoice_context`; `CheckoutView` gains `Deserialize`, `PartialEq`.
- `modules/billing/http/src/lib.rs`, `routes.rs`: quote answers `QuoteView`; `GET /billing/invoices`; coupon body adds `notice`; the confirm page seam.
- `modules/billing/http/tests/api.rs`: `config()` adds `id` as a locale-discount locale; `app_with(page)`; new tests below.
- `modules/billing/core/Cargo.toml`: optional `ts-rs`, feature `ts`.
- `modules/billing/core/src/presentation/confirm.rs` (+ `confirm_tests.rs`), `invoices.rs` (+ `invoices_tests.rs`), `fixtures.rs`: sections 7 and 8.
- `modules/billing/core/src/presentation/purchase.rs`: the D17 limit constants; `billing_http` re-exports them.

## Tests (source only, not run)

```sh
cargo test -p billing-core pricing::tests::d8_the_display_amount_is_the_page_total_and_stays_separate_from_the_charged_quote
cargo test -p billing-core presentation::invoice
cargo test -p billing-core presentation::purchase
cargo test -p billing-core presentation
cargo check -p billing-core --features ts
BILLING_TEST_DATABASE_URL=... cargo test -p billing-http --test api -- --include-ignored
```

New billing-http tests: `d8_the_quote_answers_the_shown_total_beside_the_charged_amount`, `d7_the_owner_invoice_context_and_the_admin_lookup_carry_the_same_facts`, `d8_the_browser_confirmation_page_renders_the_single_confirmation_result`.

## Status

- Contract: published to p1V, p1W, p1X, p1T, p22 and the lead.
- Billing source: written. Feedback/support: landed (section 4). Confirm, invoices, fixtures, `ts` feature and portable D17 limits: written 2026-10-26, before S1. Nothing built, formatted or run.
