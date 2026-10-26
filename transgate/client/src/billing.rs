//! The authenticated billing routes (`billing_http::authenticated`, mounted at
//! `/api`), typed with billing-core's own DTOs. Financial facts stay server
//! derived: nothing here computes a price, VAT or balance.
use billing_core::app::CancelView;
use billing_core::presentation::invoice::InvoiceContext;
use billing_core::presentation::invoices::AdminLookupReply;
use billing_core::presentation::purchase::{CatalogView, CouponReply, OrderBody, PurchaseFailure, QuoteView};
use billing_core::presentation::subscription::SubscriptionFacts;
use billing_core::pricing::SpecialClientAlert;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::api::{ApiError, HttpApi};

/// The header projection of `GET /api/billing/overview`; configured alert facts
/// are decoded verbatim, never recomputed by the client.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Balance {
    #[serde(rename = "balance")]
    pub minutes: Option<i64>,
    pub is_freemium: bool,
    pub special_client_alert: Option<SpecialClientAlert>,
}

impl ApiError {
    /// R's mapping for the purchase and subscription leaves (billing.md section 3).
    pub fn purchase_failure(&self) -> PurchaseFailure {
        match self {
            ApiError::SignedOut | ApiError::Http { status: 401, .. } => PurchaseFailure::Unauthenticated,
            ApiError::Http { status: 410, .. } => PurchaseFailure::Erased,
            ApiError::Http { status, code: Some(code), .. } if (400..500).contains(status) => PurchaseFailure::Refused { code: code.clone() },
            _ => PurchaseFailure::Unavailable,
        }
    }
}

impl HttpApi {
    pub async fn catalog(&self, user: Uuid) -> Result<CatalogView, ApiError> {
        self.json(user, Method::GET, "/billing/catalog", None).await
    }

    pub async fn balance(&self, user: Uuid) -> Result<Balance, ApiError> {
        self.json(user, Method::GET, "/billing/overview", None).await
    }

    /// The charged quote with its separate `display_amount` (D8). Never carries attribution.
    pub async fn quote(&self, user: Uuid, order: &OrderBody) -> Result<QuoteView, ApiError> {
        let body = serde_json::to_value(order).map_err(|e| ApiError::Decode(e.to_string()))?;
        self.json(user, Method::POST, "/billing/quote", Some(&body)).await
    }

    pub async fn coupon(&self, user: Uuid, id: &str) -> Result<CouponReply, ApiError> {
        let id: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        self.json(user, Method::GET, &format!("/billing/coupon/{id}"), None).await
    }

    /// The Subscription tab's facts, or `None` without an open plan.
    pub async fn subscription(&self, user: Uuid) -> Result<Option<SubscriptionFacts>, ApiError> {
        self.json(user, Method::GET, "/billing/subscription", None).await
    }

    pub async fn cancel_subscription(&self, user: Uuid, payment_id: i64) -> Result<CancelView, ApiError> {
        self.json(user, Method::POST, "/billing/cancel-subscription", Some(&json!({ "payment_id": payment_id }))).await
    }

    /// The owner's invoice context (D7).
    pub async fn invoices(&self, user: Uuid) -> Result<InvoiceContext, ApiError> {
        self.json(user, Method::GET, "/billing/invoices", None).await
    }

    /// Administrator-authorized lookup; temporary Bill To never leaves the document capture.
    pub async fn customer_invoices(&self, user: Uuid, email: &str) -> Result<AdminLookupReply, ApiError> {
        self.json(user, Method::POST, "/billing/admin/customer-invoices", Some(&json!({ "email": email }))).await
    }
}
