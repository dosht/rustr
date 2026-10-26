//! Fail-closed verification of Iyzico subscription order notifications.
//!
//! The signature header `X-IYZ-SIGNATURE-V3` is required; there is no fallback
//! that accepts a notification because its customer or subscription merely
//! exists. The merchant id and secret come from configuration, never from the
//! body. The signature covers identifiers and the event type, not an amount,
//! so nothing monetary is ever read from this payload.

use billing_core::ports::{
    Headers, SubscriptionNotification, SubscriptionOutcome, SubscriptionWebhook, WebhookError, header,
};
use serde_json::Value;

use crate::sign::subscription_v3_matches;

pub struct IyzicoWebhook {
    merchant_id: String,
    secret: String,
    environment: String,
}

impl IyzicoWebhook {
    pub fn new(merchant_id: &str, secret: &str, environment: &str) -> Result<IyzicoWebhook, String> {
        if merchant_id.is_empty() || secret.is_empty() {
            return Err("the Iyzico webhook verifier needs a merchant id and a secret".into());
        }
        Ok(IyzicoWebhook { merchant_id: merchant_id.into(), secret: secret.into(), environment: environment.into() })
    }
}

fn field(body: &Value, name: &str) -> Result<String, WebhookError> {
    body.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| WebhookError::Rejected(format!("the notification has no {name}")))
}

impl SubscriptionWebhook for IyzicoWebhook {
    fn environment(&self) -> &str {
        &self.environment
    }

    fn verify(&self, headers: &Headers, body: &[u8]) -> Result<SubscriptionNotification, WebhookError> {
        let claimed = header(headers, "x-iyz-signature-v3")
            .ok_or_else(|| WebhookError::Rejected("missing X-IYZ-SIGNATURE-V3".into()))?;
        let json: Value = serde_json::from_slice(body)
            .map_err(|_| WebhookError::Rejected("the notification body is not JSON".into()))?;
        let event_type = field(&json, "iyziEventType")?;
        let subscription_ref = field(&json, "subscriptionReferenceCode")?;
        let order_ref = field(&json, "orderReferenceCode")?;
        let customer_ref = field(&json, "customerReferenceCode")?;
        if !subscription_v3_matches(
            claimed,
            &self.secret,
            &self.merchant_id,
            &event_type,
            &subscription_ref,
            &order_ref,
            &customer_ref,
        ) {
            return Err(WebhookError::Rejected("signature mismatch".into()));
        }
        // The documented failure name and the source repository's name are both
        // accepted, explicitly. Every other signed type is acknowledged and ignored.
        let outcome = match event_type.as_str() {
            "subscription.order.success" => SubscriptionOutcome::Success,
            "subscription.order.failure" | "subscription.order.failed" => SubscriptionOutcome::Failure,
            other => return Err(WebhookError::Unsupported(format!("event type {other}"))),
        };
        Ok(SubscriptionNotification { outcome, raw_event_type: event_type, order_ref, subscription_ref, customer_ref })
    }
}
