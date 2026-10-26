//! Mail planning for committed events. Recipients come from the host's
//! customer directory and the host's configuration, never from the event's
//! caller. Sending is the relay's job; this only decides who gets what.

use serde_json::{Map, Number, Value, json};

use crate::decimal::Decimal;
use crate::model::{BillingEvent, Payment};
use crate::ports::{Customer, CustomerDirectory, DirectoryError, MailMessage};
use crate::types::Micros;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyConfig {
    pub brand: String,
    pub sales_email: String,
    pub support_email: String,
}

/// One message with the stable target name used for per-recipient delivery
/// tracking: a retry after a partial failure skips what was already sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub target: &'static str,
    pub message: MailMessage,
}

fn first_name(customer: &Customer) -> String {
    customer
        .full_name
        .as_deref()
        .and_then(|n| n.split_whitespace().next())
        .map(str::to_string)
        .unwrap_or_else(|| customer.email.split('@').next().unwrap_or("").to_string())
}

async fn customer_of(directory: &dyn CustomerDirectory, event: &BillingEvent) -> Result<Customer, DirectoryError> {
    let account = event
        .account()
        .ok_or_else(|| DirectoryError("event has no account".into()))?;
    directory
        .by_account(account)
        .await?
        .ok_or_else(|| DirectoryError(format!("no customer record for account {account}")))
}

pub async fn plan_deliveries(
    event: &BillingEvent,
    directory: &dyn CustomerDirectory,
    config: &NotifyConfig,
) -> Result<Vec<Delivery>, DirectoryError> {
    match event {
        BillingEvent::PaymentCompleted { payment_id, account, amount, currency, payment, .. } => {
            let customer = customer_of(directory, event).await?;
            let buyer = Delivery {
                target: "buyer",
                message: MailMessage {
                    to: customer.email.clone(),
                    subject: format!("Woohoo! Your {} purchase is complete!", config.brand),
                    text: format!(
                        "Hi {},\n\nThank you for your purchase. Your minutes are ready to use.\n\nThe {} team",
                        first_name(&customer),
                        config.brand
                    ),
                },
            };
            let sales = Delivery {
                target: "sales",
                message: MailMessage {
                    to: config.sales_email.clone(),
                    subject: "Payment Completed".into(),
                    text: format!(
                        "Payment id: {payment_id}\nAmount: {amount}\nCurrency: {currency}\nPayment: {payment}\nUser id: {account}\nEmail: {}",
                        customer.email
                    ),
                },
            };
            Ok(vec![buyer, sales])
        }
        BillingEvent::CancelRequested { payment_id, .. } => Ok(vec![Delivery {
            target: "support",
            message: MailMessage {
                to: config.support_email.clone(),
                subject: "!!!CANCEL SUBSCRIPTION!!!".into(),
                text: payment_id.to_string(),
            },
        }]),
        _ => Ok(vec![]),
    }
}

#[cfg(test)]
mod tests;

/// A server-side analytics event, sent once per outbox event. The wire identity is
/// derived from `dedup_name` (the source's `JSON.stringify([event, "payments", id])`),
/// so every retry and every process names the same event the same way.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalyticsEvent {
    pub name: &'static str,
    /// The account's UUID: the source's `distinctId`.
    pub distinct_id: String,
    /// The instant the transition committed. The source sends none for a signup.
    pub timestamp: Option<Micros>,
    pub dedup_name: String,
    pub properties: Map<String, Value>,
}

impl AnalyticsEvent {
    /// The per-event delivery mark: a retry skips an event already accepted.
    pub fn target(&self) -> String {
        format!("analytics:{}", self.name)
    }
}

const ATTRIBUTION_KEYS: [&str; 8] =
    ["utm_source", "utm_medium", "utm_campaign", "utm_content", "utm_term", "referrer", "device_type", "browser"];

fn dedup_name(event: &str, kind: &str, id: &str) -> String {
    json!([event, kind, id]).to_string()
}

fn origin(payment: &Payment) -> Option<&str> {
    payment.metadata.get("analytics").and_then(|a| a.get("payment_origin")).and_then(Value::as_str)
}

fn payment_properties(amount: &Decimal, currency: &str, payment_id: i64, payment: &Payment, failed: bool) -> Map<String, Value> {
    let mut properties = Map::new();
    if let Some(number) = amount.to_string().parse::<f64>().ok().and_then(Number::from_f64) {
        properties.insert("amount".into(), Value::Number(number));
    }
    properties.insert("currency".into(), json!(currency));
    properties.insert("payment_id".into(), json!(payment_id));
    properties.insert("payment_method".into(), json!("card"));
    properties.insert("is_trial".into(), json!(false));
    if let Some(name) = payment.package_name() {
        properties.insert("plan".into(), json!(name.to_lowercase()));
    }
    if payment.pricing_plan_id.is_some() {
        let yearly = match payment.metadata.get("package").and_then(|p| p.get("count")) {
            Some(Value::Number(n)) => n.as_u64() == Some(12),
            Some(Value::String(s)) => s == "12",
            _ => false,
        };
        properties.insert("billing_cycle".into(), json!(if yearly { "yearly" } else { "monthly" }));
    }
    if matches!(origin(payment), Some("initial" | "renewal")) {
        for key in ATTRIBUTION_KEYS {
            if let Some(value) = payment.metadata.get("analytics").and_then(|a| a.get(key)).and_then(Value::as_str) {
                properties.insert(key.into(), json!(value));
            }
        }
    }
    if failed {
        properties.insert("failure_reason".into(), json!("payment_failed"));
    }
    properties
}

fn payment_event(name: &'static str, payment_id: i64, account: crate::types::AccountId, at: Micros, properties: Map<String, Value>) -> AnalyticsEvent {
    AnalyticsEvent {
        name,
        distinct_id: account.to_string(),
        timestamp: Some(at),
        dedup_name: dedup_name(name, "payments", &payment_id.to_string()),
        properties,
    }
}

/// The server events the source captured (`capturePayment`, `captureSignup`), planned
/// from the committed outbox event so they leave once even when the browser never
/// returns. A missing or unreadable committed payment snapshot is an error to
/// retry, never a partially delivered event.
pub fn plan_analytics(event: &BillingEvent, occurred_at: Micros) -> Result<Vec<AnalyticsEvent>, String> {
    let read = |value: &Value| -> Result<Payment, String> {
        serde::Deserialize::deserialize(value).map_err(|e| format!("the stored payment is unreadable: {e}"))
    };
    match event {
        BillingEvent::PaymentCompleted { payment_id, account, amount, currency, payment, .. } => {
            let stored = read(payment)?;
            let properties = payment_properties(amount, currency, *payment_id, &stored, false);
            let subscription = stored.pricing_plan_id.is_some() && origin(&stored) == Some("initial");
            let mut events = Vec::with_capacity(if subscription { 2 } else { 1 });
            if subscription {
                events.push(payment_event("payment_completed", *payment_id, *account, occurred_at, properties.clone()));
                events.push(payment_event("subscription_started", *payment_id, *account, occurred_at, properties));
            } else {
                events.push(payment_event("payment_completed", *payment_id, *account, occurred_at, properties));
            }
            Ok(events)
        }
        BillingEvent::PaymentFailed { payment_id, account, amount, currency, payment, .. } => {
            let stored = read(payment)?;
            let properties = payment_properties(amount, currency, *payment_id, &stored, true);
            Ok(vec![payment_event("payment_failed", *payment_id, *account, occurred_at, properties)])
        }
        BillingEvent::FreemiumGranted { account, .. } => Ok(vec![AnalyticsEvent {
            name: "user_signed_up",
            distinct_id: account.to_string(),
            timestamp: None,
            dedup_name: dedup_name("user_signed_up", "users", &account.to_string()),
            properties: Map::new(),
        }]),
        _ => Ok(vec![]),
    }
}
