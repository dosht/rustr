//! Ports the billing use cases need. Adapters live in other crates; none of
//! them is selected implicitly. A missing configuration is an error at host
//! startup, never a silent switch to a fake.

use std::fmt;

use serde_json::Value;
use uuid::Uuid;

use crate::decimal::Decimal;
use crate::erasure::{CheckoutClosure, ErasureCounts, ErasureStatus, TerminationTarget};
use crate::ledger::PackageBalance;
use crate::model::{
    BillingEvent, BillingInfo, NewPackage, NewPayment, NewSubscription, NewUnresolved, NewUsage,
    OutboxEvent, Payment, PaymentInsert, PaymentUpdate, Subscription, UnresolvedNotification,
};
use crate::grants::PaymentStatus;
use crate::pricing::Coupon;
use crate::types::{AccountId, BoxFuture, Micros, PackageId, PaymentId, SubjectRef};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The database could not be reached or a statement failed unexpectedly.
    Unavailable(String),
    AccountMissing(AccountId),
    /// The account has an erasure barrier (open or completed): no ordinary
    /// change, creation or new payable work may touch it (D9, D10).
    AccountErased(AccountId),
    /// A unique or ownership constraint refused the write.
    Conflict(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Unavailable(e) => write!(f, "storage unavailable: {e}"),
            StoreError::AccountMissing(a) => write!(f, "billing account {a} does not exist"),
            StoreError::AccountErased(a) => write!(f, "billing account {a} is unavailable due to account erasure"),
            StoreError::Conflict(e) => write!(f, "storage conflict: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

pub type StoreResult<'a, T> = BoxFuture<'a, Result<T, StoreError>>;

/// Everything billing does inside one database transaction.
///
/// The opener of the transaction commits or rolls back; none of these methods
/// does. The order is part of the contract: lock the account first, then host
/// aggregate rows in ascending id, then payment and subscription rows.
pub trait BillingTx: Send {
    fn create_account(&mut self, account: AccountId, created_at: Micros) -> StoreResult<'_, bool>;
    /// `SELECT ... FOR UPDATE` on the account row. Missing is an error.
    fn lock_account(&mut self, account: AccountId) -> StoreResult<'_, ()>;
    fn account_created_at(&mut self, account: AccountId) -> StoreResult<'_, Option<Micros>>;

    /// Every package of the account with the sum of all usage rows that reference it.
    fn packages(&mut self, account: AccountId) -> StoreResult<'_, Vec<PackageBalance>>;
    fn insert_package<'a>(&'a mut self, package: &'a NewPackage) -> StoreResult<'a, PackageId>;
    fn insert_usage<'a>(&'a mut self, rows: &'a [NewUsage]) -> StoreResult<'a, ()>;
    /// Hard delete of a subject: its usage rows go and the minutes return.
    fn release_subject(&mut self, account: AccountId, subject: SubjectRef) -> StoreResult<'_, u64>;
    /// The accounts whose packages an authorized subject's imported usage rows
    /// draw from, in ascending account id and always including `account`. An
    /// unlocked read: callers lock every returned account in this order before
    /// `release_subject`.
    fn imported_release_owners(&mut self, account: AccountId, subject: SubjectRef) -> StoreResult<'_, Vec<AccountId>>;

    fn insert_payment<'a>(&'a mut self, payment: &'a NewPayment) -> StoreResult<'a, PaymentInsert>;
    fn payment(&mut self, id: PaymentId, lock: bool) -> StoreResult<'_, Option<Payment>>;
    fn payment_by_token<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        token: &'a str,
        lock: bool,
    ) -> StoreResult<'a, Option<Payment>>;
    fn payment_by_order_ref<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        order_ref: &'a str,
        lock: bool,
    ) -> StoreResult<'a, Option<Payment>>;
    /// Compare-and-swap: true only when the row was in `from`.
    fn set_payment_status(
        &mut self,
        id: PaymentId,
        from: PaymentStatus,
        to: PaymentStatus,
    ) -> StoreResult<'_, bool>;
    fn update_payment<'a>(&'a mut self, id: PaymentId, update: &'a PaymentUpdate) -> StoreResult<'a, ()>;
    fn stamp_cancellation(&mut self, id: PaymentId, at: Micros) -> StoreResult<'_, ()>;
    /// Completed payments newest first.
    fn completed_payments(&mut self, account: AccountId) -> StoreResult<'_, Vec<Payment>>;

    /// Inserts the binding, or returns the existing one for the same provider identity.
    fn insert_subscription<'a>(&'a mut self, subscription: &'a NewSubscription) -> StoreResult<'a, Subscription>;
    fn subscription<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        subscription_ref: &'a str,
        lock: bool,
    ) -> StoreResult<'a, Option<Subscription>>;
    /// Sets `first_order_ref` once; false when it was already set.
    fn set_first_order_ref<'a>(
        &'a mut self,
        id: i64,
        order_ref: &'a str,
        admin: &'a str,
        rationale: &'a str,
    ) -> StoreResult<'a, bool>;
    fn stamp_subscription_cancel<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        subscription_ref: &'a str,
        at: Micros,
    ) -> StoreResult<'a, ()>;

    /// Durable storage for a verified notification that cannot be applied.
    /// The bool is false when the same notification was already held.
    fn hold_notification<'a>(
        &'a mut self,
        notification: &'a NewUnresolved,
    ) -> StoreResult<'a, (UnresolvedNotification, bool)>;
    fn unresolved(&mut self, id: i64, lock: bool) -> StoreResult<'_, Option<UnresolvedNotification>>;
    fn list_unresolved<'a>(&'a mut self, status: Option<&'a str>) -> StoreResult<'a, Vec<UnresolvedNotification>>;
    fn mark_unresolved<'a>(
        &'a mut self,
        id: i64,
        status: &'a str,
        payment_id: Option<PaymentId>,
        at: Micros,
    ) -> StoreResult<'a, ()>;

    fn coupon<'a>(&'a mut self, id: &'a str) -> StoreResult<'a, Option<Coupon>>;
    fn latest_billing_info(&mut self, account: AccountId) -> StoreResult<'_, Option<BillingInfo>>;

    /// Queues an event in the same transaction as the change that caused it.
    fn emit<'a>(&'a mut self, event: &'a BillingEvent, at: Micros) -> StoreResult<'a, ()>;

    // ---- account erasure (P3: decisions D5, D9, D10; see `crate::erasure`) ----

    /// `lock_account` for erasure itself. Ordinary `lock_account` refuses an
    /// account whose erasure has begun; only the erasure steps lock it here.
    fn lock_account_for_erasure(&mut self, account: AccountId) -> StoreResult<'_, ()>;
    /// Idempotent: the durable barrier, the account row as the lock anchor
    /// (also for an account billing never provisioned), and a pending
    /// termination for every bound subscription not yet recorded.
    fn open_erasure(&mut self, account: AccountId, at: Micros) -> StoreResult<'_, ()>;
    /// `None` when no erasure was opened for the account.
    fn erasure_status(&mut self, account: AccountId) -> StoreResult<'_, Option<ErasureStatus>>;
    /// Records one termination attempt for a target of `account`; `confirmed` is
    /// final. A binding of the account with no target yet is adopted on its first
    /// attempt; another account's target for the reference is a conflict.
    fn record_termination<'a>(
        &'a mut self,
        account: AccountId,
        target: &'a TerminationTarget,
        confirmed: bool,
        outcome: &'a str,
        at: Micros,
    ) -> StoreResult<'a, ()>;
    /// Erasure-only adoption of a subscription an authenticated provider result
    /// reported for a plan checkout of `account` (D10): a pending termination
    /// target, nothing granted or bound. Idempotent for the same account and
    /// customer, leaving an existing target's status and attempts as they are;
    /// `StoreError::Conflict` when the subscription is another account's target
    /// or this account's target names another customer. The caller holds the
    /// account's erasure lock.
    fn adopt_erasure_target<'a>(&'a mut self, account: AccountId, target: &'a TerminationTarget, at: Micros) -> StoreResult<'a, ()>;
    /// An operator's verified closure of an unresolved plan checkout of an
    /// account under erasure (D10). Writes it only if the payment is a plan
    /// checkout of `account` that no termination target of the same account or
    /// earlier closure covers; returns whether it was written. The caller holds
    /// the account's erasure lock and has validated the closure.
    fn resolve_erasure_checkout<'a>(
        &'a mut self,
        account: AccountId,
        payment_id: PaymentId,
        closure: &'a CheckoutClosure,
        at: Micros,
    ) -> StoreResult<'a, bool>;
    /// An unlocked read: the erased account plus every owner of a package that
    /// the account's imported usage or any of `subjects`' imported usage draws
    /// on, ascending, deduplicated.
    fn erasure_owners<'a>(&'a mut self, account: AccountId, subjects: &'a [SubjectRef]) -> StoreResult<'a, Vec<AccountId>>;
    /// The local cascade. Callers hold every `erasure_owners` lock and have
    /// checked that every subscription is stopped.
    fn erase_account_rows<'a>(
        &'a mut self,
        account: AccountId,
        subjects: &'a [SubjectRef],
        at: Micros,
    ) -> StoreResult<'a, ErasureCounts>;
}

/// A transaction billing opened itself.
pub trait UnitOfWork: BillingTx {
    fn tx(&mut self) -> &mut dyn BillingTx;
    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>>;
    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>>;
}

pub trait BillingStore: Send + Sync {
    fn begin(&self) -> StoreResult<'_, Box<dyn UnitOfWork>>;
    fn ping(&self) -> StoreResult<'_, ()>;

    /// Claims up to `limit` unpublished events in a short transaction and
    /// releases them before any external call. A claim is exclusive until
    /// `now + lease_seconds`.
    fn claim_outbox(&self, limit: i64, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<OutboxEvent>>;
    fn delivered_targets(&self, event_id: Uuid) -> StoreResult<'_, Vec<String>>;
    fn record_delivery<'a>(&'a self, event_id: Uuid, target: &'a str, at: Micros) -> StoreResult<'a, ()>;
    /// Marks published; false when the claim was lost.
    fn ack_outbox<'a>(&'a self, event: &'a OutboxEvent, at: Micros) -> StoreResult<'a, bool>;
    /// Gives the claim back with the failure, to be retried after `retry_at`.
    fn release_outbox<'a>(&'a self, event: &'a OutboxEvent, error: &'a str, retry_at: Micros) -> StoreResult<'a, ()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayError {
    /// No answer, or an answer that cannot be a provider verdict: the payment
    /// state is unknown and must not change.
    Uncertain(String),
    /// The provider answered with an explicit failure envelope.
    Rejected { code: String, message: String },
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GatewayError::Uncertain(e) => write!(f, "provider outcome unknown: {e}"),
            GatewayError::Rejected { code, message } => write!(f, "provider rejected the request: {code} {message}"),
        }
    }
}

impl std::error::Error for GatewayError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckoutKind {
    PayAsYouGo,
    Subscription,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Buyer {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
}

/// Everything a provider needs to start a hosted checkout.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckoutStart {
    /// Our payment id when it exists before the provider call (LemonSqueezy),
    /// else a fresh unique conversation id. Sent and stored as data only.
    pub reference: String,
    pub kind: CheckoutKind,
    pub amount: Decimal,
    pub currency: String,
    pub product_name: String,
    pub pricing_plan_id: Option<String>,
    pub locale: String,
    pub callback_url: String,
    pub buyer: Buyer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckoutStarted {
    pub token: String,
    /// Iyzico: the provider's embedded form content (untrusted HTML).
    pub form_content: Option<String>,
    /// LemonSqueezy: the hosted checkout URL.
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CheckoutResult {
    Success {
        customer_ref: Option<String>,
        /// Subscription checkouts only: the created subscription's reference.
        subscription_ref: Option<String>,
        observed_amount: Option<Decimal>,
        observed_currency: Option<String>,
        provider_payment_id: Option<String>,
    },
    Failure { raw_status: String },
}

pub trait CheckoutGateway: Send + Sync {
    fn gateway(&self) -> &str;
    fn environment(&self) -> &str;
    fn start<'a>(&'a self, request: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutStarted, GatewayError>>;
    fn result<'a>(&'a self, token: &'a str, kind: CheckoutKind) -> BoxFuture<'a, Result<CheckoutResult, GatewayError>>;
}

/// Lower-cased header names.
pub type Headers = Vec<(String, String)>;

pub fn header<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebhookError {
    /// Missing, malformed or wrong signature, or an unreadable body.
    Rejected(String),
    /// Authentic but not an event this module acts on: acknowledged, no effect.
    Unsupported(String),
    /// The signature was authentic but a provider lookup needed to complete the
    /// event could not be answered. Retryable: nothing is granted or held.
    Unavailable(String),
}

impl fmt::Display for WebhookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebhookError::Rejected(e) => write!(f, "webhook rejected: {e}"),
            WebhookError::Unsupported(e) => write!(f, "webhook event ignored: {e}"),
            WebhookError::Unavailable(e) => write!(f, "webhook verification unavailable: {e}"),
        }
    }
}

impl std::error::Error for WebhookError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionOutcome {
    Success,
    Failure,
}

/// A signature-verified Iyzico subscription order event. The signature covers
/// identifiers and the event type, never an amount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionNotification {
    pub outcome: SubscriptionOutcome,
    pub raw_event_type: String,
    pub order_ref: String,
    pub subscription_ref: String,
    pub customer_ref: String,
}

pub trait SubscriptionWebhook: Send + Sync {
    fn environment(&self) -> &str;
    fn verify(&self, headers: &Headers, body: &[u8]) -> Result<SubscriptionNotification, WebhookError>;
}

/// The provider's subscription detail, parsed only from a uniquely matched
/// documented or source-derived shape. Anything ambiguous is `Uncertain`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionDetail {
    pub subscription_ref: String,
    pub customer_ref: Option<String>,
    pub pricing_plan_ref: Option<String>,
    pub status: Option<String>,
    pub order_refs: Vec<String>,
}

pub trait SubscriptionGateway: Send + Sync {
    fn environment(&self) -> &str;
    fn detail<'a>(&'a self, subscription_ref: &'a str) -> BoxFuture<'a, Result<SubscriptionDetail, GatewayError>>;
}

/// A signature-verified LemonSqueezy order event.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderNotification {
    pub event_name: String,
    pub order_id: String,
    pub status: String,
    pub store_id: String,
    pub variant_id: String,
    pub quantity: i64,
    pub currency: String,
    pub subtotal_cents: i64,
    pub discount_cents: i64,
    pub tax_cents: i64,
    pub total_cents: i64,
    pub tax_inclusive: bool,
    pub test_mode: bool,
    /// `custom_data.payment_id`, correlation only.
    pub payment_id: Option<i64>,
    /// `custom_data.account_id`, correlation only.
    pub account_id: Option<String>,
    pub receipt_url: Option<String>,
    pub raw: Value,
}

pub trait OrderWebhook: Send + Sync {
    fn environment(&self) -> &str;
    /// The store and variant this deployment sells; compared to every order.
    fn expected_store(&self) -> &str;
    fn expected_variant(&self) -> &str;
    /// Verifies the signature over the raw bytes first, then completes the event
    /// from authenticated provider lookups when the signed payload omits a field.
    /// It never performs a lookup for an unsigned or invalid event.
    fn verify<'a>(&'a self, headers: &'a Headers, body: &'a [u8]) -> BoxFuture<'a, Result<OrderNotification, WebhookError>>;
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Customer {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    /// Trusted account creation instant (signup), the anchor of the free trial.
    pub created_at: Micros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryError(pub String);

impl fmt::Display for DirectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "customer directory: {}", self.0)
    }
}

impl std::error::Error for DirectoryError {}

/// Host-owned customer records: who an account is, and when it was created.
/// Recipient and eligibility data come from here, never from a request.
pub trait CustomerDirectory: Send + Sync {
    fn by_account(&self, account: AccountId) -> BoxFuture<'_, Result<Option<Customer>, DirectoryError>>;
    fn by_email<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<Option<Customer>, DirectoryError>>;
    /// Idempotent. An existing customer keeps its original `created_at`.
    fn register<'a>(&'a self, customer: &'a Customer) -> BoxFuture<'a, Result<Customer, DirectoryError>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailMessage {
    pub to: String,
    pub subject: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailError(pub String);

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mail delivery: {}", self.0)
    }
}

impl std::error::Error for MailError {}

pub trait Mailer: Send + Sync {
    /// `idempotency_key` is stable across retries of one logical message.
    fn send<'a>(&'a self, message: &'a MailMessage, idempotency_key: &'a str) -> BoxFuture<'a, Result<(), MailError>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalyticsError(pub String);

impl fmt::Display for AnalyticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AnalyticsError {}

/// The host's server-side analytics destination. `dedup_key` is stable across
/// retries of one logical event, so a destination that honors it counts it once.
pub trait AnalyticsSink: Send + Sync {
    fn send<'a>(&'a self, event: &'a crate::notify::AnalyticsEvent, dedup_key: &'a str) -> BoxFuture<'a, Result<(), AnalyticsError>>;
}
