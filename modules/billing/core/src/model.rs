//! Records the store returns and the events it queues. Plain data, no I/O.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::decimal::Decimal;
use crate::grants::{PackageSpec, PaymentStatus};
use crate::types::{AccountId, Micros, PackageId, PaymentId};

/// A payment row. `currency` and `gateway` are free text because historical
/// rows carry values (`bank`, `Iyzico IBAN`) that new commands never accept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Payment {
    pub id: PaymentId,
    pub account: AccountId,
    /// The quoted amount. Historical amounts are kept verbatim.
    pub amount: Decimal,
    pub currency: String,
    pub gateway: String,
    pub status: PaymentStatus,
    /// Iyzico: the checkout token, or the order reference of a renewal.
    /// LemonSqueezy: the order id once paid.
    pub token: Option<String>,
    /// The checkout row's creation time: the anchor of package windows.
    pub created: Micros,
    /// `live`, `sandbox` or `test`: part of every provider identity.
    pub environment: String,
    pub metadata: Value,
    pub coupon_id: Option<String>,
    pub pricing_plan_id: Option<String>,
    pub customer_id: Option<String>,
    pub subscription_ref: Option<String>,
    pub provider_order_ref: Option<String>,
    pub subscription_canceled_at: Option<Micros>,
    /// Verified provider amounts, separate from the quote above.
    pub observed: Option<Value>,
    pub receipt_url: Option<String>,
}

impl Payment {
    /// The `metadata.package` block.
    pub fn spec(&self) -> Option<PackageSpec> {
        self.metadata.get("package").and_then(|p| serde_json::from_value(p.clone()).ok())
    }

    pub fn package_name(&self) -> Option<String> {
        self.metadata.get("package").and_then(|p| p.get("name")).and_then(Value::as_str).map(str::to_string)
    }

    pub fn is_renewal(&self) -> bool {
        self.metadata
            .get("analytics")
            .and_then(|a| a.get("payment_origin"))
            .and_then(Value::as_str)
            == Some("renewal")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewPayment {
    pub account: AccountId,
    pub amount: Decimal,
    pub currency: String,
    pub gateway: String,
    pub environment: String,
    pub token: Option<String>,
    pub created: Micros,
    pub status: PaymentStatus,
    pub metadata: Value,
    pub coupon_id: Option<String>,
    pub pricing_plan_id: Option<String>,
    pub provider_order_ref: Option<String>,
    pub subscription_ref: Option<String>,
    pub customer_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaymentInsert {
    Inserted(Payment),
    /// A unique provider identity already exists: the existing row.
    Duplicate(Payment),
}

/// Fields a confirmed payment may gain. `None` leaves a column untouched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaymentUpdate {
    pub token: Option<String>,
    pub customer_id: Option<String>,
    pub subscription_ref: Option<String>,
    pub provider_order_ref: Option<String>,
    pub observed: Option<Value>,
    pub receipt_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewPackage {
    pub account: AccountId,
    pub payment_id: Option<PaymentId>,
    pub payment_seq: Option<i32>,
    pub name: String,
    pub minutes: i64,
    pub start_at: Micros,
    pub end_at: Micros,
    pub created: Micros,
    /// The one signup grant of an account; unique per account in storage.
    pub signup_grant: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewUsage {
    pub account: AccountId,
    pub package_id: PackageId,
    pub subject: Option<i64>,
    pub minutes: i64,
    pub used_at: Micros,
}

/// The trusted binding created by an initial subscription confirmation.
#[derive(Clone, Debug, PartialEq)]
pub struct Subscription {
    pub id: i64,
    pub account: AccountId,
    pub gateway: String,
    pub environment: String,
    pub subscription_ref: String,
    pub customer_ref: Option<String>,
    pub pricing_plan_id: Option<String>,
    pub currency: String,
    pub amount: Decimal,
    pub spec: PackageSpec,
    pub initial_payment_id: PaymentId,
    /// Set only by an explicit, verified admin identification (decision D4).
    pub first_order_ref: Option<String>,
    pub first_order_resolved_by: Option<String>,
    pub first_order_rationale: Option<String>,
    pub created_at: Micros,
    pub cancel_requested_at: Option<Micros>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewSubscription {
    pub account: AccountId,
    pub gateway: String,
    pub environment: String,
    pub subscription_ref: String,
    pub customer_ref: Option<String>,
    pub pricing_plan_id: Option<String>,
    pub currency: String,
    pub amount: Decimal,
    pub spec: PackageSpec,
    pub initial_payment_id: PaymentId,
    pub created_at: Micros,
}

/// A verified provider notification that cannot be applied automatically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnresolvedNotification {
    pub id: i64,
    pub gateway: String,
    pub environment: String,
    /// `success` or `failure`: distinct observations of one order stay distinct.
    pub event_type: String,
    pub subscription_ref: Option<String>,
    pub order_ref: Option<String>,
    pub customer_ref: Option<String>,
    pub payment_id: Option<PaymentId>,
    pub reason: String,
    pub payload: Value,
    pub received_at: Micros,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewUnresolved {
    pub gateway: String,
    pub environment: String,
    pub event_type: String,
    pub subscription_ref: Option<String>,
    pub order_ref: Option<String>,
    pub customer_ref: Option<String>,
    pub payment_id: Option<PaymentId>,
    pub reason: String,
    pub payload: Value,
    pub received_at: Micros,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BillingInfo {
    pub id: i64,
    pub account: Option<AccountId>,
    pub full_name: Option<String>,
    pub company_name: Option<String>,
    pub address: Option<String>,
    pub country: Option<String>,
    pub tax_id: Option<String>,
    pub parasut_contact_id: Option<String>,
    pub created_at: Micros,
}

/// Why a notification was held. Strings are stored and shown to admins.
pub mod reasons {
    pub const SUBSCRIPTION_UNBOUND: &str = "subscription_unbound";
    pub const INITIAL_ORDER_UNPROVEN: &str = "initial_order_unproven";
    pub const CUSTOMER_MISMATCH: &str = "customer_mismatch";
    pub const PAYMENT_UNKNOWN: &str = "payment_unknown";
    pub const ACCOUNT_MISMATCH: &str = "account_mismatch";
    pub const ENVIRONMENT_MISMATCH: &str = "environment_mismatch";
    pub const STORE_MISMATCH: &str = "store_mismatch";
    pub const VARIANT_MISMATCH: &str = "variant_mismatch";
    pub const CURRENCY_MISMATCH: &str = "currency_mismatch";
    pub const AMOUNT_MISMATCH: &str = "amount_mismatch";
    pub const AMOUNT_INVALID: &str = "amount_invalid";
    pub const QUANTITY_MISMATCH: &str = "quantity_mismatch";
    pub const DISCOUNT_PRESENT: &str = "discount_present";
    pub const CONFLICTING_ORDER: &str = "conflicting_order";
}

/// Events committed with the change that caused them. At-least-once delivery
/// with the stable outbox `event_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum BillingEvent {
    PaymentCompleted {
        payment_id: PaymentId,
        account: AccountId,
        amount: Decimal,
        currency: String,
        gateway: String,
        package_name: Option<String>,
        renewal: bool,
        /// The payment row as it was when it completed.
        payment: Value,
    },
    PaymentFailed {
        payment_id: PaymentId,
        account: AccountId,
        amount: Decimal,
        currency: String,
        gateway: String,
        /// The payment row as it was when it failed.
        payment: Value,
    },
    CancelRequested {
        payment_id: PaymentId,
        account: AccountId,
    },
    FreemiumGranted {
        account: AccountId,
        package_id: PackageId,
    },
    NotificationHeld {
        gateway: String,
        reason: String,
        order_ref: Option<String>,
        subscription_ref: Option<String>,
    },
    FirstOrderResolved {
        subscription_ref: String,
        order_ref: String,
        admin: String,
    },
}

impl BillingEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            BillingEvent::PaymentCompleted { .. } => "PaymentCompleted",
            BillingEvent::PaymentFailed { .. } => "PaymentFailed",
            BillingEvent::CancelRequested { .. } => "CancelRequested",
            BillingEvent::FreemiumGranted { .. } => "FreemiumGranted",
            BillingEvent::NotificationHeld { .. } => "NotificationHeld",
            BillingEvent::FirstOrderResolved { .. } => "FirstOrderResolved",
        }
    }

    pub fn account(&self) -> Option<AccountId> {
        match self {
            BillingEvent::PaymentCompleted { account, .. }
            | BillingEvent::PaymentFailed { account, .. }
            | BillingEvent::CancelRequested { account, .. }
            | BillingEvent::FreemiumGranted { account, .. } => Some(*account),
            _ => None,
        }
    }
}

/// A queued event as the relay sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct OutboxEvent {
    pub id: i64,
    /// Stable across retries: the idempotency key of every delivery.
    pub event_id: Uuid,
    /// Proves this consumer still owns the claim when it acknowledges.
    pub claim_token: Uuid,
    pub occurred_at: Micros,
    pub attempts: i32,
    pub event: BillingEvent,
}
