//! Transgate's billing host: catalog and lists, customer directory, JWT
//! identity, mail delivery and the composition of the billing module into
//! routes the Transgate server mounts.
//!
//! Configuration is complete or refused. A missing required setting is an
//! error naming it; no failure ever selects a fake adapter.

pub mod analytics;
pub mod catalog;
pub mod directory;
pub mod identity;
pub mod mail;
pub mod settings;
pub mod session;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use billing_core::app::{Audience, Billing, BillingConfig, FreemiumPolicy};
use billing_core::notify::NotifyConfig;
use billing_core::types::{Clock, Micros};
use billing_http::{BillingHttp, BillingIdentity, authenticated, provider};
use billing_iyzico::{IyzicoClient, IyzicoConfig, IyzicoWebhook};
use billing_lemonsqueezy::{LemonClient, LemonConfig};
use billing_postgres::{PgBillingStore, connect_owner, connect_runtime, migrate};
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tokio::task::JoinHandle;

use crate::directory::PgCustomerDirectory;
use crate::identity::JwtIdentity;
use crate::mail::HttpMailer;
use crate::analytics::PostHog;
use crate::directory::SupabaseAdmin;
use crate::settings::Settings;

pub use settings::settings_from;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Micros {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        Micros(elapsed.as_micros() as i64)
    }
}

/// Both browser documents are supplied by the host, without moving billing decisions into it.
pub struct Pages {
    pub confirm: Arc<dyn billing_http::ConfirmPage>,
    pub checkout: billing_http::CheckoutDocument,
}

pub struct Transgate {
    pub router: Router,
    pub billing: Arc<Billing>,
    /// The restricted runtime pool billing runs on. The job host uses this same pool, so a job's
    /// status change and its billing joins are one transaction on one database.
    pub pool: PgPool,
    /// The verified caller, with the contact refresh the billing routes use.
    pub identity: Arc<dyn BillingIdentity>,
    /// Profile and read-only owner receipt authentication; never a general admin bypass.
    pub session: Arc<session::SessionIdentity>,
    pub mailer: Arc<dyn billing_core::ports::Mailer>,
    pub analytics: Option<Arc<dyn billing_core::ports::AnalyticsSink>>,
    /// The configured Iyzico client: the gateway the erasure cancels recurring subscriptions through.
    pub iyzico: Option<Arc<IyzicoClient>>,
    /// The trusted auth directory's admin client (lookup and the account's deletion).
    pub auth_admin: Option<Arc<SupabaseAdmin>>,
    relay: Option<JoinHandle<()>>,
}

impl Transgate {
    /// Stops the background delivery task. In-flight claims expire on their lease.
    pub fn stop(&mut self) {
        if let Some(task) = self.relay.take() {
            task.abort();
        }
    }
}

/// What the host serves when billing is not configured: a readiness answer
/// that says so, distinct from process liveness.
pub fn unconfigured_router() -> Router {
    async fn not_ready() -> impl IntoResponse {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ready": false, "reason": "billing is not configured: set TRANSGATE_DATABASE_URL and the settings it needs"})),
        )
    }
    Router::new().route("/api/billing/ready", get(not_ready))
}

/// The host's schema, with its own migration history.
pub async fn migrate_host(owner: &PgPool) -> Result<(), String> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.create_schema("transgate_billing");
    migrator.dangerous_set_table_name("transgate_billing._sqlx_migrations");
    migrator.run(owner).await.map_err(|e| format!("transgate billing migration failed: {e}"))
}

pub fn billing_config(settings: &Settings) -> BillingConfig {
    let (sales, support) = (settings.mail.sales_email.clone(), settings.mail.support_email.clone());
    BillingConfig {
        freemium: FreemiumPolicy { name: "freemium".into(), minutes: 20, months: 2 },
        catalog: catalog::catalog(),
        rate: settings.rate,
        lists: catalog::freemium_lists(),
        audience: Audience {
            special_offer_emails: settings.eligibility.special_offer_emails.clone(),
            special_clients: settings.eligibility.special_clients.clone(),
        },
        special_client_offer: settings.eligibility.special_client_offer.clone(),
        notify: NotifyConfig { brand: "Transgate".into(), sales_email: sales, support_email: support },
        public_base_url: settings.public_url.clone(),
    }
}

/// How long contact facts are trusted before the auth directory is asked again.
const CONTACT_REFRESH_AFTER: Duration = Duration::from_secs(900);

/// Builds the real runtime with the system clock.
pub async fn compose(settings: Settings, pages: Option<Pages>) -> Result<Transgate, String> {
    compose_with(settings, Arc::new(SystemClock), pages).await
}

/// Builds the real runtime: migrates with owner authority when given, connects
/// as the restricted runtime role (refused otherwise), wires the configured
/// gateways, the required mail delivery, the optional analytics destination and the
/// trusted auth directory, and starts the outbox relay. The clock is injectable so
/// retry timing can be proven without waiting.
pub async fn compose_with(settings: Settings, clock: Arc<dyn Clock>, pages: Option<Pages>) -> Result<Transgate, String> {
    if let Some(url) = &settings.migration_database_url {
        let owner = connect_owner(url).await.map_err(|e| e.to_string())?;
        migrate(&owner).await.map_err(|e| e.to_string())?;
        migrate_host(&owner).await?;
    }
    let options = PgConnectOptions::from_str(&settings.database_url).map_err(|e| format!("TRANSGATE_DATABASE_URL: {e}"))?;
    let pool = connect_runtime(options, 10).await.map_err(|e| e.to_string())?;

    let mut customers = PgCustomerDirectory::new(pool.clone());
    let mut auth_admin: Option<Arc<SupabaseAdmin>> = None;
    if let Some(a) = &settings.auth_directory {
        let auth = Arc::new(SupabaseAdmin::new(&a.url, &a.service_key, Duration::from_secs(10))?);
        auth_admin = Some(auth.clone());
        customers = customers.with_auth(auth, CONTACT_REFRESH_AFTER);
    }
    let directory = Arc::new(customers);
    let directory_for_identity = directory.clone();
    let mut billing = Billing::new(Arc::new(PgBillingStore::new(pool.clone())), clock, directory, billing_config(&settings)).requires_mail();

    let mut iyzico: Option<Arc<IyzicoClient>> = None;
    if let Some(i) = &settings.iyzico {
        let client = Arc::new(
            IyzicoClient::new(IyzicoConfig {
                base_url: i.base_url.clone(),
                api_key: i.api_key.clone(),
                secret_key: i.secret_key.clone(),
                environment: i.environment.clone(),
                timeout: Duration::from_secs(20),
            })?,
        );
        let hook = IyzicoWebhook::new(&i.merchant_id, &i.secret_key, &i.environment)?;
        billing = billing.with_checkout(client.clone()).with_subscription_gateway(client.clone()).with_subscription_webhook(Arc::new(hook));
        iyzico = Some(client);
    }
    if let Some(l) = &settings.lemon {
        let client = LemonClient::new(LemonConfig {
            base_url: l.base_url.clone(),
            api_key: l.api_key.clone(),
            store_id: l.store_id.clone(),
            variant_id: l.variant_id.clone(),
            signing_secret: l.signing_secret.clone(),
            environment: l.environment.clone(),
            timeout: Duration::from_secs(20),
        })?;
        let hook = client.webhook();
        billing = billing.with_checkout(Arc::new(client)).with_order_webhook(Arc::new(hook));
    }
    let m = &settings.mail;
    let mailer: Arc<dyn billing_core::ports::Mailer> = Arc::new(HttpMailer::new(&m.url, &m.api_key, &m.from, Duration::from_secs(15))?);
    billing = billing.with_mailer(mailer.clone());
    let mut analytics: Option<Arc<dyn billing_core::ports::AnalyticsSink>> = None;
    if let Some(a) = &settings.analytics {
        let sink: Arc<dyn billing_core::ports::AnalyticsSink> = Arc::new(PostHog::new(&a.host, &a.api_key, Duration::from_secs(3))?);
        billing = billing.with_analytics(sink.clone());
        analytics = Some(sink);
    }
    let billing = Arc::new(billing);

    let jwt = JwtIdentity::new(
        settings.identity.hs256_secret.as_deref(),
        settings.identity.jwks_url.clone(),
        settings.identity.dev_tokens,
        settings.identity.admin_emails.clone(),
    )?;
    // The account's own authenticated access is when its contact facts are re-confirmed against
    // the trusted auth directory (at most once per refresh interval), so a verified email change
    // reaches purchase mail and the admin lookup without anyone reporting it.
    let refreshed = Arc::new(ContactRefresh { inner: Arc::new(jwt), directory: directory_for_identity, pool: pool.clone() });
    let session = Arc::new(session::SessionIdentity { identity: refreshed.clone() });
    let identity: Arc<dyn BillingIdentity> = refreshed;
    let mut http = BillingHttp::new(billing.clone(), identity.clone());
    if let Some(pages) = pages {
        http = http.with_confirm_page(pages.confirm)
            .with_checkout_page(pages.checkout.page, &pages.checkout.site_origin)?;
    }
    if let Some(token) = &settings.signup_token {
        http = http.with_signup_token(token.clone());
    }
    let state = Arc::new(http);
    let router = Router::new().nest("/api", authenticated(state.clone())).merge(provider(state));

    let relay = Some({
        let billing = billing.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Err(e) = billing.relay_outbox(50, 60).await {
                    eprintln!("transgate billing: outbox relay: {e}");
                }
            }
        })
    });
    Ok(Transgate { router, billing, pool, identity, session, mailer, analytics, iyzico, auth_admin, relay })
}

/// Authenticates with the inner verifier, then lets the directory adopt the caller or
/// re-confirm its contact facts when they are due. When the trusted directory is due to be
/// asked and cannot answer, the request fails explicitly as unavailable: it is not served
/// from facts that may have changed (checkout and cancellation mail would reach an old
/// address). The stored facts stay, still due, and the next access retries.
struct ContactRefresh {
    inner: Arc<dyn BillingIdentity>,
    directory: Arc<PgCustomerDirectory>,
    pool: PgPool,
}

impl ContactRefresh {
    async fn erasing(&self, account: billing_core::types::AccountId) -> Result<bool, billing_http::AuthError> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM billing.erasure_barriers WHERE account_id = $1)")
            .bind(account.0)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| billing_http::AuthError::Unavailable(format!("the account erasure barrier cannot be checked: {e}")))
    }

    async fn refresh(&self, caller: billing_http::Caller) -> Result<billing_http::Caller, billing_http::AuthError> {
        if self.erasing(caller.account).await? {
            return Err(billing_http::AuthError::AccountErased(caller.account));
        }
        if self.directory.has_auth() && let Err(e) = billing_core::ports::CustomerDirectory::by_account(&*self.directory, caller.account).await {
            // Erasure may have started after the check above. Its trigger refused adoption;
            // that refusal is a 410, not an outage. Actual directory failures remain 503.
            if self.erasing(caller.account).await? {
                return Err(billing_http::AuthError::AccountErased(caller.account));
            }
            return Err(billing_http::AuthError::Unavailable(format!("the account directory cannot confirm this account: {e}")));
        }
        // Fence a successful contact read that raced a newly opened barrier before email-derived
        // admin authority escapes the composition.
        if self.erasing(caller.account).await? {
            return Err(billing_http::AuthError::AccountErased(caller.account));
        }
        Ok(caller)
    }
}

impl BillingIdentity for ContactRefresh {
    fn authenticate<'a>(&'a self, headers: &'a axum::http::HeaderMap) -> billing_core::types::BoxFuture<'a, Result<billing_http::Caller, billing_http::AuthError>> {
        Box::pin(async move { self.refresh(self.inner.authenticate(headers).await?).await })
    }

    fn authenticate_for_erasure<'a>(&'a self, headers: &'a axum::http::HeaderMap) -> billing_core::types::BoxFuture<'a, Result<billing_core::types::AccountId, billing_http::AuthError>> {
        Box::pin(async move {
            let caller = self.inner.authenticate(headers).await?;
            // Unbarred users retain normal adoption/refresh (including first-request deletion and
            // real directory outages). Only a permanent erasure barrier permits the bypass, even
            // when it appeared during contact hydration. Never return an admin/email-bearing actor.
            match self.refresh(caller).await {
                Ok(caller) => Ok(caller.account),
                Err(billing_http::AuthError::AccountErased(account)) => Ok(account),
                Err(e) => Err(e),
            }
        })
    }
}
