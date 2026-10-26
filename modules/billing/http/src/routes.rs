//! Handlers. Order of checks on every protected route: authenticate (401),
//! authorize (403), validate the body (400), then act. Nothing here decides a
//! billing rule: handlers call `Billing` use cases and map their errors.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use billing_core::app::{BillingError, OrderRequest};
use billing_core::ports::Headers;
use billing_core::ports::Customer;
use billing_core::presentation::{CHECKOUT_START_FAILED, SIGN_IN_MESSAGE};
use billing_core::pricing::{CouponField, CouponLookup, payg_card, plan_card};
use billing_core::types::{AccountId, Micros};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

use crate::{
    AuthError, BillingHttp, CHECKOUT_FORM_MAX_BYTES, CHECKOUT_ORDER_MAX_BYTES, CHECKOUT_TOKEN_MAX_BYTES, Caller,
    CheckoutOutcome, ConfirmOutcome,
};

type State_ = State<Arc<BillingHttp>>;

pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> ApiError {
        ApiError { status, code: code.into(), message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": {"code": self.code, "message": self.message}}))).into_response()
    }
}

impl From<BillingError> for ApiError {
    fn from(error: BillingError) -> ApiError {
        let status = StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = match &error {
            // Storage details never reach a client.
            BillingError::Store(_) | BillingError::Directory(_) | BillingError::Internal(_) => {
                eprintln!("billing: {error}");
                "Billing storage is unavailable.".to_string()
            }
            BillingError::Gateway(_) => CHECKOUT_START_FAILED.to_string(),
            other => other.to_string(),
        };
        ApiError::new(status, error.code(), message)
    }
}

type Reply<T> = Result<Json<T>, ApiError>;

async fn authed(state: &BillingHttp, headers: &HeaderMap) -> Result<Caller, ApiError> {
    state.identity.authenticate(headers).await.map_err(|e| match e {
        AuthError::Unauthenticated(_) => ApiError::new(StatusCode::UNAUTHORIZED, "unauthenticated", SIGN_IN_MESSAGE),
        AuthError::AccountErased(_) => ApiError::new(StatusCode::GONE, "account_erased", "This account is unavailable due to account erasure."),
        AuthError::Unavailable(why) => {
            eprintln!("billing: identity unavailable: {why}");
            ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "identity_unavailable", "The sign-in service is not reachable.")
        }
    })
}

async fn admin(state: &BillingHttp, headers: &HeaderMap) -> Result<Caller, ApiError> {
    let caller = authed(state, headers).await?;
    if caller.is_admin {
        Ok(caller)
    } else {
        Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden", "This action needs an administrator."))
    }
}

fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", format!("invalid request body: {e}")))
}

fn header_pairs(headers: &HeaderMap) -> Headers {
    headers
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_ascii_lowercase(), value.to_str().ok()?.to_string())))
        .collect()
}

/// Account-scoped and admin routes, under `/billing`. Hosts nest or merge
/// this router where their own API lives (for example under `/api`).
pub fn authenticated(state: Arc<BillingHttp>) -> Router {
    Router::new()
        .route("/billing/ready", get(ready))
        .route("/billing/catalog", get(catalog))
        .route("/billing/overview", get(overview))
        .route("/billing/history", get(history))
        .route("/billing/invoices", get(invoices))
        .route("/billing/subscription", get(subscription))
        .route("/billing/payments/{id}/status", get(payment_status))
        .route("/billing/quote", post(quote))
        .route("/billing/coupon/{id}", get(coupon))
        .route("/billing/checkout", post(checkout))
        .route("/billing/cancel-subscription", post(cancel))
        .route("/billing/admin/customer-invoices", post(customer_invoices))
        .route("/billing/admin/payments/{id}/reconcile", post(reconcile))
        .route("/billing/admin/unresolved", get(list_unresolved))
        .route("/billing/admin/unresolved/{id}/replay", post(replay))
        .route("/billing/admin/subscriptions/first-order", post(first_order))
        .route("/billing/admin/erasure/checkouts/{id}/resolve", post(resolve_erasure_checkout))
        .route("/billing/internal/signup", post(signup))
        .with_state(state)
}

/// Routes the provider or the customer's browser calls without a session.
/// Mounted at these exact paths because they are configured at the providers.
pub fn provider(state: Arc<BillingHttp>) -> Router {
    Router::new()
        .route("/checkout", get(checkout_get).post(checkout_document))
        .route("/checkout/confirm", post(confirm))
        .route("/api/subscription-callback", post(subscription_callback))
        .route("/api/lemonsqueezy-webhook", post(order_webhook))
        .with_state(state)
}

// ---- public ----------------------------------------------------------------

async fn ready(State(state): State_) -> Response {
    let storage = state.billing.ready().await;
    let body = json!({
        "ready": storage.is_ok(),
        "configured": state.billing.configured(),
        "storage": if storage.is_ok() { "ok" } else { "unavailable" },
    });
    let status = if storage.is_ok() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(body)).into_response()
}

async fn catalog(State(state): State_) -> Json<Value> {
    let catalog = &state.billing.config().catalog;
    Json(json!({
        "plans": catalog.plans.iter().enumerate().map(|(index, p)| json!({"package_id": index, "card": plan_card(p)})).collect::<Vec<_>>(),
        "pay_as_you_go": payg_card(catalog),
        "min_hours": catalog.payg.as_ref().map(|p| p.min_hours),
        "max_hours": catalog.payg.as_ref().map(|p| p.max_hours),
    }))
}

// ---- account scoped ----------------------------------------------------------

async fn overview(State(state): State_, headers: HeaderMap) -> Reply<billing_core::app::Overview> {
    let caller = authed(&state, &headers).await?;
    Ok(Json(state.billing.overview(caller.account).await?))
}

/// Where a provider redirect that carries no token returns the buyer: the stored
/// status of the caller's own payment. Reading it never grants and never calls a provider.
async fn payment_status(State(state): State_, headers: HeaderMap, Path(id): Path<i64>) -> Reply<billing_core::app::PaymentStatusView> {
    let caller = authed(&state, &headers).await?;
    Ok(Json(state.billing.payment_status(caller.account, id).await?))
}

async fn history(State(state): State_, headers: HeaderMap) -> Reply<Value> {
    let caller = authed(&state, &headers).await?;
    let rows = state.billing.history(caller.account).await?;
    Ok(Json(json!({"payments": rows})))
}

/// What a client may say about an order. Account, amount and rights are
/// absent on purpose: unknown fields are ignored, never honored.
#[derive(Deserialize, Default)]
#[serde(default)]
struct OrderBody {
    package_id: Option<usize>,
    pay_as_you_go_hours: Option<i64>,
    currency: Option<String>,
    locale: Option<String>,
    coupon_id: Option<String>,
    gateway: Option<String>,
    analytics: Option<Value>,
}

impl OrderBody {
    fn into_request(self) -> OrderRequest {
        OrderRequest {
            package_id: self.package_id,
            pay_as_you_go_hours: self.pay_as_you_go_hours,
            currency: self.currency,
            ui_locale: self.locale.unwrap_or_else(|| "en".into()),
            coupon_id: self.coupon_id,
            gateway: self.gateway.unwrap_or_else(|| "iyzico".into()),
            analytics: self.analytics.unwrap_or(Value::Null),
        }
    }
}

/// The charged quote plus `display_amount`, the total the summary shows (P5 D8).
async fn quote(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::presentation::purchase::QuoteView> {
    let caller = authed(&state, &headers).await?;
    let request = parse::<OrderBody>(&body)?.into_request();
    Ok(Json(state.billing.quote_view(caller.account, &request).await?))
}

/// The Subscription tab's facts, or `null` without an open plan (WEB-BIL-022/023).
async fn subscription(State(state): State_, headers: HeaderMap) -> Reply<Option<billing_core::presentation::subscription::SubscriptionFacts>> {
    let caller = authed(&state, &headers).await?;
    Ok(Json(state.billing.subscription(caller.account).await?))
}

/// The caller's own invoice context: Bill To identity and invoice facts (P5 D7).
async fn invoices(State(state): State_, headers: HeaderMap) -> Reply<billing_core::presentation::invoice::InvoiceContext> {
    let caller = authed(&state, &headers).await?;
    Ok(Json(state.billing.invoice_context(caller.account).await?))
}

async fn coupon(State(state): State_, headers: HeaderMap, Path(id): Path<String>) -> Reply<Value> {
    authed(&state, &headers).await?;
    let now = state.billing.now();
    let found = state.billing.coupon_status(&id).await?;
    let mut field = CouponField::default();
    field.submit(&id);
    field.lookup_result(found.clone().map(CouponLookup::Found).unwrap_or(CouponLookup::NotFound), now);
    Ok(Json(json!({
        "valid": field.applied().is_some(),
        "message": field.message(),
        // The same outcome as a code, for localized shells (CouponReply::lookup).
        "notice": field.notice(),
        "coupon": field.applied().map(|c| json!({"id": c.id, "amount": c.amount, "product": c.filter_product})),
    })))
}

async fn checkout(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::CheckoutView> {
    let caller = authed(&state, &headers).await?;
    let request = parse::<OrderBody>(&body)?.into_request();
    Ok(Json(state.billing.start_checkout(caller.account, &request).await?))
}

#[derive(Deserialize)]
struct CancelBody {
    payment_id: i64,
}

async fn cancel(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::CancelView> {
    let caller = authed(&state, &headers).await?;
    let body: CancelBody = parse(&body)?;
    Ok(Json(state.billing.cancel_subscription(caller.account, body.payment_id).await?))
}

// ---- admin -------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct EmailBody {
    email: String,
}

async fn customer_invoices(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::AdminLookup> {
    admin(&state, &headers).await?;
    let body: EmailBody = parse(&body)?;
    if body.email.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "An email is required."));
    }
    Ok(Json(state.billing.admin_customer_invoices(body.email.trim()).await?))
}

async fn reconcile(State(state): State_, headers: HeaderMap, Path(id): Path<i64>) -> Reply<billing_core::app::ConfirmView> {
    admin(&state, &headers).await?;
    Ok(Json(state.billing.reconcile_payment(id).await?))
}

async fn list_unresolved(State(state): State_, headers: HeaderMap) -> Reply<Value> {
    admin(&state, &headers).await?;
    let rows = state.billing.list_unresolved(Some("held")).await?;
    Ok(Json(json!({"held": rows})))
}

async fn replay(State(state): State_, headers: HeaderMap, Path(id): Path<i64>) -> Reply<billing_core::app::NotificationOutcome> {
    admin(&state, &headers).await?;
    Ok(Json(state.billing.replay_unresolved(id).await?))
}

#[derive(Deserialize)]
struct FirstOrderBody {
    subscription_ref: String,
    order_ref: String,
    rationale: String,
}

async fn first_order(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::ResolveView> {
    let caller = admin(&state, &headers).await?;
    let body: FirstOrderBody = parse(&body)?;
    Ok(Json(
        state
            .billing
            .resolve_first_order(&caller.email, &body.subscription_ref, &body.order_ref, &body.rationale)
            .await?,
    ))
}

/// Missing attestations read as not verified, so a bare rationale is refused.
#[derive(Deserialize)]
struct ClosureBody {
    #[serde(default)]
    checkout_cannot_complete: bool,
    #[serde(default)]
    no_renewing_subscription: bool,
    #[serde(default)]
    evidence_ref: String,
    #[serde(default)]
    rationale: String,
}

/// D10: an operator's verified closure of a plan checkout that keeps an erasure pending.
async fn resolve_erasure_checkout(
    State(state): State_,
    headers: HeaderMap,
    Path(id): Path<i64>,
    body: Bytes,
) -> Reply<billing_core::app::ErasureCheckoutView> {
    let caller = admin(&state, &headers).await?;
    let body: ClosureBody = parse(&body)?;
    let closure = billing_core::erasure::CheckoutClosure {
        resolved_by: caller.email,
        checkout_cannot_complete: body.checkout_cannot_complete,
        no_renewing_subscription: body.no_renewing_subscription,
        evidence_ref: body.evidence_ref,
        rationale: body.rationale,
    };
    Ok(Json(state.billing.resolve_erasure_checkout(id, &closure).await?))
}

// ---- host signup ---------------------------------------------------------------

#[derive(Deserialize)]
struct SignupBody {
    account_id: String,
    email: String,
    full_name: Option<String>,
    /// The instant the host's identity system created the account.
    created_at_micros: i64,
}

/// The host's signup system reports a trusted account creation instant.
/// Authenticated by a service token, never by a user session.
async fn signup(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::Provisioned> {
    let Some(expected) = state.signup_token.as_deref() else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", "No such endpoint."));
    };
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !bool::from(presented.as_bytes().ct_eq(expected.as_bytes())) {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthenticated", "The signup service token is wrong."));
    }
    let body: SignupBody = parse(&body)?;
    let account = AccountId::parse(&body.account_id)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "account_id must be a UUID."))?;
    if body.email.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "An email is required."));
    }
    // A signup cannot be in the future: that would pre-grant a trial window.
    if Micros(body.created_at_micros) > state.billing.now().plus_secs(86_400) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "created_at_micros is in the future."));
    }
    let customer = Customer {
        account,
        email: body.email.trim().to_string(),
        full_name: body.full_name.filter(|n| !n.trim().is_empty()),
        created_at: Micros(body.created_at_micros),
    };
    Ok(Json(state.billing.provision(customer).await?))
}

// ---- provider and browser routes ---------------------------------------------------

/// Parses `token` from a JSON body or an `application/x-www-form-urlencoded`
/// body: Iyzico posts the customer's browser back with a form.
fn token_from(headers: &HeaderMap, body: &Bytes) -> Option<String> {
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    if content_type.contains("json") {
        let value: Value = serde_json::from_slice(body).ok()?;
        return value.get("token").and_then(Value::as_str).map(str::to_string).filter(|t| !t.is_empty());
    }
    let text = std::str::from_utf8(body).ok()?;
    text.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if key != "token" {
            return None;
        }
        let decoded = percent_encoding::percent_decode_str(&value.replace('+', " ")).decode_utf8().ok()?.to_string();
        Some(decoded).filter(|t| !t.is_empty())
    })
}

/// The source sends a cold `GET /checkout` home; it never starts a checkout.
async fn checkout_get() -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, "/"), (header::CACHE_CONTROL, "no-store")]).into_response()
}

struct CheckoutForm {
    order: Vec<u8>,
    access_token: String,
}

fn form_error(status: StatusCode, code: &str, message: &str) -> ApiError {
    ApiError::new(status, code, message)
}

/// D17: a single-part `application/x-www-form-urlencoded` body with exactly the
/// fields `order` (base64url without padding of the order JSON) and
/// `access_token` (RFC 6750 section 2.2), each once.
fn checkout_form(body: &[u8]) -> Result<CheckoutForm, ApiError> {
    use base64::Engine;
    let bad = |message: &str| form_error(StatusCode::BAD_REQUEST, "invalid_request", message);
    let text = std::str::from_utf8(body).map_err(|_| bad("the checkout form is not ASCII"))?;
    if !text.is_ascii() {
        return Err(bad("the checkout form is not ASCII"));
    }
    let (mut order, mut token) = (None, None);
    for pair in text.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').ok_or_else(|| bad("a checkout form field has no value"))?;
        let value = percent_encoding::percent_decode_str(&value.replace('+', " "))
            .decode_utf8()
            .map_err(|_| bad("a checkout form field is not text"))?
            .into_owned();
        let slot = match key {
            "order" => &mut order,
            "access_token" => &mut token,
            _ => return Err(bad("the checkout form carries an unknown field")),
        };
        if slot.replace(value).is_some() {
            return Err(bad("a checkout form field is repeated"));
        }
    }
    let token = token.filter(|t| !t.is_empty()).ok_or_else(|| form_error(StatusCode::UNAUTHORIZED, "unauthenticated", SIGN_IN_MESSAGE))?;
    if token.len() > CHECKOUT_TOKEN_MAX_BYTES || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(form_error(StatusCode::UNAUTHORIZED, "unauthenticated", SIGN_IN_MESSAGE));
    }
    let encoded = order.ok_or_else(|| bad("the checkout form has no order"))?;
    if encoded.len() > CHECKOUT_ORDER_MAX_BYTES.div_ceil(3) * 4 {
        return Err(bad("the order is too large"));
    }
    let order = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded.as_bytes()).map_err(|_| bad("the order is not base64url"))?;
    if order.len() > CHECKOUT_ORDER_MAX_BYTES {
        return Err(bad("the order is too large"));
    }
    Ok(CheckoutForm { order, access_token: token })
}

/// D17: the one checkout start of a hidden same-origin form post, answered with the
/// isolated checkout document (or a 303 to a provider's hosted URL). The bearer token
/// is accepted from the body on this route only; it is never echoed or logged.
async fn checkout_document(State(state): State_, headers: HeaderMap, body: Bytes) -> Response {
    let Some(document) = state.checkout_document.clone() else {
        return ApiError::new(StatusCode::NOT_FOUND, "not_found", "No such endpoint.").into_response();
    };
    let mut locale = None;
    let outcome = async {
        let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if content_type.split(';').next().map(str::trim) != Some("application/x-www-form-urlencoded") {
            return Err(form_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid_request", "The checkout form must be form-urlencoded."));
        }
        if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(document.site_origin.as_str()) {
            return Err(form_error(StatusCode::FORBIDDEN, "forbidden", "The checkout form must come from this site."));
        }
        if headers.contains_key(header::AUTHORIZATION) {
            return Err(form_error(StatusCode::BAD_REQUEST, "invalid_request", "Use one credential method."));
        }
        if body.len() > CHECKOUT_FORM_MAX_BYTES {
            return Err(form_error(StatusCode::PAYLOAD_TOO_LARGE, "invalid_request", "The checkout form is too large."));
        }
        let form = checkout_form(&body)?;
        let request = parse::<OrderBody>(&Bytes::from(form.order))?.into_request();
        locale = Some(request.ui_locale.clone());
        let mut bearer = HeaderMap::new();
        let value = axum::http::HeaderValue::from_str(&format!("Bearer {}", form.access_token))
            .map_err(|_| form_error(StatusCode::UNAUTHORIZED, "unauthenticated", SIGN_IN_MESSAGE))?;
        bearer.insert(header::AUTHORIZATION, value);
        let caller = authed(&state, &bearer).await?;
        let view = state.billing.start_checkout(caller.account, &request).await?;
        Ok((view, request.ui_locale))
    }
    .await;
    let outcome = match outcome {
        Ok((view, locale)) => match (view.form_content, view.url) {
            (Some(content), _) if !content.is_empty() => CheckoutOutcome::Form { payment_id: view.payment_id, gateway: view.gateway, content, locale },
            (_, Some(url)) if url.starts_with("https://") => {
                return (StatusCode::SEE_OTHER, [(header::LOCATION, url.as_str()), (header::CACHE_CONTROL, "no-store")]).into_response();
            }
            // A started checkout with nothing to show is a failure, never a guessed success.
            _ => CheckoutOutcome::Failed { status: 502, code: "checkout_failed".into(), message: CHECKOUT_START_FAILED.into(), locale: Some(locale) },
        },
        Err(error) => CheckoutOutcome::Failed { status: error.status.as_u16(), code: error.code, message: error.message, locale },
    };
    let rendered = document.page.render(&outcome);
    let status = StatusCode::from_u16(outcome.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let csp = axum::http::HeaderValue::from_str(&rendered.content_security_policy)
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("default-src 'none'"));
    (
        status,
        [
            (header::CONTENT_TYPE, axum::http::HeaderValue::from_static("text/html; charset=utf-8")),
            (header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store")),
            (header::CONTENT_SECURITY_POLICY, csp),
        ],
        rendered.html,
    )
        .into_response()
}

/// A browser form post that accepts HTML gets the host's page, when one is
/// configured; every other caller keeps the JSON answer.
fn wants_page(headers: &HeaderMap) -> bool {
    let header_text = |name| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    header_text(header::CONTENT_TYPE).starts_with("application/x-www-form-urlencoded") && header_text(header::ACCEPT).contains("text/html")
}

async fn confirm(State(state): State_, headers: HeaderMap, body: Bytes) -> Response {
    let result = match token_from(&headers, &body) {
        // No token in the request shows the failure page.
        None => Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", "No payment was found for this checkout.")),
        Some(token) => state.billing.confirm_checkout("iyzico", &token).await.map_err(ApiError::from),
    };
    // The one confirmation above is the only one: the page renders its result and
    // never confirms again (P5 D8).
    let page = state.confirm_page.as_ref().filter(|_| wants_page(&headers));
    match (page, result) {
        (None, Ok(view)) => Json(view).into_response(),
        (None, Err(error)) => error.into_response(),
        (Some(page), result) => {
            let outcome = match result {
                Ok(view) => ConfirmOutcome::Confirmed { view },
                Err(error) => ConfirmOutcome::Failed { status: error.status.as_u16(), code: error.code, message: error.message },
            };
            let status = StatusCode::from_u16(outcome.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (
                status,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")],
                page.render(&outcome),
            )
                .into_response()
        }
    }
}

async fn subscription_callback(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::NotificationOutcome> {
    Ok(Json(state.billing.process_subscription_webhook(&header_pairs(&headers), &body).await?))
}

async fn order_webhook(State(state): State_, headers: HeaderMap, body: Bytes) -> Reply<billing_core::app::NotificationOutcome> {
    Ok(Json(state.billing.process_order_webhook(&header_pairs(&headers), &body).await?))
}
