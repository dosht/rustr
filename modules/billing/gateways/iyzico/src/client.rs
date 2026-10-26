//! The Iyzico HTTP client: hosted checkout forms (pay-as-you-go and recurring
//! subscriptions) and subscription detail. Real requests, signed per request,
//! against a configurable base URL (sandbox, production or a local protocol
//! fixture). No provider account was available: the request and response
//! shapes come from the installed SDK, the public documentation and the
//! source repository's own usage, and are documented as such in the fixtures.

use std::time::Duration;

use billing_core::decimal::Decimal;
use billing_core::ports::{
    Buyer, CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart, CheckoutStarted, GatewayError,
    SubscriptionDetail, SubscriptionGateway,
};
use billing_core::types::BoxFuture;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Method;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::sign::authorization_v2;

#[derive(Clone, Debug)]
pub struct IyzicoConfig {
    /// `https://sandbox-api.iyzipay.com`, the production host, or a fixture server.
    pub base_url: String,
    pub api_key: String,
    pub secret_key: String,
    /// `sandbox` or `live`: part of every stored provider identity.
    pub environment: String,
    pub timeout: Duration,
}

pub struct IyzicoClient {
    http: reqwest::Client,
    config: IyzicoConfig,
}

/// A path segment is encoded so an identifier can never change the request
/// path. Unreserved characters stay as they are.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

pub fn segment(value: &str) -> String {
    utf8_percent_encode(value, SEGMENT).to_string()
}

const PAYG_INIT: &str = "/payment/iyzipos/checkoutform/initialize/auth/ecom";
const PAYG_RESULT: &str = "/payment/iyzipos/checkoutform/auth/ecom/detail";
const SUBSCRIPTION_INIT: &str = "/v2/subscription/checkoutform/initialize";

// Fixed placeholder identity fields, kept as the legacy checkout sends them (BIL-069, BIL-070).
const PLACEHOLDER_ADDRESS: &str = "Nidakule Göztepe, Merdivenköy Mah. Bora Sok. No:1";
const PLACEHOLDER_IDENTITY: &str = "11111111111";

impl IyzicoClient {
    pub fn new(config: IyzicoConfig) -> Result<IyzicoClient, String> {
        if config.api_key.is_empty() || config.secret_key.is_empty() || config.base_url.is_empty() {
            return Err("Iyzico needs a base URL, an API key and a secret key".into());
        }
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|e| format!("building the Iyzico HTTP client: {e}"))?;
        Ok(IyzicoClient { http, config })
    }

    /// Serializes once, signs those exact bytes and transmits the same bytes.
    async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value, GatewayError> {
        let payload = match body {
            Some(value) => serde_json::to_string(value).map_err(|e| GatewayError::Uncertain(e.to_string()))?,
            None => String::new(),
        };
        // A GET has no request model; the installed SDK signs `{}` for it.
        let signed = if body.is_some() { payload.as_str() } else { "{}" };
        let nonce = nonce();
        let authorization = authorization_v2(&self.config.api_key, &self.config.secret_key, &nonce, path, signed);
        let url = format!("{}{}", self.config.base_url.trim_end_matches('/'), path);
        let mut request = self
            .http
            .request(method, &url)
            .header(AUTHORIZATION, authorization)
            .header("x-iyzi-rnd", nonce)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json");
        if body.is_some() {
            request = request.body(payload);
        }
        let response = request
            .send()
            .await
            .map_err(|e| GatewayError::Uncertain(format!("Iyzico request failed: {}", e.without_url())))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| GatewayError::Uncertain(format!("reading the Iyzico answer failed: {}", e.without_url())))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            GatewayError::Uncertain(format!("Iyzico answered HTTP {} with a body that is not JSON", status.as_u16()))
        })?;
        if status.is_server_error() {
            return Err(GatewayError::Uncertain(format!("Iyzico answered HTTP {}", status.as_u16())));
        }
        match value.get("status").and_then(Value::as_str) {
            Some(s) if s.eq_ignore_ascii_case("success") => {
                if status.is_success() {
                    Ok(value)
                } else {
                    Err(GatewayError::Uncertain("Iyzico success envelope contradicted by HTTP status".into()))
                }
            }
            Some(_) => Err(GatewayError::Rejected {
                code: text(&value, "errorCode").unwrap_or_default(),
                message: text(&value, "errorMessage").unwrap_or_default(),
            }),
            None => Err(GatewayError::Uncertain("Iyzico answered without a status".into())),
        }
    }

    /// A rejected lookup is not an authoritative observation of a failed charge.
    /// Initialization still exposes `Rejected`; no decline-code list is inferred.
    async fn lookup(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value, GatewayError> {
        self.call(method, path, body).await.map_err(|error| match error {
            GatewayError::Rejected { code, message } => {
                GatewayError::Uncertain(format!("Iyzico lookup not confirmed ({code}): {message}"))
            }
            other => other,
        })
    }
}

fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn decimal(value: &Value, key: &str) -> Option<Decimal> {
    text(value, key).and_then(|t| Decimal::parse(&t).ok())
}

/// Match the SDK's wire representation without passing exact money through f64.
fn provider_price(amount: Decimal) -> String {
    let mut price = amount.to_string();
    if price.contains('.') {
        while price.ends_with('0') {
            price.pop();
        }
        if price.ends_with('.') {
            price.push('0');
        }
    } else {
        price.push_str(".0");
    }
    price
}

fn provider_locale(ui_locale: &str) -> &'static str {
    if ui_locale == "tr" { "tr" } else { "en" }
}

fn local_part(email: &str) -> &str {
    email.split('@').next().unwrap_or("")
}

fn names(buyer: &Buyer) -> (String, String, String) {
    let words: Vec<&str> = buyer.full_name.as_deref().unwrap_or("").split(' ').collect();
    let first = words.first().copied().filter(|w| !w.is_empty()).unwrap_or_else(|| local_part(&buyer.email));
    let second = words.get(1).copied().filter(|w| !w.is_empty()).unwrap_or_else(|| local_part(&buyer.email));
    let contact = buyer
        .full_name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| local_part(&buyer.email).to_string());
    (first.to_string(), second.to_string(), contact)
}

fn address(contact: &str, district: bool) -> Value {
    let mut value = json!({
        "contactName": contact,
        "city": "Istanbul",
        "country": "Turkey",
        "address": PLACEHOLDER_ADDRESS,
        "zipCode": "34742",
    });
    if district {
        value["district"] = json!("altunizade");
    }
    value
}

fn payg_body(request: &CheckoutStart) -> Value {
    let (name, surname, contact) = names(&request.buyer);
    let amount = provider_price(request.amount);
    json!({
        "price": amount,
        "paidPrice": amount,
        "currency": request.currency,
        "locale": provider_locale(&request.locale),
        "conversationId": request.reference,
        "basketId": "B67832",
        "paymentGroup": "PRODUCT",
        "callbackUrl": request.callback_url,
        "enabledInstallments": [2, 3, 6, 9],
        "buyer": {
            "id": request.buyer.account.to_string(),
            "name": name,
            "surname": surname,
            "email": request.buyer.email,
            "registrationAddress": PLACEHOLDER_ADDRESS,
            "city": "Istanbul",
            "country": "Turkey",
            "zipCode": "34732",
            "identityNumber": PLACEHOLDER_IDENTITY,
            "gsmNumber": "+905350000000",
        },
        "shippingAddress": address(&contact, false),
        "billingAddress": address(&contact, false),
        "basketItems": [{
            "id": request.product_name,
            "name": request.product_name,
            "category1": "Pay-as-you-Go",
            "itemType": "VIRTUAL",
            "price": amount,
        }],
    })
}

fn subscription_body(request: &CheckoutStart) -> Value {
    let (name, surname, contact) = names(&request.buyer);
    json!({
        "pricingPlanReferenceCode": request.pricing_plan_id,
        "subscriptionInitialStatus": "ACTIVE",
        "locale": provider_locale(&request.locale),
        "conversationId": request.reference,
        "callbackUrl": request.callback_url,
        "customer": {
            "name": name,
            "surname": surname,
            "email": request.buyer.email,
            "identityNumber": PLACEHOLDER_IDENTITY,
            "gsmNumber": "+905520000000",
            "billingAddress": address(&contact, true),
            "shippingAddress": address(&contact, true),
        },
    })
}

impl CheckoutGateway for IyzicoClient {
    fn gateway(&self) -> &str {
        "iyzico"
    }

    fn environment(&self) -> &str {
        &self.config.environment
    }

    fn start<'a>(&'a self, request: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutStarted, GatewayError>> {
        Box::pin(async move {
            let (path, body) = match request.kind {
                CheckoutKind::PayAsYouGo => (PAYG_INIT, payg_body(request)),
                CheckoutKind::Subscription => {
                    if request.pricing_plan_id.is_none() {
                        return Err(GatewayError::Rejected {
                            code: "plan".into(),
                            message: "a subscription checkout needs a plan code".into(),
                        });
                    }
                    (SUBSCRIPTION_INIT, subscription_body(request))
                }
            };
            let answer = self.call(Method::POST, path, Some(&body)).await?;
            // Subscription answers may nest the form under `data`.
            let source = answer.get("data").filter(|d| d.get("token").is_some()).unwrap_or(&answer);
            let token = text(source, "token")
                .ok_or_else(|| GatewayError::Uncertain("Iyzico answered success without a token".into()))?;
            Ok(CheckoutStarted {
                token,
                form_content: text(source, "checkoutFormContent"),
                url: text(source, "paymentPageUrl"),
            })
        })
    }

    fn result<'a>(&'a self, token: &'a str, kind: CheckoutKind) -> BoxFuture<'a, Result<CheckoutResult, GatewayError>> {
        Box::pin(async move {
            match kind {
                CheckoutKind::PayAsYouGo => {
                    // The result lookup is in English with the legacy fixed conversation id (BIL-071).
                    let body = json!({"locale": "en", "conversationId": "123456789", "token": token});
                    let answer = self.lookup(Method::POST, PAYG_RESULT, Some(&body)).await?;
                    if let Some(returned) = answer.get("token") && returned.as_str() != Some(token) {
                        return Err(GatewayError::Uncertain("Iyzico answered for another checkout token".into()));
                    }
                    let status = text(&answer, "paymentStatus")
                        .ok_or_else(|| GatewayError::Uncertain("Iyzico answered success without a paymentStatus".into()))?;
                    if status.eq_ignore_ascii_case("SUCCESS") {
                        Ok(CheckoutResult::Success {
                            customer_ref: None,
                            subscription_ref: None,
                            observed_amount: decimal(&answer, "paidPrice").or_else(|| decimal(&answer, "price")),
                            observed_currency: text(&answer, "currency"),
                            provider_payment_id: text(&answer, "paymentId"),
                        })
                    } else if status.eq_ignore_ascii_case("FAILURE") {
                        Ok(CheckoutResult::Failure { raw_status: status })
                    } else {
                        Err(GatewayError::Uncertain(format!("Iyzico payment status is not final: {status}")))
                    }
                }
                CheckoutKind::Subscription => {
                    let path = format!("/v2/subscription/checkoutform/{}", segment(token));
                    let answer = self.lookup(Method::GET, &path, None).await?;
                    let data = answer.get("data").unwrap_or(&Value::Null);
                    let subscription_ref = text(data, "referenceCode").ok_or_else(|| {
                        GatewayError::Uncertain("Iyzico answered success without a subscription reference".into())
                    })?;
                    Ok(CheckoutResult::Success {
                        customer_ref: text(data, "customerReferenceCode"),
                        subscription_ref: Some(subscription_ref),
                        observed_amount: None,
                        observed_currency: None,
                        provider_payment_id: None,
                    })
                }
            }
        })
    }
}

/// The subscription detail, parsed only from a uniquely matched shape: the
/// documented `data.items[]` with exactly one item for the requested
/// reference, or the source repository's `data` being that subscription.
/// Anything else, or both at once, cannot be verified.
pub fn parse_detail(answer: &Value, requested: &str) -> Result<SubscriptionDetail, GatewayError> {
    let data = answer.get("data").unwrap_or(&Value::Null);
    let items = data.get("items").and_then(Value::as_array);
    let direct = data.get("referenceCode").is_some();
    let subscription = match (items, direct) {
        (Some(items), false) if items.len() == 1 => &items[0],
        (None, true) => data,
        (Some(_), true) => {
            return Err(GatewayError::Uncertain("subscription detail has both documented and source shapes".into()));
        }
        (Some(items), false) => {
            return Err(GatewayError::Uncertain(format!("subscription detail lists {} subscriptions", items.len())));
        }
        (None, false) => return Err(GatewayError::Uncertain("subscription detail has no recognizable shape".into())),
    };
    if text(subscription, "referenceCode").as_deref() != Some(requested) {
        return Err(GatewayError::Uncertain("the provider answered for another subscription".into()));
    }
    let order_refs = match subscription.get("orders") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(orders)) => orders.iter().filter_map(|o| text(o, "referenceCode")).collect(),
        Some(_) => return Err(GatewayError::Uncertain("subscription orders are not a list".into())),
    };
    Ok(SubscriptionDetail {
        subscription_ref: requested.to_string(),
        customer_ref: text(subscription, "customerReferenceCode"),
        pricing_plan_ref: text(subscription, "pricingPlanReferenceCode"),
        status: text(subscription, "subscriptionStatus"),
        order_refs,
    })
}

impl SubscriptionGateway for IyzicoClient {
    fn environment(&self) -> &str {
        &self.config.environment
    }

    fn detail<'a>(&'a self, subscription_ref: &'a str) -> BoxFuture<'a, Result<SubscriptionDetail, GatewayError>> {
        Box::pin(async move {
            let path = format!("/v2/subscription/subscriptions/{}", segment(subscription_ref));
            let answer = self.lookup(Method::GET, &path, None).await?;
            parse_detail(&answer, subscription_ref)
        })
    }
}

impl billing_core::erasure::SubscriptionTermination for IyzicoClient {
    fn gateway(&self) -> &str {
        "iyzico"
    }

    fn environment(&self) -> &str {
        &self.config.environment
    }

    fn terminate<'a>(
        &'a self,
        target: &'a billing_core::erasure::TerminationTarget,
    ) -> BoxFuture<'a, Result<billing_core::erasure::TerminationEvidence, GatewayError>> {
        use billing_core::erasure::TerminationEvidence;
        Box::pin(async move {
            if target.gateway != "iyzico" || target.environment != self.config.environment || target.subscription_ref.is_empty() {
                return Err(GatewayError::Uncertain("erasure subscription gateway/environment binding mismatch".into()));
            }
            let path = format!("/v2/subscription/subscriptions/{}/cancel", segment(&target.subscription_ref));
            match self.call(Method::POST, &path, Some(&json!({}))).await {
                Ok(_) => Ok(TerminationEvidence::CancelSucceeded),
                // An explicit failure envelope may mean already terminated. Report authenticated
                // detail, not a guessed terminal status. Core owns customer binding and D10.
                Err(GatewayError::Rejected { .. }) => {
                    let detail = self.detail(&target.subscription_ref).await?;
                    Ok(TerminationEvidence::Detail {
                        subscription_ref: detail.subscription_ref,
                        customer_ref: detail.customer_ref,
                        status: detail.status,
                    })
                }
                // A lost/unreadable cancel response stays uncertain; no hidden repeat POST.
                Err(error) => Err(error),
            }
        })
    }
}
