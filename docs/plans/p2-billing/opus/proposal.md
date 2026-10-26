# P2 billing: Opus independent proposal

Status: design proposal, written 2026-10-24 before reading the Astra proposal. Nothing here was compiled, run or verified against a provider. Evidence references are paths read during this design; claims about Transgate behavior come from `transgate/docs/discovery/billing.md` unless a source line is cited. Scratch findings: `notes.md` beside this file.

## 0. Main decisions in one screen

1. The billing rule set ports as-is, including its known defects, except DEF-001 (the PRD names the unlocked balance check). Every other outcome-changing defect stays current behavior, is pinned by a test, and is listed in section 9 with a recommended answer. Trust-boundary fixes that change nothing for a legitimate customer are made now and named (section 4.8).
2. One pure crate `billing-core` holds every rule, including the presentation rules (balance text, plan box, coupon messages, freemium transcript cut). Gateways, Postgres and HTTP are separate crates under `modules/billing/`. Transgate data (catalog, plan codes, freemium lists, special clients) lives in a new host crate `transgate/billing`, not in the module.
3. Atomicity uses a host-owned transaction: the billing Postgres adapter exposes a `BillingUnit` over a connection the caller already holds. Job start (P3) will run job status compare-and-swap and the charge in one transaction, which is how the trigger behaved (BIL-030). Per-account serialization is a row lock on `billing.accounts`, taken before any balance read, under READ COMMITTED. No gateway HTTP runs while a transaction is open.
4. Legacy money formulas are reproduced bit for bit in IEEE-754 `f64` (they were JavaScript `Number` arithmetic), then frozen into an exact decimal string at the boundary. Minutes are `i64`. Time is `i64` microseconds UTC, so Postgres instants round-trip without losing boundary or tie information.
5. P2 is proven by a separate, honestly named P2 billing gate. `gate-modules.sh` stays the final gate and keeps failing until CMS exists (section 7.5).

Unresolved risks the lead should see first: the rule ledger assembler rejects any status other than `ledger` (blocks every green row), Postgres tests currently skip and report `ok` (false green), DEF-004 (paid customer can get no packages) is preserved by the zero-change rule, and the data-model keep/throw list wrongly throws three live Iyzico columns (section 6.3).

## 1. Scope matrix

Owner column: `core` = `modules/billing/core` (`billing-core`), `pg` = `modules/billing/postgres` (`billing-postgres`), `http` = `modules/billing/http` (`billing-http`), `iyz` = `modules/billing/gateways/iyzico` (`billing-iyzico`), `ls` = `modules/billing/gateways/lemonsqueezy` (`billing-lemonsqueezy`), `tg` = `transgate/billing` (`transgate-billing`, Transgate data and host ports). Boundary says where the rule is enforced. Test strategy names the crate whose tests carry the ledger reference. Every rule row gets a Rust test in P2, including presentation rules, because they are pure functions in the core; the views that render them are P5 parity rows, listed separately.

### 1.1 BIL rows (79)

| Rows | Rule area | Owner | Boundary | Test strategy |
|---|---|---|---|---|
| BIL-001, BIL-002 | Package and usage row shape | core types, pg schema | `Package`, `UsageRow`; tables `billing.packages`, `billing.usage` | core unit (construction), pg integration (round trip, FK, `minutes >= 0` check) |
| BIL-003 to BIL-008 | Remaining minutes, balance at t, inclusive window, expiry, sum, missing balance | core | `balance(open, t)`, `is_open(p, t)`, `BalanceRow` absent vs `0` | core unit; pg integration repeats BIL-004/005/008 against stored rows at a fixed instant |
| BIL-009 | Negative balance only via bypass | core | `Minutes` is signed; only `ChargePlan::Bypass` may overdraw | core unit plus property test that no non-bypass plan overdraws |
| BIL-010 | Integer minutes, ceil per file, summed after rounding | core | `minutes_from_seconds`, `batch_minutes` | core unit (61 s gives 2, ten 6 s files give 10) |
| BIL-011, BIL-012 | Allocation order and formula | core | `allocate(request, open)` reproducing the SQL window with RANGE peers | core unit, including equal `end_at` peers (pins DEF-016 current outcome) |
| BIL-013 to BIL-018 | Six breakdown cases | core | `allocate` | one core unit test per case, exact carry values |
| BIL-019 | Users isolated | core, pg | allocation takes one account's packages; pg loads by `account_id` | core unit; pg integration with two accounts deducting concurrently |
| BIL-020 to BIL-022 | Deduct check, write, raise on zero rows | core | `plan_deduction(balance, allocation, check)` returning `Refusal::NotEnough(negative)` or `Refusal::NothingDeducted` | core unit; exact final balance 0 (not 0 to 5) |
| BIL-023 | Three overloads collapse to one | core | one entry `deduct(unit, DeductRequest{account, minutes, job, at, skip_check})` | core unit asserting the five-argument semantics, both modes |
| BIL-024 | Skip-check mode: full minutes on smallest `end_at`, raise if none | core | `ChargePlan::Bypass` targets smallest `end_at`, ties by smallest id (unspecified today) | core unit; pg integration for negative result |
| BIL-025 to BIL-033 | Start billing: transitions, once, whole duration, NULL deferral, paid path errors, rollback, sequence, two-package draw | core decision, pg atomicity, host transition | `charge_job_start(unit, JobStart{from_status, duration, other_completed_jobs})` | core unit for decisions; pg integration for BIL-030/031 rollback (job status row in a host test table keeps old status) and BIL-032/033 |
| BIL-034 to BIL-037 | Deferred charge from provider answer, length source, failure only logged, shorter or longer length | core (rules), P3 caller | `charge_deferred`, `provider_minutes_deepgram`, `provider_minutes_soniox`, `duration_after_provider` | core unit; P3 owns the callback wiring, billing tests the functions with the recorded provider metadata shapes |
| BIL-038 | Web start gated on non-null duration | core | `web_start_allowed(drafts)` | core unit |
| BIL-039, BIL-040 | Web pre-check `< 0`, API pre-check `<= 0` | core | `web_precheck(balance)`, `api_precheck(balance)` | core unit (0 passes web, fails API) |
| BIL-041 | Error mapping to 402 and the legacy 405 text | core codes, http mapping | `ChargeRefusal::code()`; http maps to `402 insufficient_balance` | core unit for classification; http test for status and body |
| BIL-042 | No refund; hard delete returns minutes | core, pg | `release_job_charges(unit, job)` deletes that job's usage rows; no other refund path | pg integration |
| BIL-043 to BIL-049 | Grant on pending to completed only; count, names, dates, interval error, examples | core | `expand_grant(payment_created, PackageSpec)`; `PaymentTransition::grants()` | core unit per example; pg integration for "failed to completed grants nothing" |
| BIL-050 to BIL-056 | Catalog entries, renewal grant, stored names | tg data, core types | `transgate_billing::catalog()` | tg unit per catalog row |
| BIL-057 to BIL-061 | Token idempotency, renewal outcome rules, confirm idempotency, outcome mapping, renewal copy | core state machine, pg constraint, iyz parsing | `PaymentStatus::transition(observed, verdict)` | core unit; pg integration for unique token race; iyz protocol test for verdict parsing |
| BIL-062, BIL-063 | Cancel stamp, next payment date | core, pg | `cancel_subscription`, `next_payment_date` | core unit; http test (owner check, section 4.8) |
| BIL-064, BIL-065 | Renewal text, current plan box | core presentation | `renewal_text(plan)`, `current_plan(open, now)` | core unit |
| BIL-066 | Purchase emails on pending to completed | core event, tg template | `BillingEvent::PaymentCompleted` written to `billing.outbox` | pg integration (event written once, only on that transition); tg template test with a recording mailer |
| BIL-067, BIL-068 | Row written only after gateway accepted; POST and session required | core flow, http | `start_checkout` calls gateway first, then inserts | core unit with fake gateway (failure writes nothing); http test |
| BIL-069 to BIL-071 | Exact Iyzico request and lookup fields | iyz | request builders | iyz protocol test against the local fixture server (exact JSON key order and values) |
| BIL-072 | Result page, countdown, analytics events | core events, core presentation | `ConfirmOutcome`, event names | core unit (events only on status change) |
| BIL-073, BIL-074 | Balance display, `newBalance` and `isFreemium` in answers | core presentation | `balance_text`, `StartAnswerMeta` | core unit |
| BIL-075, BIL-076 | History list, invoice id, admin lookup | core, http, tg policy | `invoice_id(payment_id)`, `AdminPolicy` port | core unit; http test (401, 403, 400, 404, 200) |
| BIL-077 | Fraud check off | core | no fraud gate exists | core unit: freemium user with a 100 KB file and 1-minute drafts is charged normally |
| BIL-078 | `canceled` never written by code; free-text gateway and currency | core, pg | enum contains `canceled`; no use case produces it; pg accepts it for migrated rows | core unit; pg migration fixture with one `canceled` row (production has 1) |
| BIL-079 | Resume removed; completed to not_started not billed | core | transition table | core unit |

### 1.2 FRE rows (25)

| Rows | Rule area | Owner | Boundary | Test strategy |
|---|---|---|---|---|
| FRE-001, FRE-002 | Freemium grant once per account, ordinary package | core rule, tg policy values, pg idempotency | `ensure_account(unit, account, FreemiumPolicy)` | core unit; pg integration (two concurrent first sign-ins grant once) |
| FRE-003 | DB definition of freemium-only (exact name `freemium`) | core | `freemium_only_at(open, now)` | core unit including `freemium-gift` counting as paid (production has that name) |
| FRE-004 to FRE-012 | Bypass, single row, completed-job eligibility, strict later jobs, exact fit, exhausted, overdrawn, totals, expired free package errors | core | `charge_job_start` | core unit per row; pg integration for FRE-006 two first jobs serialized both bypass |
| FRE-013, FRE-014 | App definitions 1 and 2 (kept distinct) | core | `freemium_app1(all_packages)`, `freemium_app2(result, now)` (error counts as freemium) | core unit |
| FRE-015, FRE-018, FRE-019 | 20/45 guard on batch total, first job language only, URL jobs skip guard | core algorithm, tg lists | `freemium_web_guard(total, ui_locale, first_language, first_target, lists)` | core unit with test lists; tg unit with the real lists |
| FRE-016, FRE-017 | Exception lists (48 locales, 37 languages) | tg | `transgate_billing::freemium_lists()` | tg unit: exact sets, `en` absent |
| FRE-020, FRE-021, FRE-023 | Display cuts (22:00 plus fade; 21:00 plus 3 lines and stop live), banner | core presentation | `transcript_cut_view`, `transcript_cut_loaded`, `freemium_banner` | core unit; views are P5 |
| FRE-022 | Downloads not blocked | core presentation | `download_allowed` is always true | core unit |
| FRE-024 | Chat limits apply only to freemium (app definition 2) | core predicate; AI topic owns limits | `freemium_app2` | core unit for the predicate; AI-015 to AI-019 stay AI rows |
| FRE-025 | Partial and resume removed | core | no split | core unit: 36-minute freemium first job charged 36 in one row |

### 1.3 CPN rows (19)

| Rows | Rule area | Owner | Boundary | Test strategy |
|---|---|---|---|---|
| CPN-001 | Currency from locale, USD and TRY only | core | `checkout_currency(locale)`; unknown currency is a checkout error | core unit |
| CPN-002, CPN-003 | TRY = USD x rate, no rounding; plan amount stored as USD x rate | core | `LegacyAmount` (f64) and `usd_try_rate(env)` | core unit with the 393.35999999999996 example; golden sweep (section 4.2) |
| CPN-004, CPN-005 | Payg price and hours | core | `payg_price(hours)`; server accepts any number (DEF-009 kept) | core unit |
| CPN-006 | Locale discount `id`, `pt` | core | `locale_discount(total, locale)` for the charged total and `locale_discount_shown(hour_price)` for the shown total | core unit (7.45 charged, 7.40 shown) |
| CPN-007 | Special offer buyers | core rule, tg settings | `special_offer_total(hours)`; buyer list from settings | core unit; tg config test that emails are not in source |
| CPN-008 to CPN-011 | Coupon shape, product filter, discount, expiry | core | `Coupon::applies_to`, `apply_coupon`, `is_expired` | core unit, `amount` read through float4 text (section 4.2) |
| CPN-012 | Coupon screen messages and fetch rules | core presentation | `CouponFieldState` machine | core unit |
| CPN-013 | Server re-reads coupon; mismatched coupon still saved | core | `price_order` returns `coupon_id` whenever the coupon exists and is unexpired | core unit |
| CPN-014 | No usage limits | core | none exist | core unit: same coupon applied twice by two accounts |
| CPN-015, CPN-016 | Displayed prices, struck prices, `Save %10` | core presentation, tg data | `plan_card_view` | core unit; tg catalog test |
| CPN-017 | Analytics purchase in USD | core | `analytics_usd(amount, currency, rate)` | core unit |
| CPN-018 | Invoice VAT rule | core presentation | `invoice_tax(amount, currency)` | core unit |
| CPN-019 | Special clients alert | core rule, tg data | `special_client_alert(account, list)`; list loaded as data | core unit with fixture ids |

### 1.4 Rows other topics own that billing must serve

| Row | Billing obligation | Owner of the row |
|---|---|---|
| JOB-006, JOB-007, JOB-009, TRX-033, TRX-034 | `charge_job_start`, `charge_deferred`, pre-checks and error codes are the only billing entry points the job use cases call | P3 jobs; billing supplies tested functions |
| JOB-008 | Contradicts BIL-025: JOB-008 says INSERT as `not_started` is billed. Trigger DDL is AFTER UPDATE only; billing pgTAP `040:93` and `060:169` agree with BIL-025. Billing implements BIL-025; JOB-008 text should be corrected by its owner | P3 |
| IDN-004 | Account provisioning calls `ensure_account` (freemium) | identity host; billing tests idempotency |
| IDN-032, FLG-003, FLG-007 | `AdminPolicy` port implemented by Transgate from the allow-list setting; billing history is always on | tg |
| PRV-007, PRV-060, PRV-D03 | `erase_account(unit, account)` deletes payments, packages and usage and nulls `billing_info.account_id` (today's cascade); refunds stay billing | privacy saga (P3); billing tests the function |
| DAT-003, DAT-004, DAT-005 | Migration preserves package, usage and job references | P9 migration; billing schema keeps ids |
| AI-F09, AI-F10 | Use `freemium_app2` | P4 |
| API-F26, OPS-P005, EML-P002, EML-P003 | Outbox events with stable ids replace the database webhook | platform email and analytics; billing writes the events |

### 1.5 Explicitly excluded legacy items (no runtime port, still tested where a row exists)

`process_payment(text)`, the Paraşüt flow and tables, `Invoices`, `generic_contacts`, `invoice_requests`, Stripe-shaped `products`, `prices`, `subscriptions`, analytics payment views with fixed rates, `FREEMIUM-COVERAGE-MAP.md`, `Infra/gcf-iyzico`, `NewBilling`, `InvoiceForm` (WEB-BIL-018, WEB-BIL-019 stay throw unless mu reverses), the debug callback email (EML-BIL-004), `PriceCountdown`, `fraud.ts` (BIL-077 is tested as "no gate"), the resume feature (BIL-079, FRE-025 tested as absent), the database webhook triggers (replaced by the outbox), the `usage_history` insert policy (DEF-002). `billing_info.parasut_contact_id` is carried as data.

### 1.6 Billing parity rows: who makes them green

| Rows | P2 delivers | Remaining owner |
|---|---|---|
| API-BIL-001 to API-BIL-005, API-BIL-007 to API-BIL-010, API-BIL-012 (balance and meta parts) | `billing-http` routes and tests | none for the HTTP contract; API-BIL-012 job endpoints are P3/P7 |
| API-BIL-006 | Replaced by `billing.outbox` (internal contract) | test in pg |
| API-BIL-011 | `freemium_app2` | P4 owns the endpoint |
| EML-BIL-001 to EML-BIL-003 | Event plus Rust template, recorded by a mailer fake | Resend delivery belongs to the platform email rows |
| WEB-BIL-001 to WEB-BIL-031, WEB-035 to WEB-038 | View models and fixtures from the core | P5 views and Playwright |
| WEB-071, WEB-072 | Same entitlement API | P8 |

## 2. Crates, types and ports

### 2.1 Graph

```
modules/billing/core            billing-core          no dependencies, no I/O; feature "testing" adds MemoryStore and FakeGateway
modules/billing/postgres        billing-postgres      billing-core, sqlx (same features as taskr-postgres), uuid, serde_json
modules/billing/http            billing-http          billing-core, axum, serde, serde_json
modules/billing/gateways/iyzico billing-iyzico        billing-core, reqwest (rustls), hmac, sha2, sha1, base64, serde_json
modules/billing/gateways/lemonsqueezy billing-lemonsqueezy  billing-core, reqwest, hmac, sha2, serde_json

transgate/billing               transgate-billing     billing-core, billing-postgres, billing-http, billing-iyzico, billing-lemonsqueezy
transgate/server                transgate-server      + transgate-billing (mounts routes, runs relay)

crates/taskr-core               unchanged dependency list (none); gains a metering port only
crates/taskr-postgres           + billing-core, billing-postgres (charge inside the task save transaction)
crates/taskr-server             + billing-core, billing-http, billing-lemonsqueezy
```

No module crate depends on `taskr-*` or `transgate/*`, so `check-prd.py` module graph rules hold. `billing-core` cannot reuse `taskr_core::shared::future::BoxFuture` (that would be a forbidden host edge), so it declares the same two-line alias. No shared kernel crate is introduced. Fixture servers are dev-dependencies of the gateway crates (`axum`, `tokio`), not production code.

The root `Cargo.toml` members gain `modules/billing/*`, `modules/billing/gateways/*` and `transgate/billing`. Lead owns that edit and the lockfile.

### 2.2 Core types (sketch, not final code)

```rust
pub struct AccountId(pub Uuid128);          // 16 bytes; Transgate user uuid, taskr workspace uuid
pub struct PackageId(pub i64);              // migrated serial ids kept
pub struct PaymentId(pub i64);
pub struct JobRef(pub i64);                 // host job id; billing never reads job tables
pub struct Micros(pub i64);                 // UTC instant, microseconds, same precision as timestamptz
pub struct Minutes(pub i64);                // signed: balances can be negative (BIL-009)

pub struct Package { id, account, payment: Option<PaymentId>, name: Option<String>,
                     minutes_included: i64, start_at: Micros, end_at: Micros }
pub struct OpenPackage { package: Package, consumed: i64 }      // remaining = included - consumed
pub struct UsageRow { account, package: PackageId, job: Option<JobRef>, minutes: i64, used_at: Micros }

pub enum Interval { Month, Year }
pub struct PackageSpec { name: String, minutes: i64, count: u32, interval: Interval, frequency: u32, expiry: u32 }

pub enum PaymentStatus { Pending, Completed, Failed, Canceled }
pub struct Payment { id, account, amount: AmountText, currency: String, gateway: String, status,
                     token: Option<String>, created: Micros, spec: Option<PackageSpec>,
                     analytics: JsonText, coupon_id: Option<String>, pricing_plan_id: Option<String>,
                     customer_id: Option<String>, subscription_canceled_at: Option<Micros> }

pub struct LegacyAmount(f64);               // only produced by the legacy formulas in pricing.rs
pub struct AmountText(String);              // exact decimal as stored in numeric; LegacyAmount::freeze()
```

`name` stays a free string: rule keys compare the exact text `freemium` (FRE-003), matching today including `freemium-gift`, `Master` and the null-named package seen in production (`transgate/docs/production-facts.md:31`). No `kind` column is introduced in P2 (DEF-026 is a decision, section 9).

### 2.3 Pure functions that carry the rules

```rust
pub fn is_open(p: &Package, t: Micros) -> bool;                                  // start <= t <= end
pub fn balance(open: &[OpenPackage]) -> Option<Minutes>;                         // None = no row (BIL-008)
pub fn allocate(request: i64, open: &[OpenPackage]) -> Vec<Breakdown>;           // SQL window, RANGE peers
pub fn plan_deduction(req: &DeductRequest, open: &[OpenPackage]) -> Result<Vec<UsageRow>, Refusal>;
pub fn decide_job_start(job: &JobStart, open: &[OpenPackage], now: Micros) -> Result<StartDecision, ChargeRefusal>;
pub fn decide_deferred(job: JobRef, minutes: i64, open: &[OpenPackage], now: Micros) -> Result<Vec<UsageRow>, Refusal>;
pub fn expand_grant(created: Micros, spec: &PackageSpec) -> Result<Vec<(Micros, Micros)>, InvalidInterval>;
pub fn add_months_utc(t: Micros, months: u32) -> Micros;                          // Postgres interval semantics, day clamped
pub fn transition(current: PaymentStatus, incoming: Outcome, source: OutcomeSource) -> Transition;
pub fn price_order(req: &OrderRequest, catalog: &Catalog, coupon: Option<&Coupon>, ctx: &PricingContext) -> Result<PricedOrder, OrderError>;
```

`StartDecision` is `Deferred` (NULL duration), `Strict(Vec<UsageRow>)` or `Bypass(UsageRow)`. `JobStart` carries `from_status`, `to_status`, `duration: Option<i64>` and `other_completed_jobs: u64`. The host supplies `other_completed_jobs` from its own tables inside the same transaction; billing never queries jobs.

### 2.4 Ports

```rust
/// One unit of work over one database transaction (or the memory store's lock).
/// The opener commits; billing functions never commit or roll back.
pub trait BillingUnit: Send {
    fn lock_account<'a>(&'a mut self, a: &'a AccountId) -> BoxFuture<'a, Result<AccountLock, StorageError>>;
    fn open_packages<'a>(&'a mut self, a: &'a AccountId, at: Micros) -> BoxFuture<'a, Result<Vec<OpenPackage>, StorageError>>;
    fn insert_usage<'a>(&'a mut self, rows: &'a [UsageRow]) -> BoxFuture<'a, Result<(), StorageError>>;
    fn insert_packages<'a>(&'a mut self, rows: &'a [NewPackage]) -> BoxFuture<'a, Result<(), StorageError>>;
    fn payment_for_update<'a>(&'a mut self, key: &'a PaymentKey) -> BoxFuture<'a, Result<Option<Payment>, StorageError>>;
    fn insert_payment<'a>(&'a mut self, p: &'a NewPayment) -> BoxFuture<'a, Result<PaymentInsert, StorageError>>; // Inserted(id) | Duplicate(id)
    fn set_payment_status<'a>(&'a mut self, id: PaymentId, expected: PaymentStatus, next: PaymentStatus, customer: Option<&'a str>) -> BoxFuture<'a, Result<bool, StorageError>>;
    fn emit<'a>(&'a mut self, e: &'a BillingEvent) -> BoxFuture<'a, Result<(), StorageError>>;
    // plus reads used by views: packages_of, completed_payments_of, coupon, billing_info, release_job, erase_account
}

pub trait BillingStore: Send + Sync {
    fn begin(&self) -> BoxFuture<'_, Result<Box<dyn BillingUnit + '_>, StorageError>>;
    fn commit<'a>(&'a self, unit: Box<dyn BillingUnit + 'a>) -> BoxFuture<'a, Result<(), StorageError>>;
}

pub trait PaymentGateway: Send + Sync {          // implemented by billing-iyzico and billing-lemonsqueezy
    fn start_checkout<'a>(&'a self, o: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutForm, GatewayError>>;
    fn checkout_result<'a>(&'a self, token: &'a str, kind: OrderKind) -> BoxFuture<'a, Result<GatewayVerdict, GatewayError>>;
}
pub trait Subscriptions: Send + Sync {           // Iyzico only (LemonSqueezy has no subscriptions in scope)
    fn subscription<'a>(&'a self, reference: &'a str) -> BoxFuture<'a, Result<SubscriptionInfo, GatewayError>>;
}
pub enum GatewayError { Transport(String), Rejected { code: String, message: String } }
pub enum GatewayVerdict { Success { customer_ref: Option<String> }, NotSuccess { raw_status: String } }

pub trait Clock: Send + Sync { fn now(&self) -> Micros; }
pub trait AdminPolicy: Send + Sync { fn may_lookup_customers(&self, email: &str) -> bool; }
```

The use cases are free async functions over `&mut dyn BillingUnit` (for flows inside a host transaction) plus a small `Billing` service struct holding `Arc<dyn BillingStore>`, gateways, catalog, policies and the clock (for flows billing opens itself: checkout, confirm, renewal, LemonSqueezy order, freemium provisioning, cancel, reads). Gateway-specific JSON never enters the core; the core sees `GatewayVerdict`, `SubscriptionInfo { customer_ref }` and `OrderPaid { payment, order_id, total_minor, currency }`.

### 2.5 How each consumer exercises behavior

Transgate (`transgate-billing` plus `transgate-server`): provides the catalog (`frontend/src/domain/packages.ts:21-67` copied as Rust data with both Iyzico plan codes), `FreemiumPolicy { name: "freemium", minutes: 20, months: 2 }`, the two exception lists, settings (rate, special-offer emails, admin allow-list, Iyzico and LemonSqueezy endpoints and keys), and mounts `billing-http` at the legacy paths. P2 smoke: provision a dev user (freemium 20), price and start a pay-as-you-go checkout through the Iyzico adapter against the local protocol server, confirm, observe balance 20 plus `hours x 60`, deliver a recorded renewal callback for a plan, observe 12 packages for a yearly plan, deliver a signed LemonSqueezy order, read history. The job-start functions are exercised by a Transgate integration test that runs a host table `transgate_test.jobs` compare-and-swap plus `charge_job_start` in one transaction; P3 replaces that test table with the real `Job` aggregate.

taskr: `taskr-core` gains a port in the tasks context so the rule "adding a task in a signed-in workspace costs one credit" lives in the core use case and not in a shell:

```rust
pub struct Charge { pub units: u32, pub reference: u64 }   // reference = the new task id
// TaskListRepository::save gains `charge: Option<Charge>`; SaveError gains `Refused(Refusal)`
```

`taskr-postgres` runs `billing_postgres::unit(&mut tx)` and `billing_core::deduct` inside the same transaction as the task insert, so an exhausted workspace adds nothing and a conflict charges nothing. Memory and file adapters serve the signed-out shells (CLI, TUI, mobile, local web), which are unmetered by design and never receive a `Charge` (the use case passes `None` for `WorkspaceId::local()`). Workspace creation calls `ensure_account` with a taskr policy (10 credits, 12 months, name `freemium`). A pack is bought through `billing-lemonsqueezy` (fixed variant, signed order webhook, recorded fixture). This is the PRD scenario 5 slice: one paid package, one freemium limit, one gateway webhook. It changes taskr server behavior on purpose (an 11th task in a fresh workspace answers 402), so existing taskr server tests that add more than 10 tasks need a funded fixture; that is in the slice, not a side effect.

## 3. Transactions and concurrency

### 3.1 Rules every flow follows

- Isolation is READ COMMITTED (the Postgres default), set explicitly by the adapter. The account lock statement runs before the read of open packages, so the read is a new statement snapshot that sees every ledger write committed by the previous lock holder. Under REPEATABLE READ this would be wrong; a test asserts the adapter's isolation.
- Per-account serialization: `select 1 from billing.accounts where account_id = $1 for update`. The row exists for every account (provisioning creates it; migration creates it for every user). A missing row is an error (`AccountMissing`), never a silent unlocked path. A row lock is chosen over an advisory lock because it is visible in `pg_locks` by table, needs no hash of a uuid and is released by the same commit or rollback as the writes.
- Lock order is fixed: host aggregate rows first (jobs, task lists), then the payment row, then the account row. Billing never locks a job or a payment after an account. This rules out deadlock between job start, deferred charge and payment grants.
- No gateway HTTP inside a transaction. Every flow is: read (short transaction or none), gateway call (no transaction), one write transaction that re-checks state with a compare-and-swap.
- The unit that opens a transaction commits it. Billing functions return `Err` and the opener rolls back; there is no partial commit inside billing.

### 3.2 Flows

| Flow | Transaction shape | Under concurrency |
|---|---|---|
| Job start (P3 host calls) | Host tx: lock job rows in id order, CAS `draft/ready/uploaded -> not_started`, compute `other_completed_jobs`, then `charge_job_start` per job in ascending job id (lock account, load open packages, decide, insert usage), commit. Any refusal rolls back every job in the batch (today one UPDATE statement covers the batch, so the batch is all or nothing) | Two simultaneous starts for one account: the second waits on the account row, then reads the first's committed rows and is refused if the balance no longer covers it. This is the DEF-001 fix and the only intended outcome change |
| Two first jobs (freemium) | Same | Both see zero other completed jobs (neither has completed), so both get the bypass, serially. This is today's captured rule FRE-006, and the lock only removes the read/write interleaving, not the eligibility. A lifetime marker would change eligibility (DEF-020, section 9) |
| Deferred charge (P3 provider callback) | Host tx: lock job, set duration, lock account, `decide_deferred`, insert usage, commit. On refusal: host commits the duration update without usage and logs (BIL-036 keeps delivery). To keep "duration set even if charge fails", the host uses a savepoint around the charge | Serialized with job starts by the account lock |
| Freemium provisioning | Billing tx: `insert into billing.accounts ... on conflict do nothing returning`; when a row was inserted, insert the freemium package; commit | Two first sign-ins: one insert wins, one grant. Repeated calls are no-ops |
| Checkout start (Iyzico) | No tx while calling Iyzico. After Iyzico returns token and form: one tx inserts the `pending` payment. Insert failure: the customer gets the error page, exactly like today (`frontend/src/dao/Payments.ts:14-19` throws before the form is returned at `checkout/index.tsx:247-250`) | Double submit creates two pending rows and two forms, as today |
| Confirm (Iyzico browser return) | Read payment by token (no lock). If `completed`: return success (BIL-059). Call Iyzico retrieve (no tx). One tx: `payment_for_update`, `set_payment_status(expected = observed, next)` as a CAS; if it changed and the transition is `pending -> completed`: lock account, `expand_grant`, insert packages, emit `PaymentCompleted`; if it changed to `failed`: emit `PaymentFailed`; commit | Refresh plus redirect: both may call Iyzico; only one CAS succeeds; the loser re-reads and returns the current state. Events are emitted once (fixes DEF-030 analytics duplication; packages were already safe) |
| Renewal callback (Iyzico server to server) | Retrieve subscription by body reference (no tx). One tx: insert renewal payment `pending` with `token = orderReferenceCode`, `on conflict (payment_gateway, token) do nothing`, then `payment_for_update` by token, apply BIL-058 (`neq completed`, `neq same status`), grant only for `pending -> completed` | Repeated callbacks: no change. Success then failed: failed is ignored. Failed then success: status becomes `completed` and, preserved as today, no packages (DEF-004, section 9). Two concurrent deliveries of the same order: the unique index arbitrates, the CAS under the row lock decides once |
| LemonSqueezy order | Checkout: tx inserts `pending` payment (new flow, so persist first is free), then create the checkout (no tx), then tx stores the checkout id in metadata; a definite creation failure sets `failed`. Webhook: verify signature on raw bytes, then tx: `payment_for_update` by our `custom_data.payment_id`; reject if gateway, account, total or currency differ from the stored order; CAS `pending -> completed` with `token = order id`; grant; commit | Repeated `order_created` for the same order: token already set, no-op. A second, different paid order for the same payment: conflicting payload, no grant, `PaymentConflict` event for support, answered 200 so LemonSqueezy stops retrying |
| Cancel subscription | One tx: load payment, require owner, stamp `subscription_canceled_at`, emit `CancelRequested` (support email EML-BIL-003). Iyzico is not called (BIL-062 preserved) | Repeated cancels re-stamp, as today |
| Hard delete of a job, account erase | Host tx calls `release_job_charges` or `erase_account` | Account lock taken, so no charge interleaves |

### 3.3 Outbox and recovery

`billing.outbox(id bigserial, account_id, name, occurred_at, payload jsonb, published_at)` is written in the same transaction as the change. A relay in the host server publishes in id order with `for update skip locked` so two replicas never send the same row twice concurrently; delivery is at least once and consumers key on the outbox id (the email consumer records sent ids). Events: `PaymentCompleted` (two emails, analytics `payment_completed`, `subscription_started` for an initial plan payment), `PaymentFailed`, `CancelRequested`, `PaymentConflict`, `FreemiumGranted`.

Failure points and what recovers them:

| Crash or error at | State left | Recovery |
|---|---|---|
| After Iyzico init, before payment insert | Token at Iyzico, no row, customer saw an error page | None needed; the form was never shown |
| After Iyzico said SUCCESS, before the confirm tx commits | Payment `pending` | The next confirm (refresh) or nothing, as today. A reconcile sweep is DEF-005, a decision |
| Iyzico retrieve transport error | Payment set `failed` (BIL-060 preserved) | Today's outcome; DEF-005 recommends leaving it `pending` |
| After commit, before relay publish | Event unpublished | Relay retries; emails may repeat only if the mailer crashes after sending and before recording |
| Renewal tx fails (database down) | Nothing written | Handler answers 500; Iyzico retries, the token index makes the retry idempotent (BIL-057) |

## 4. Fidelity details

### 4.1 Package windows, ties and time

- Open means `start_at <= t <= end_at`, both inclusive (BIL-005, pgTAP `020:92`). Consecutive yearly windows share the boundary instant and both count there (DEF-015 kept).
- Time is `Micros` (i64, UTC). Millisecond time would truncate migrated `timestamptz` values: an `end_at` and a `start_at` that differ by less than a millisecond would become equal, which changes both window inclusion and allocation ties. The core never reads a session time zone; the adapter binds `timestamptz` only and a test runs every pg test with `set time zone 'Asia/Istanbul'` to prove DEF-012 is gone.
- Allocation reproduces `request_breakdown_over_packages` exactly, including the SQL default RANGE frame: packages with equal `end_at` are peers and share one running total. Consequence pinned by tests (current behavior, DEF-016): two peers of 10 left each, request 5, each takes 0, nothing is written, `NothingDeducted` is raised for a user holding 20; request 15 writes 5 + 5 and undercharges by 5. The fix (order by `end_at, id` with a row frame) is a one-function change waiting for mu (section 9).
- Bypass target: smallest `end_at`; ties pick the smallest package id. Today's `limit 1` has no tie order, so any choice is consistent with current behavior; the choice is documented, not a rule change.
- "Now" inside one charge is one clock read per transaction, matching Postgres `now()` being the transaction start time.

### 4.2 Integer minutes and exact money

- Minutes: `i64` everywhere in the core, `integer` in storage (a 9999-hour pack is 599940 minutes). Job durations from the host are integers already rounded by BIL-010 rules.
- Zero-minute charges: a paid job of duration 0 writes no row and raises `NothingDeducted` (BIL-022); a freemium first job of 0 writes a 0-minute bypass row (BIL-024). So the new check constraint is `minutes >= 0`, not `> 0` as DEF-002 proposed; a `> 0` constraint would reject a legitimate current write.
- Money: today every amount is JavaScript `Number` arithmetic, and `iyzipay` sends `parseFloat(price).toString()` (`frontend/node_modules/iyzipay/lib/utils.js:40-49`). The faithful port computes the same IEEE-754 doubles in Rust (`f64` addition and multiplication are the same operations), with exact emulation of `Math.round` (round half toward positive infinity) and `Math.floor`, then freezes the result once into `AmountText` using the shortest round-trip decimal (the same digits JavaScript prints, so `393.35999999999996` stays `393.35999999999996`, `616` stays `616`). Storage is `numeric` with no scale, which keeps both the historical values and new ones exact. No arithmetic happens on `AmountText` after freezing; display helpers that today run `toFixed(2)` or `amount / 1.2` (CPN-018) parse to `f64` on purpose, documented as legacy display arithmetic.
- Evidence step during implementation (no network): generate a golden file with the real `iyzipay` and checkout formulas in local Node for hours 5 to 9999, locales `en`, `tr`, `id`, `pt`, coupon amounts {null, 0.1, 0.25, 0.33}, rate 44, special offer on and off; a Rust test compares every string. The command and Node version are recorded beside the golden file.
- Coupon `amount` is Postgres `real` and today reaches JavaScript through PostgREST as its shortest float4 text (`0.1`, not `0.10000000149011612`). The adapter selects `amount::text` and parses to `f64`, never `amount::float8`.
- Rate: `USD_TRY_RATE` parsed like `Number(x)`; zero, empty or not a number falls back to 44 (CPN-002). Production uses 44 (`transgate/docs/production-facts.md:13`).
- DEF-011 (round TRY to 2 decimals, store the gateway amount) is not applied; section 9.

### 4.3 Calendar month stepping

`add_months_utc` implements Postgres `timestamptz + interval 'N months'` evaluated in UTC (the production session zone that DEF-012 depends on): add months to the civil year and month, clamp the day to the month's last day, keep the time of day. `Interval::Year` adds 12 months (Feb 29 plus 1 year gives Feb 28, same as Postgres). `expand_grant` steps iteratively from the previous start (31 Jan, 28 Feb, 28 Mar, ...), each end is its own start plus `expiry` intervals (BIL-045, DEF-014 kept). Package 1 starts at `payments.created`, the checkout row insert time (DEF-013 kept); renewals start at the renewal row insert time (BIL-055). Freemium ends at provisioning time plus 2 months with the same function. Tests include 31 Jan, 29 Feb, 30 Apr and 23:59:59.999999 starts.

### 4.4 Negative balances

`Minutes` is signed. Only `StartDecision::Bypass` can produce a negative balance. Web pre-check refuses below 0, API pre-check at or below 0, the strict path refuses when balance minus duration is below 0, and a later freemium job after an overdraw is refused in both places (FRE-010). A property test generates random package sets and requests and asserts that no `Strict` or deferred plan leaves the balance below 0 while `Bypass` may.

### 4.5 First job: three separate rules, kept separate

| Check | Where it runs | Definition used | Source row |
|---|---|---|---|
| Freemium web guard (20 or 45 minutes on the batch total) | Before the job transaction, web submissions only | App definition 1 (exactly one package row ever, named `freemium`) | FRE-013, FRE-015, FRE-018 |
| Database eligibility for the bypass | Inside the job transaction, per job | DB definition (no open package not named `freemium`) and no other completed job | FRE-003, FRE-004, FRE-006 |
| Deferred deduction | Provider callback, strict, never bypass | Check on, `skip_check = false` | BIL-034, FRE-019 |

They disagree on purpose today (DEF-019, DEF-020). The proposal keeps three functions with three names; unifying them is a product decision.

### 4.6 Coupons and currency

Coupons apply only when `filter.product` equals the product name exactly; pay-as-you-go passes `Pay-as-you-Go`, plans pass none, so no coupon ever applies to a plan (CPN-009). The order is: special offer (TRY 14.99 per hour, no coupon, no locale discount, no conversion), otherwise locale discount on `hours x 1.49`, `round2`, coupon, then TRY conversion when the request currency is `TRY` (`checkout/index.tsx:288-313`). `coupon_id` is saved whenever the coupon exists and is unexpired, even when it did not apply (CPN-013). The TRY rule and the rate are Iyzico-only. LemonSqueezy charges USD; whether coupons and the `id`/`pt` discount apply there is a new-gateway decision (section 9); the P2 default is the same `price_order` result in USD, so a customer pays the same on either gateway.

### 4.7 Recurring Iyzico versus one-off LemonSqueezy

Iyzico: four plan entries map to real Iyzico subscriptions by plan code per currency; renewals arrive by webhook and grant through the same expansion. Pay-as-you-go is a one-off Iyzico checkout form. LemonSqueezy: one-off orders only (PRD section 3). Default P2 scope: pay-as-you-go through a custom-priced checkout, so 5 to 9999 hours work without a variant per hour count; plans are not sold through LemonSqueezy because that would require subscriptions or a yearly one-off product that does not exist today. Each paid order becomes a `payments` row with `payment_gateway = 'lemonsqueezy'`, `token = order id`, and `metadata.package` identical to the Iyzico pay-as-you-go spec, so `expand_grant` is shared.

### 4.8 Signature and authentication trust boundary

| Surface | Today | P2 | Outcome for a legitimate customer |
|---|---|---|---|
| Iyzico API calls | `IYZWSv2` HMAC-SHA256 over random key, path and `JSON.stringify(body)`, plus the `IYZWS` SHA1 fallback (`iyzipay/lib/utils.js:13-36`, `IyzipayResource.js:18-50`) | Same two headers; request JSON serialized in the request model's key order with undefined keys omitted (`requests/CreateCheckoutFormInitializeRequest.js:12-32`), because the signature covers that exact string | Unchanged |
| Confirm (browser POST with token) | No session; trust is the token plus a server-side Iyzico retrieve | Same | Unchanged |
| Renewal webhook | No signature; trusts the body, retrieves the subscription only to read the customer code (DEF-003) | Same retrieve, plus require the retrieved subscription's `customerReferenceCode` to equal the body's. A mismatch answers 400 and writes nothing | Unchanged for a real Iyzico callback; a forged body with a mismatched customer no longer grants. Full verification (signature header or order lookup) is a decision because its evidence is missing (section 9) |
| LemonSqueezy webhook | New | `X-Signature` = hex HMAC-SHA256 of the raw body with the signing secret, constant-time compare, before parsing | New flow |
| Cancel | Any caller can cancel any payment id (DEF-006) | Session required and the payment must belong to the caller; otherwise 404 | Unchanged for the owner. Calling Iyzico cancel is a decision |
| Ledger writes | Users may insert their own usage rows (DEF-002) | No user write path exists; only billing code writes, tables owned by the server role | Unchanged for every flow the product offers |
| Admin lookup | Session plus allow-list flag (BIL-076) | `AdminPolicy` from the same allow-list setting | Unchanged |

## 5. Gateway execution path and fixtures

### 5.1 Adapters are real HTTP clients

`billing-iyzico` is a `reqwest` client with a configurable base URL (`IYZIPAY_URI`), implementing exactly five calls, with paths taken from the SDK resources:

| Call | Method and path | Source |
|---|---|---|
| Pay-as-you-go form init | `POST /payment/iyzipos/checkoutform/initialize/auth/ecom` | `iyzipay/lib/resources/CheckoutFormInitialize.js:9` |
| Pay-as-you-go result | `POST /payment/iyzipos/checkoutform/auth/ecom/detail` | `resources/CheckoutForm.js:9` |
| Subscription form init | `POST /v2/subscription/checkoutform/initialize` | `resources/SubscriptionCheckoutForm.js:13` |
| Subscription form result | `GET /v2/subscription/checkoutform/{token}` | `resources/SubscriptionCheckoutForm.js:18` |
| Subscription retrieve (renewal) | `GET /v2/subscription/subscriptions/{reference}` | `resources/Subscription.js:39` |

Mapping rules copied from Transgate: envelope `status != "success"` on init is a checkout failure (`checkout/index.tsx:397,484`); payg result uses `paymentStatus`, plan result uses the envelope `status` and `data.customerReferenceCode` (`checkout/confirm.tsx:286-287,314`); `SUCCESS` compared case-insensitively. Transport errors are `GatewayError::Transport`; the core decides what that means (BIL-060).

`billing-lemonsqueezy` is a `reqwest` client for one call (create checkout, JSON:API, bearer key, store and variant relationships, `custom_price` in cents, `checkout_data.custom.payment_id`) plus a pure webhook verifier and parser for `order_created`. Its request and payload shapes come from the public LemonSqueezy API documentation; no Transgate source exists for it (billing.md keep/throw "LemonSqueezy | gap").

### 5.2 Fakes are test adapters only

`billing-core` feature `testing` provides `FakeGateway` (scripted verdicts) and `MemoryStore`. Production composition in `transgate-server` reads Iyzico and LemonSqueezy settings and fails to start with a named missing-setting error; there is no code path that substitutes a fake when settings are absent or wrong. taskr's server may run the LemonSqueezy adapter against the local fixture server in tests; its dev mode uses the fake only when `TASKR_BILLING_GATEWAY=fake` is set explicitly, and logs that it is a fake.

### 5.3 Local protocol fixture servers

Each gateway crate has a dev-dependency fixture server (axum on `127.0.0.1:0`) that behaves like the provider for the recorded exchanges:

- Iyzico: recomputes `IYZWSv2` and `IYZWS` from the fixture key, secret, `x-iyzi-rnd`, path and the received body bytes; rejects a wrong signature with Iyzico's error envelope; asserts method, path and exact JSON (key order included) against the recorded request; returns the recorded response. Tests cover success, `failure` envelope, `paymentStatus: FAILURE`, HTTP 500, connection refused and a slow response past the client timeout.
- LemonSqueezy: asserts bearer key, JSON:API headers and body for checkout creation and returns a recorded checkout; webhook tests post recorded `order_created` payloads signed by the fixture secret, plus tampered body, wrong secret, missing header, unpaid status, unknown payment id, mismatched total and replay.

Known-answer vectors pin the crypto independently of our own code: during implementation, compute one `generateHashV2` and one `generateHash` vector with the real `iyzipay` functions in local Node (fixed key, secret, random string, path, body), and one LemonSqueezy vector with `openssl dgst -sha256 -hmac`; commit the vectors with the exact command. No provider is contacted.

### 5.4 Fixture provenance

`modules/billing/gateways/<name>/fixtures/PROVENANCE.md` has one row per fixture: file, what it represents, source (repository path and line, SDK file, or documentation URL with retrieval date), what was synthesized (ids replaced by fixed fixture uuids, amounts chosen), and "no customer data". Iyzico sources available read-only: the SDK request models and resources above, the renewal callback samples in `frontend/src/pages/api/subscription-callback.ts:44-65`, the subscription form response sample in `checkout/confirm.tsx:257-268`, the sandbox form-content sample in `checkout/index.tsx:143`. Response bodies not present in any source are marked "synthesized from SDK response model fields" so nobody mistakes them for recordings. Iyzico sandbox and a LemonSqueezy test store are unavailable; the milestone must say the adapters are proved against local protocol fixtures only.

## 6. Storage and host integration

### 6.1 Schema and migrations

Postgres schema `billing` (the module's prefix), its own migration folder `modules/billing/postgres/migrations`, and its own migration history table inside that schema, so it never shares version numbers with `taskr-postgres` or a future Transgate migrator. The exact sqlx 0.9 mechanism (custom migrations table name or a dedicated migrator invocation with `search_path`) must be confirmed against the sqlx version in the lockfile during slice 2; if sqlx cannot do it, the fallback is to prefix version numbers per module and record the decision.

```
billing.accounts      (account_id uuid pk, created_at timestamptz)
billing.payments      (id bigint identity pk, account_id uuid, amount numeric not null, currency text not null,
                       payment_gateway text not null, status billing.payment_status not null, token text,
                       created timestamptz not null, metadata jsonb, coupon_id text, pricing_plan_id text,
                       customer_id text, subscription_canceled_at timestamptz, updated_at timestamptz)
  unique (payment_gateway, token) where token is not null and payment_gateway in ('iyzico', 'lemonsqueezy')
  index (account_id, status), index (customer_id)
billing.packages      (id bigint identity pk, account_id uuid not null, payment_id bigint references billing.payments,
                       payment_seq smallint, name text, minutes_included integer, created timestamptz,
                       start_at timestamptz, end_at timestamptz)
  unique (payment_id, payment_seq) where payment_id is not null; index (account_id, end_at)
billing.usage         (id bigint identity pk, account_id uuid not null, package_id bigint references billing.packages on delete cascade,
                       job_ref bigint, minutes integer not null check (minutes >= 0), used_at timestamptz not null)
  index (package_id), index (account_id), index (job_ref)
billing.coupons       (id text pk, created_at, amount real, discount_type text, expires_at timestamptz, filter jsonb)
billing.billing_info  (id bigint pk, account_id uuid null, created_at, address, company_name, country, full_name, tax_id, parasut_contact_id)
billing.special_clients (account_id uuid pk)
billing.outbox        (id bigserial pk, account_id uuid, name text, occurred_at timestamptz, payload jsonb, published_at timestamptz)
```

No triggers, no functions that decide. `payment_seq` makes a second grant for the same payment fail on a constraint even if a bug reached it. `job_ref` has no foreign key: jobs belong to the host. The `usage -> packages` cascade copies today's behavior; `usage.job_ref` deletes happen only through `release_job_charges`.

### 6.2 Migration-compatible ids and anomalies

Ids of payments, packages and usage are kept; identity sequences restart above the largest copied id (P9 tool). Historical anomalies are preserved as data and never re-validated by new rules: amounts like `393.35999999999996`, TRY plan amounts that differ from what Iyzico charged, gateway values `bank` and `Iyzico IBAN`, the one `canceled` payment, package names `Master`, `Pay-as-you-Go-Add`, `pay-as-you-go`, `freemium-gift` and a null name, users without a freemium package (98383 users vs 98359 freemium packages). New rules apply only to new writes. New cross-account inputs are never trusted: a usage row is written only by `insert_usage` with the locked account's package ids, and `open_packages` filters by `account_id` on both packages and usage, so a legacy usage row whose `user_id` differs from its package owner (DEF-002) cannot double count (today's `group by p.id, uh.user_id` would). The P9 dry run must list those rows and rows with `minutes <= 0` or null `package_id`; if any non-negative check fails on real data, the migration reports it instead of loosening the constraint silently.

### 6.3 Data-dependent assumptions (none block P2)

| Assumption | Why it matters | How P2 proceeds |
|---|---|---|
| No two open packages of one account share an `end_at` in production, or few | Ties trigger DEF-016 | Current behavior reproduced; P9 dry run lists them (billing.md Research) |
| No negative or zero usage rows except bypass zeros | `minutes >= 0` constraint | P9 reports violations |
| No duplicate `iyzico` tokens before 2026-10-22 | Unique index | P9 reports |
| Renewal body `customerReferenceCode` always equals the retrieved subscription's | New 400 check | Recorded samples agree; if production logs show otherwise, drop the check |
| `pricing_plan_id`, `customer_id`, `subscription_canceled_at` hold live data | The data-model keep/throw row throws them as "unused Stripe leftovers" | That row is wrong: renewals find the customer by `customer_id` (`frontend/src/dao/Payments.ts:50-56`), confirm branches on `pricing_plan_id` (`checkout/confirm.tsx:168`). Lead should correct `transgate/docs/discovery/data-model.md`; billing keeps all three |

### 6.4 Host integration

- Transgate: `transgate-server` builds one `PgPool`, runs the billing migrator, constructs `Billing` with the Iyzico and LemonSqueezy adapters, mounts `billing-http` at `/checkout`, `/checkout/confirm`, `/api/subscription-callback`, `/api/cancel-subscription`, `/api/admin/customer-invoices`, `/api/billing/...` (balance, packages, payments, coupon lookup), `/api/billing/lemonsqueezy/webhook`, and runs the outbox relay. Confirm answers a 303 to a result route the P5 web app renders; until P5 that route is unlinked and WEB-BIL-011 to WEB-BIL-014 stay red. The Identity port for P2 tests is the existing dev identity pattern.
- taskr: `taskr-server` mounts the LemonSqueezy webhook and a balance route; `taskr-postgres` calls billing inside the task save transaction.
- Both: the account row is created on first use through `ensure_account`, which is also where the freemium grant happens.

## 7. Verification

Domain rules get a failing test first during implementation (slice order in section 8 enforces it). Advisors ran nothing.

### 7.1 Matrix

| Proof | Concrete check | Crate |
|---|---|---|
| Six breakdown cases, exact | BIL-013 to BIL-018 exact take and carry per package | billing-core |
| Exact final balance | BIL-021 asserts 0 left, not 0 to 5 | billing-core, billing-postgres |
| Equal expiry peers | Request 5 and 15 against two peers of 10 (current outcome) | billing-core |
| Month stepping | Payment on 31 Jan 2023: 12 starts 31 Jan, 28 Feb, 28 Mar, ...; 29 Feb 2024 yearly; BIL-046 to BIL-048 dates | billing-core |
| Money golden sweep | Every Node-generated amount string equals the Rust string | billing-core |
| Simultaneous deductions | Two tasks block on a barrier, both call `charge_job_start` for 60 minutes against a 100 minute balance; exactly one commits; ledger sum 60; the loser gets `NotEnough`. Repeat 50 times. A second test proves the first holder is blocking (`pg_locks` shows a waiter on `billing.accounts`) so the test cannot pass by luck of timing | billing-postgres |
| Two first jobs | Freemium-only account, two concurrent first jobs of 36 minutes: both bypass, serialized, balance -52, two rows on the free package | billing-postgres |
| Rollback | Host test table job row CAS plus refused charge in one tx: job keeps `draft`, zero usage rows, zero outbox rows (BIL-030, BIL-031) | billing-postgres |
| Batch all or nothing | Three jobs, third unaffordable: none start, no rows | billing-postgres |
| Isolation guard | Adapter connection reports READ COMMITTED; the post-lock read sees a row committed after the reader's transaction began | billing-postgres |
| Time zone | Whole pg suite under `Asia/Istanbul` session zone gives the same windows | billing-postgres |
| Payment dedupe | Concurrent identical renewal deliveries: one payment row, one grant, one `PaymentCompleted` | billing-postgres |
| Out of order | success then failed: completed with packages; failed then success: completed without packages (DEF-004 pinned) | billing-postgres |
| Confirm race | Two confirms with a scripted gateway: one transition, one event, packages once | billing-postgres |
| Conflicting LemonSqueezy payload | Different order id for a completed payment: no grant, `PaymentConflict` emitted, 200 | billing-postgres, billing-http |
| Iyzico protocol | Signed requests accepted by the fixture server; exact JSON for BIL-069 to BIL-071; failure envelopes, 500, refused connection, timeout | billing-iyzico |
| LemonSqueezy protocol | Signature accept and every reject case | billing-lemonsqueezy |
| HTTP contract | Status codes and bodies for checkout, confirm, renewal, cancel (owner), admin lookup, balance, webhook | billing-http |
| Transgate data | Catalog rows BIL-050 to BIL-056, CPN-015/016 prices, freemium lists FRE-016/017 exact sets | transgate-billing |
| Transgate end to end | Real Postgres plus Iyzico fixture server: provision, payg checkout and confirm, yearly renewal (12 packages), LemonSqueezy order, history | transgate-billing integration test, then `transgate-server` HTTP smoke |
| taskr end to end | Fresh workspace adds 10 tasks, 11th answers 402 with `insufficient_credits`, signed LemonSqueezy order for a pack, 11th succeeds; conflicting save charges nothing | taskr-postgres, taskr-server |
| Boundary | `check-prd.py modules` graph: no billing crate reaches `taskr-*` or `transgate/*`; both consumers reach billing | script |

### 7.2 Ledger test references

- Test names start with the lowercase row id, then a short slug: `bil_013_breakdown_request_10`. A test may cover several rows (name it after the first, list all in a doc comment); a row may cite several tests separated by `; `.
- Unit tests live inside the module they test so libtest names are paths (`allocate::tests::bil_013_breakdown_request_10`); integration tests live in one file per crate area, with names unique across the package because libtest prints integration tests without the file name.
- References use the existing format `cargo:<package>:<libtest name>` (`scripts/check-prd.py:71`) and are written into `transgate/docs/discovery/billing.md` (the source of truth), then `rules.md` is regenerated with `scripts/assemble-discovery.py`. Status becomes `tested` only when the named test passes in the gate run.
- Blockers the lead owns before any row can turn green:
  1. `assemble-discovery.py` `lint_rules` rejects any status except `ledger` (`scripts/assemble-discovery.py:155-156`). It must accept `ledger`, `tested` and `changed`, or regeneration fails the moment one row is updated.
  2. Postgres-backed tests in this repo skip by returning early and report `ok` without a database (`crates/taskr-postgres/src/lib.rs:376-380`). `cargo_evidence` would count those as passed. Billing Postgres tests must be `#[ignore = "needs BILLING_TEST_DATABASE_URL"]` and the P2 gate must run them with `--include-ignored` and the URL set, failing when the URL is missing. A plain `cargo test --workspace` on a laptop stays green without a database, and no skipped test can count as evidence.
  3. A scoped ledger check is needed: `check-prd.py ledgers` covers all 914 obligations and will stay red for P3 to P10. P2 needs `ledgers --prefix BIL,FRE,CPN` plus an explicit list of P2 parity rows (section 1.6), still executing every named test.

### 7.3 Both-consumer smoke

A script (lead-owned, for example `scripts/smoke-billing.py`) starts Postgres from `deploy/dev/compose.yaml`, the Iyzico and LemonSqueezy fixture servers as small binaries (or the test harness), `transgate-server` and `taskr-server` with `TASKR_DEV_AUTH=1`, then drives the HTTP flows in 7.1 and checks the database rows. It exits non-zero on any mismatch.

### 7.4 What P2 does not claim

No provider sandbox or live call, no Resend delivery, no rendered billing screens, no migration of real data, no Swift or Android billing flows.

### 7.5 The CMS gate problem

Recommendation: keep `scripts/gate-modules.sh` as the final module gate, unchanged in meaning; it requires billing and CMS and stays red until P6. Add a P2 gate that the milestone names honestly:

```
scripts/gate-p2-billing.sh
  check 'Billing boundary and both consumers'  python3 -B scripts/check-prd.py modules --module billing
  check 'Billing tests incl. Postgres'         (cargo test -p each billing crate, ignored tests included, URL required)
  check 'Billing ledger rows'                  python3 -B scripts/check-prd.py ledgers --prefix BIL,FRE,CPN --parity <P2 list>
  check 'Consumers build and test'             cargo test -p transgate-billing -p transgate-server -p taskr-postgres -p taskr-server
  check 'Both-consumer smoke'                  python3 -B scripts/smoke-billing.py
```

`--module billing` reuses the same graph code and reports CMS as "not in this gate's scope", never as passing. The P2 milestone then states: P2 gate green; final module gate red only for `modules/cms (P6)`; Transgate gate red for the non-billing rows. No fake CMS crate, and the final gate's meaning does not move.

## 8. Implementation slices for one developer

Each slice ends with its tests passing and named ledger references added to `billing.md`. The developer owns the files listed; the lead owns root `Cargo.toml`, `Cargo.lock`, gate scripts, `check-prd.py`, `assemble-discovery.py`, `rules.md` regeneration, milestone and book.

| # | Slice | Files owned | Depends on | Done when |
|---|---|---|---|---|
| 0 | Lead prerequisites | lead: workspace members, assembler status values, ignored-test evidence, scoped ledger and module checks | none | Lead confirms; developer does not start slice 2 tests as evidence before this |
| 1 | Core ledger: time, calendar, packages, balance, allocation, deduction, job start and deferred decisions, grant expansion, freemium decisions, pre-checks | `modules/billing/core/src/{time,calendar,ledger,charge,grant,freemium}.rs`, `Cargo.toml` | 0 | BIL-001 to BIL-049, BIL-077 to BIL-079, FRE-001 to FRE-014, FRE-025 core tests written failing first, then green |
| 2 | Postgres unit of work, schema, migrator, account lock, outbox, concurrency and rollback proofs | `modules/billing/postgres/**` | 1 | 7.1 Postgres rows green with a real database |
| 3 | Pricing and coupons with the Node golden file, payment state machine, checkout, confirm, renewal, cancel use cases with `FakeGateway`; presentation helpers | `core/src/{pricing,coupon,payment,checkout,present}.rs`, `core/tests/golden/` | 1 | BIL-050 to BIL-076 core parts, FRE-015 to FRE-024, CPN-001 to CPN-019 |
| 4 | Iyzico adapter, fixture server, provenance, known-answer vectors | `modules/billing/gateways/iyzico/**` | 3 | Protocol tests green |
| 5 | LemonSqueezy adapter, verifier, fixture server, provenance | `modules/billing/gateways/lemonsqueezy/**` | 3 | Protocol tests green |
| 6 | HTTP contract and router | `modules/billing/http/**` | 2, 3 | Contract tests green with `MemoryStore` and with Postgres |
| 7 | Transgate consumer: catalog, policies, settings, templates, server mount, relay, end-to-end test | `transgate/billing/**`, `transgate/server/src/**` | 2, 4, 5, 6 | Transgate data rows and the end-to-end test green |
| 8 | taskr consumer: core port and use case, Postgres transaction charge, server mount, funded test fixtures | `crates/taskr-core/src/tasks/**`, `crates/taskr-core/src/testing.rs`, `crates/taskr-postgres/**`, `crates/taskr-server/**` | 2, 5, 6 | taskr rows in 7.1 green; existing taskr tests still green |
| 9 | Evidence pass | `transgate/docs/discovery/billing.md` Rust test column only | 1 to 8 | Every BIL, FRE, CPN row names passing tests; lead regenerates and runs the P2 gate |

Narrow contracts between slices: slice 2 implements `BillingUnit` and `BillingStore` exactly as section 2.4; slices 4 and 5 implement `PaymentGateway` and `Subscriptions` and never touch storage; slice 6 calls only `Billing` and free use-case functions; slices 7 and 8 never import `sqlx` types from billing except `billing_postgres::unit(&mut PgConnection)` and the migrator.

Avoid: a `minutes_remaining` read model, a generic plugin registry for gateways, a separate idempotency-key table, a subscription aggregate (DEF-006), a refund or adjustment row type (DEF-018), feature-flag plumbing. None is needed for current behavior.

## 9. Unresolved business decisions

Default in every row is current behavior, already pinned by a test, so implementation does not wait. Each changes an outcome, so only mu can approve it. Recommendation in brackets.

| # | Decision | Current (implemented) | Alternative | Evidence |
|---|---|---|---|---|
| 1 | DEF-004: grant packages on any first transition into `completed` (failed then completed) | No packages, no email | Grant once, keyed by `payment_seq` [approve: a paying customer gets nothing today] | `iyzicoRenewal.ts:43-47` allows failed to completed; trigger needs `pending` |
| 2 | DEF-005: transport error on confirm leaves `pending`, plus a reconcile sweep | Marked `failed` | Leave `pending`, sweep re-reads [approve together with 1] | `checkout/confirm.tsx:186-198` |
| 3 | DEF-016: equal `end_at` allocation | Undercharge or false refusal | Order by `end_at, id`, row frame [approve] | Window default RANGE frame |
| 4 | DEF-003: full renewal verification | Body trusted after subscription retrieve (plus the customer equality check proposed in 4.8) | Iyzico signature header or order lookup; needs the Iyzico signing documentation and secret [approve when the secret exists] | `subscription-callback.ts:70-79` |
| 5 | DEF-006: call Iyzico cancel | Support cancels by hand | Call `POST /v2/subscription/subscriptions/{ref}/cancel` (exists in the SDK, `Subscription.js:23`) and keep the email [mu] | |
| 6 | DEF-007: renewal copies the oldest payment | Oldest payment's plan and amount | Match by subscription reference, amount from the order [approve] | `dao/Payments.ts:50-56` |
| 7 | DEF-011: TRY rounding and stored amount | Unrounded double, USD x rate stored | Round half up to 2 decimals; store gateway amount and rate [approve] | `iyzipay/lib/utils.js:40-49` sends the raw double |
| 8 | DEF-009: server validates hours 5 to 9999 | Any number accepted | Validate [approve] | `checkout/index.tsx:288-295` |
| 9 | DEF-010: `id`/`pt` shown vs charged | Shows 7.40, charges 7.45 | One function for both [mu picks which] | CPN-006 |
| 10 | DEF-013, DEF-014, DEF-015: start time, month stepping, shared boundary | Checkout time, iterative, both open at the boundary | Confirmation time; `created + (i-1) months`; half-open [mu] | BIL-045, pgTAP `020:92` |
| 11 | DEF-017: unpaid deferred delivery | Logged only, transcript delivered | Hold minimum, record debt, or refuse [mu] | `deepgram.ts:380-386` |
| 12 | DEF-018: refunds | None; hard delete returns minutes | Adjustment rows [mu] | Terms page |
| 13 | DEF-019, DEF-020: one freemium definition, race-free first job | Three definitions; completed-job eligibility | Unified `is_freemium`; lifetime marker [mu] | FRE-003, FRE-013, FRE-014 |
| 14 | DEF-021: server-side transcript cut | Display-only cut | Enforce on server [keep current] | |
| 15 | DEF-022 to DEF-024, DEF-026, DEF-032: badge text, special client percent, coupon semantics, plan code column, analytics tier | As today | Per billing.md proposals [mu] | |
| 16 | DEF-008 second half: sweep old `pending` to `canceled` | Never closed | Sweep [not needed for P2] | 4354 pending rows in production |
| 17 | LemonSqueezy scope: which products, which customers see it, coupons and `id`/`pt` discount on it, receipt instead of PDF invoice | Not in Transgate | P2 default: pay-as-you-go only, custom price equal to the Iyzico USD price, same coupon rules, a `gateway` field on the checkout request, no UI choice built in P2 [mu picks the selection rule for P5] | billing.md open questions 2 to 4 |
| 18 | Hard delete of a job returning minutes (BIL-042) vs DM-18 "no cascade" | Cascade returns minutes | Append-only ledger [keep current until privacy decides] | data-model DM-18 |

Not decisions, but corrections the lead should route: the data-model keep/throw row for the three Iyzico columns (6.3), JOB-008 versus BIL-025 (1.4), and DEF-008's claim that a customer can pay without a payment row (the form is returned only after the insert succeeds, `checkout/index.tsx:247-250`, and the insert throws on error, `dao/Payments.ts:16-19`).
