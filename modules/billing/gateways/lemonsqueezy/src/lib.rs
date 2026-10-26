//! LemonSqueezy adapter: a hosted pay-as-you-go checkout priced with
//! `custom_price`, and verification of signed order webhooks. No subscriptions,
//! no customer portal, no refunds, no license keys.
//!
//! Documentation-derived fixtures only; no real store or transaction was
//! available (see fixtures/PROVENANCE.md).

use std::time::Duration;

use billing_core::ports::{
    CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart, CheckoutStarted, GatewayError, Headers,
    OrderNotification, OrderWebhook, WebhookError, header,
};
use billing_core::types::BoxFuture;
use hmac::{Hmac, Mac};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug)]
pub struct LemonConfig {
    /// `https://api.lemonsqueezy.com` or a local protocol fixture.
    pub base_url: String,
    pub api_key: String,
    pub store_id: String,
    /// A one-off product variant. Its price is overridden per checkout.
    pub variant_id: String,
    pub signing_secret: String,
    /// `test` or `live`: matched against every order's `test_mode`.
    pub environment: String,
    pub timeout: Duration,
}

pub struct LemonClient {
    http: reqwest::Client,
    config: LemonConfig,
}

impl LemonClient {
    pub fn new(config: LemonConfig) -> Result<LemonClient, String> {
        if config.base_url.is_empty()
            || config.api_key.is_empty()
            || config.store_id.is_empty()
            || config.variant_id.is_empty()
            || config.signing_secret.is_empty()
        {
            return Err("LemonSqueezy needs a base URL, API key, store id, variant id and signing secret".into());
        }
        if !matches!(config.environment.as_str(), "test" | "live") {
            return Err("the LemonSqueezy environment is `test` or `live`".into());
        }
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| format!("building the LemonSqueezy HTTP client: {e}"))?;
        Ok(LemonClient { http, config })
    }

    /// The verifier for this deployment's store, variant and mode.
    pub fn webhook(&self) -> LemonWebhook {
        LemonWebhook { http: self.http.clone(), config: self.config.clone() }
    }
}

fn id_value(id: &str) -> Value {
    id.parse::<u64>().map(|n| json!(n)).unwrap_or_else(|_| json!(id))
}

fn create_body(config: &LemonConfig, request: &CheckoutStart, cents: i64) -> Value {
    json!({
        "data": {
            "type": "checkouts",
            "attributes": {
                // The application priced the order already: a positive integer of cents.
                "custom_price": cents,
                "product_options": {
                    "enabled_variants": [id_value(&config.variant_id)],
                    "redirect_url": request.callback_url,
                },
                // The provider must not apply a second discount to a priced order.
                "checkout_options": {"discount": false},
                "checkout_data": {
                    "email": request.buyer.email,
                    "custom": {
                        "payment_id": request.reference,
                        "account_id": request.buyer.account.to_string(),
                    },
                },
                "test_mode": config.environment == "test",
            },
            "relationships": {
                "store": {"data": {"type": "stores", "id": config.store_id}},
                "variant": {"data": {"type": "variants", "id": config.variant_id}},
            },
        }
    })
}

impl CheckoutGateway for LemonClient {
    fn gateway(&self) -> &str {
        "lemonsqueezy"
    }

    fn environment(&self) -> &str {
        &self.config.environment
    }

    fn start<'a>(&'a self, request: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutStarted, GatewayError>> {
        Box::pin(async move {
            if request.kind != CheckoutKind::PayAsYouGo {
                return Err(GatewayError::Rejected {
                    code: "kind".into(),
                    message: "LemonSqueezy sells pay-as-you-go orders only".into(),
                });
            }
            let cents = request
                .amount
                .to_minor_units()
                .filter(|c| *c > 0)
                .ok_or_else(|| GatewayError::Rejected { code: "amount".into(), message: "custom_price must be positive whole cents".into() })?;
            let body = serde_json::to_vec(&create_body(&self.config, request, cents))
                .map_err(|e| GatewayError::Uncertain(e.to_string()))?;
            let url = format!("{}/v1/checkouts", self.config.base_url.trim_end_matches('/'));
            let response = self
                .http
                .post(&url)
                .header(AUTHORIZATION, format!("Bearer {}", self.config.api_key))
                .header(ACCEPT, "application/vnd.api+json")
                .header(CONTENT_TYPE, "application/vnd.api+json")
                .body(body)
                .send()
                .await
                .map_err(|e| GatewayError::Uncertain(format!("LemonSqueezy request failed: {}", e.without_url())))?;
            let status = response.status();
            let bytes = response
                .bytes()
                .await
                .map_err(|e| GatewayError::Uncertain(format!("reading the LemonSqueezy answer failed: {}", e.without_url())))?;
            if status.is_server_error() {
                return Err(GatewayError::Uncertain(format!("LemonSqueezy answered HTTP {}", status.as_u16())));
            }
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
                GatewayError::Uncertain(format!("LemonSqueezy answered HTTP {} with a body that is not JSON", status.as_u16()))
            })?;
            if status.is_client_error() {
                let first = value.get("errors").and_then(|e| e.get(0));
                let field = |name: &str| first.and_then(|e| e.get(name)).and_then(Value::as_str).unwrap_or_default();
                return Err(GatewayError::Rejected {
                    code: status.as_u16().to_string(),
                    message: format!("{} {}", field("title"), field("detail")).trim().to_string(),
                });
            }
            let data = value.get("data").unwrap_or(&Value::Null);
            let id = match data.get("id") {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => return Err(GatewayError::Uncertain("LemonSqueezy answered without a checkout id".into())),
            };
            let url = data
                .get("attributes")
                .and_then(|a| a.get("url"))
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .ok_or_else(|| GatewayError::Uncertain("LemonSqueezy answered without a checkout url".into()))?;
            Ok(CheckoutStarted { token: id, form_content: None, url: Some(url.to_string()) })
        })
    }

    fn result<'a>(&'a self, _token: &'a str, _kind: CheckoutKind) -> BoxFuture<'a, Result<CheckoutResult, GatewayError>> {
        Box::pin(async move {
            Err(GatewayError::Uncertain("a LemonSqueezy order is confirmed by its signed webhook, not a lookup".into()))
        })
    }
}

/// Verifies `X-Signature` = hex HMAC-SHA256(signing secret, exact raw body)
/// in constant time, before any JSON is interpreted.
pub struct LemonWebhook {
    http: reqwest::Client,
    config: LemonConfig,
}

impl LemonWebhook {
    async fn item_quantity(
        &self,
        order: &OrderNotification,
        item: &Value,
    ) -> Result<i64, WebhookError> {
        let unreadable = |what: &str| WebhookError::Rejected(format!("unreadable order item: {what}"));
        let item_id = int(item, "id").filter(|id| *id > 0).ok_or_else(|| unreadable("id"))?;
        let price = int(item, "price").filter(|price| *price > 0).ok_or_else(|| unreadable("price"))?;
        if string(item, "order_id").as_deref() != Some(order.order_id.as_str()) {
            return Err(unreadable("order_id does not match the signed order"));
        }
        if order.store_id != self.config.store_id
            || order.variant_id != self.config.variant_id
            || order.test_mode != (self.config.environment == "test")
        {
            return Err(unreadable("store, variant or environment does not match this sale"));
        }
        // Only a numeric ID from the authenticated event determines the suffix.
        // Never follow links supplied in a webhook or a provider response.
        let url = format!("{}/v1/order-items/{item_id}", self.config.base_url.trim_end_matches('/'));
        let response = self.http
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.config.api_key))
            .header(ACCEPT, "application/vnd.api+json")
            .header(CONTENT_TYPE, "application/vnd.api+json")
            .send()
            .await
            .map_err(|e| WebhookError::Unavailable(format!("LemonSqueezy order-item lookup failed: {}", e.without_url())))?;
        if !response.status().is_success() {
            return Err(WebhookError::Unavailable(format!("LemonSqueezy order-item lookup HTTP {}", response.status().as_u16())));
        }
        let bytes = response.bytes().await
            .map_err(|e| WebhookError::Unavailable(format!("reading the LemonSqueezy order item failed: {}", e.without_url())))?;
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|_| WebhookError::Unavailable("LemonSqueezy order item is not JSON".into()))?;
        let unavailable = |what: &str| WebhookError::Unavailable(format!("unverified LemonSqueezy order item: {what}"));
        let data = json.get("data").ok_or_else(|| unavailable("data"))?;
        let attributes = data.get("attributes").ok_or_else(|| unavailable("attributes"))?;
        if data.get("type").and_then(Value::as_str) != Some("order-items")
            || int(data, "id") != Some(item_id)
            || string(attributes, "order_id").as_deref() != Some(order.order_id.as_str())
            || string(attributes, "variant_id").as_deref() != Some(order.variant_id.as_str())
            || int(attributes, "price") != Some(price)
        {
            return Err(unavailable("type, id, order, variant or price mismatch"));
        }
        let quantity = attributes.get("quantity").and_then(Value::as_i64)
            .filter(|quantity| *quantity > 0).ok_or_else(|| unavailable("quantity"))?;
        if price.checked_mul(quantity) != Some(order.subtotal_cents) {
            return Err(unavailable("item price and quantity do not match the order subtotal"));
        }
        Ok(quantity)
    }
}

/// The signature of a body, for fixtures and tests of other crates.
pub fn signature_hex(secret: &str, raw_body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(raw_body);
    hex::encode(mac.finalize().into_bytes())
}

fn signature_matches(secret: &str, claimed_hex: &str, raw_body: &[u8]) -> bool {
    let Ok(claimed) = hex::decode(claimed_hex.trim()) else { return false };
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(raw_body);
    mac.verify_slice(&claimed).is_ok()
}

fn int(value: &Value, key: &str) -> Option<i64> {
    match value.get(key)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn string(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl OrderWebhook for LemonWebhook {
    fn environment(&self) -> &str {
        &self.config.environment
    }

    fn expected_store(&self) -> &str {
        &self.config.store_id
    }

    fn expected_variant(&self) -> &str {
        &self.config.variant_id
    }

    fn verify<'a>(&'a self, headers: &'a Headers, body: &'a [u8]) -> BoxFuture<'a, Result<OrderNotification, WebhookError>> {
        Box::pin(async move {
            let claimed = header(headers, "x-signature").ok_or_else(|| WebhookError::Rejected("missing X-Signature".into()))?;
            if !signature_matches(&self.config.signing_secret, claimed, body) {
                return Err(WebhookError::Rejected("signature mismatch".into()));
            }
            let json: Value = serde_json::from_slice(body)
                .map_err(|_| WebhookError::Rejected("the notification body is not JSON".into()))?;
            let meta = json.get("meta").unwrap_or(&Value::Null);
            let event_name = string(meta, "event_name").ok_or_else(|| WebhookError::Rejected("no event name".into()))?;
            if event_name != "order_created" {
                return Err(WebhookError::Unsupported(format!("event {event_name}")));
            }
            let unreadable = |what: &str| WebhookError::Rejected(format!("unreadable order: {what}"));
            let data = json.get("data").unwrap_or(&Value::Null);
            if data.get("type").and_then(Value::as_str) != Some("orders") {
                return Err(unreadable("type"));
            }
            let order_id = string(data, "id").ok_or_else(|| unreadable("id"))?;
            let attributes = data.get("attributes").ok_or_else(|| unreadable("attributes"))?;
            let item = attributes.get("first_order_item").unwrap_or(&Value::Null);
            let custom = meta.get("custom_data").unwrap_or(&Value::Null);
            // Absence permits retrieval; a present invalid quantity must never be replaced.
            let quantity = match item.get("quantity") {
                None => None,
                Some(value) => Some(value.as_i64().filter(|q| *q > 0).ok_or_else(|| unreadable("quantity"))?),
            };
            let mut order = OrderNotification {
                event_name,
                order_id,
                status: string(attributes, "status").ok_or_else(|| unreadable("status"))?,
                store_id: string(attributes, "store_id").ok_or_else(|| unreadable("store_id"))?,
                variant_id: string(item, "variant_id").ok_or_else(|| unreadable("variant_id"))?,
                // Not exposed until a missing quantity is authenticated below.
                quantity: quantity.unwrap_or(0),
                currency: string(attributes, "currency").ok_or_else(|| unreadable("currency"))?,
                subtotal_cents: int(attributes, "subtotal").ok_or_else(|| unreadable("subtotal"))?,
                discount_cents: int(attributes, "discount_total").ok_or_else(|| unreadable("discount_total"))?,
                tax_cents: int(attributes, "tax").ok_or_else(|| unreadable("tax"))?,
                total_cents: int(attributes, "total").ok_or_else(|| unreadable("total"))?,
                tax_inclusive: attributes.get("tax_inclusive").and_then(Value::as_bool).ok_or_else(|| unreadable("tax_inclusive"))?,
                test_mode: attributes
                    .get("test_mode")
                    .and_then(Value::as_bool)
                    .or_else(|| meta.get("test_mode").and_then(Value::as_bool))
                    .ok_or_else(|| unreadable("test_mode"))?,
                payment_id: int(custom, "payment_id"),
                account_id: string(custom, "account_id"),
                receipt_url: attributes.get("urls").and_then(|u| string(u, "receipt")),
                raw: Value::Null,
            };
            if quantity.is_none() {
                order.quantity = self.item_quantity(&order, item).await?;
            }
            Ok(order)
        })
    }
}
