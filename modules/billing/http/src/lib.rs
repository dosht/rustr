//! Axum contract for billing-core.
//!
//! Identity is a host port: every account-scoped route takes the account from
//! the verified caller, never from the request. Admin rights come from the
//! same verified identity. Provider routes authenticate by signature over raw
//! bytes, not by session. Nothing in a request body can name an account, an
//! amount, a completion count or a bypass.

mod routes;

use std::sync::Arc;

use axum::http::HeaderMap;
use billing_core::app::{Billing, ConfirmView};
use billing_core::types::{AccountId, BoxFuture};
use serde::Serialize;

pub use routes::{authenticated, provider};

/// A verified caller. `is_admin` is decided by the host from verified identity
/// and host data, never from anything the client sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    pub is_admin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthError {
    /// No credential, or a credential that does not verify.
    Unauthenticated(String),
    /// The identity provider could not be reached.
    Unavailable(String),
    /// A verified identity whose account has an erasure barrier. Not an identity-provider outage.
    AccountErased(AccountId),
}

pub trait BillingIdentity: Send + Sync {
    fn authenticate<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Caller, AuthError>>;

    /// Authenticate only the owner of a self-erasure request, never an administrator or an ordinary
    /// caller. Hosts may omit contact adoption/refresh here so erasure can continue after local
    /// contact data is gone. Credential verification must remain identical to normal authentication.
    /// Returning only the account prevents this path from carrying email-based/admin authority.
    fn authenticate_for_erasure<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<AccountId, AuthError>> {
        Box::pin(async move { self.authenticate(headers).await.map(|caller| caller.account) })
    }
}

/// The single result of one provider confirmation, as the browser page receives
/// it (P5 D8). `Confirmed` carries the stored status honestly, pending included;
/// `Failed` keeps the HTTP status and the error the JSON answer would carry.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ConfirmOutcome {
    Confirmed { view: ConfirmView },
    Failed { status: u16, code: String, message: String },
}

impl ConfirmOutcome {
    /// The HTTP status the page is served with: what the JSON answer would use.
    pub fn http_status(&self) -> u16 {
        match self {
            ConfirmOutcome::Confirmed { .. } => 200,
            ConfirmOutcome::Failed { status, .. } => *status,
        }
    }
}

/// Renders the confirmation page from the one confirmation result. The host
/// supplies it; billing calls the provider confirmation exactly once and never
/// lets the renderer confirm again.
pub trait ConfirmPage: Send + Sync {
    fn render(&self, outcome: &ConfirmOutcome) -> String;
}

/// D17 limits of the checkout document POST.
pub use billing_core::presentation::purchase::{CHECKOUT_FORM_MAX_BYTES, CHECKOUT_ORDER_MAX_BYTES, CHECKOUT_TOKEN_MAX_BYTES};

/// The one checkout start a `POST /checkout` document shows.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CheckoutOutcome {
    /// The provider's hosted form, as the provider returned it. The renderer puts
    /// it in the blank checkout document and adds its nonce to the form's scripts.
    Form { payment_id: i64, gateway: String, content: String, locale: String },
    /// The request was refused or the start failed (source failure page).
    /// `locale` is the order's locale when the order was readable.
    Failed { status: u16, code: String, message: String, locale: Option<String> },
}

impl CheckoutOutcome {
    pub fn http_status(&self) -> u16 {
        match self {
            CheckoutOutcome::Form { .. } => 200,
            CheckoutOutcome::Failed { status, .. } => *status,
        }
    }
}

/// The checkout document and the policy that must travel with it: the nonce in
/// the policy is the one the document's scripts carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedCheckout {
    pub html: String,
    pub content_security_policy: String,
}

/// Renders the blank checkout document from the one start result. The host
/// supplies it; billing starts the checkout once and never lets it start again.
pub trait CheckoutPage: Send + Sync {
    fn render(&self, outcome: &CheckoutOutcome) -> RenderedCheckout;
}

#[derive(Clone)]
pub struct CheckoutDocument {
    pub page: Arc<dyn CheckoutPage>,
    /// The exact `Origin` a checkout form post must carry, for example
    /// `https://transgate.ai`.
    pub site_origin: String,
}

pub struct BillingHttp {
    pub billing: Arc<Billing>,
    pub identity: Arc<dyn BillingIdentity>,
    /// A shared secret for the host's own signup system, which reports trusted
    /// account creation instants. `None` disables the signup route.
    pub signup_token: Option<String>,
    /// `None` keeps `/checkout/confirm` JSON-only for every caller.
    pub confirm_page: Option<Arc<dyn ConfirmPage>>,
    /// `None` leaves `POST /checkout` unavailable (404); `GET /checkout` still goes home.
    pub checkout_document: Option<CheckoutDocument>,
}

/// HTTPS only, except an explicit loopback fixture; scheme and host only.
fn valid_site_origin(origin: &str) -> bool {
    let rest = if let Some(rest) = origin.strip_prefix("https://") {
        rest
    } else if let Some(rest) = origin.strip_prefix("http://") {
        let host = rest.rsplit_once(':').map(|(h, _)| h).unwrap_or(rest);
        if !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
            return false;
        }
        rest
    } else {
        return false;
    };
    !rest.is_empty() && !rest.contains(['/', '?', '#', '@', ' '])
}

impl BillingHttp {
    pub fn new(billing: Arc<Billing>, identity: Arc<dyn BillingIdentity>) -> BillingHttp {
        BillingHttp { billing, identity, signup_token: None, confirm_page: None, checkout_document: None }
    }

    pub fn with_signup_token(mut self, token: impl Into<String>) -> BillingHttp {
        self.signup_token = Some(token.into());
        self
    }

    pub fn with_confirm_page(mut self, page: Arc<dyn ConfirmPage>) -> BillingHttp {
        self.confirm_page = Some(page);
        self
    }

    /// Enables the D17 `POST /checkout` document. `site_origin` must be an HTTPS
    /// origin (scheme and host, no path), or plain HTTP only for a loopback fixture.
    pub fn with_checkout_page(mut self, page: Arc<dyn CheckoutPage>, site_origin: &str) -> Result<BillingHttp, String> {
        if !valid_site_origin(site_origin) {
            return Err(format!("{site_origin:?} is not an HTTPS origin (or a loopback fixture)"));
        }
        self.checkout_document = Some(CheckoutDocument { page, site_origin: site_origin.to_string() });
        Ok(self)
    }
}
