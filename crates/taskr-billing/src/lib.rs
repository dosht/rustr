//! taskr's billing sample: a separate account with 10 free credits, one
//! metered sample operation, and one paid credit pack through LemonSqueezy.
//!
//! It is additive. Ordinary task creation is never metered and nothing here
//! changes taskr-core. The billing module is mounted with taskr's own
//! configured identity; billing for an account starts at an explicit
//! enrollment, never at a first guess.
//!
//! Credits are the module's minutes: a "package of hours" in the module's
//! vocabulary is a pack of credits here (one credit per unit, USD 0.10 each).

pub mod identity;
pub mod work;

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::routing::post;
use billing_core::app::{Audience, Billing, BillingConfig, FreemiumPolicy};
use billing_core::decimal::Decimal;
use billing_core::freemium::FreemiumLists;
use billing_core::notify::NotifyConfig;
use billing_core::pricing::{Catalog, PaygEntry};
use billing_core::types::{Clock, Micros};
use billing_http::{BillingHttp, authenticated, provider};
use billing_lemonsqueezy::{LemonClient, LemonConfig};
use billing_postgres::{PgBillingStore, connect_owner, connect_runtime, migrate};
use sqlx::postgres::PgConnectOptions;
use taskr_core::shared::access::Identity;

use crate::identity::TaskrIdentity;
use crate::work::{PgCustomerDirectory, WorkState, enroll, sample_work};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Micros {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        Micros(elapsed.as_micros() as i64)
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    /// The restricted runtime login. An owner connection is refused.
    pub database_url: String,
    pub migration_database_url: Option<String>,
    pub public_url: String,
    pub lemon_base_url: String,
    pub lemon_api_key: String,
    pub lemon_store_id: String,
    pub lemon_variant_id: String,
    pub lemon_signing_secret: String,
    pub lemon_environment: String,
}

/// `Ok(None)`: no billing database, so taskr runs exactly as before. Otherwise
/// the LemonSqueezy group must be complete; nothing falls back to a fake.
pub fn settings_from(get: &dyn Fn(&str) -> Option<String>) -> Result<Option<Settings>, String> {
    let Some(database_url) = get("TASKR_BILLING_DATABASE_URL") else { return Ok(None) };
    let need = |name: &str| get(name).ok_or_else(|| format!("{name} is required when TASKR_BILLING_DATABASE_URL is set"));
    let environment = need("LEMONSQUEEZY_ENVIRONMENT")?;
    if !matches!(environment.as_str(), "test" | "live") {
        return Err("LEMONSQUEEZY_ENVIRONMENT must be test or live".into());
    }
    Ok(Some(Settings {
        database_url,
        migration_database_url: get("TASKR_BILLING_MIGRATION_DATABASE_URL"),
        public_url: need("TASKR_PUBLIC_URL")?,
        lemon_base_url: get("LEMONSQUEEZY_API_URL").unwrap_or_else(|| "https://api.lemonsqueezy.com".into()),
        lemon_api_key: need("LEMONSQUEEZY_API_KEY")?,
        lemon_store_id: need("LEMONSQUEEZY_STORE_ID")?,
        lemon_variant_id: need("LEMONSQUEEZY_VARIANT_ID")?,
        lemon_signing_secret: need("LEMONSQUEEZY_WEBHOOK_SECRET")?,
        lemon_environment: environment,
    }))
}

fn d(text: &str) -> Decimal {
    text.parse().expect("catalog decimal literal")
}

/// One pack: credits at USD 0.10 each, 5 to 1000 per purchase, valid a year.
pub fn billing_config(settings: &Settings) -> BillingConfig {
    BillingConfig {
        // 10 free credits, valid for 12 months from enrollment.
        freemium: FreemiumPolicy { name: "freemium".into(), minutes: 10, months: 12 },
        catalog: Catalog {
            plans: vec![],
            payg: Some(PaygEntry {
                stored_name: "credit-pack".into(),
                catalog_label: "CREDIT PACK".into(),
                usd_per_hour: d("0.10"),
                struck_per_hour: d("0.15"),
                min_hours: 5,
                max_hours: 1000,
                minutes_per_hour: 1,
            }),
            locale_discount_locales: BTreeSet::new(),
            special_offer_try_per_hour: None,
        },
        rate: d("44"),
        lists: FreemiumLists::default(),
        audience: Audience::default(),
        special_client_offer: None,
        notify: NotifyConfig { brand: "taskr".into(), sales_email: String::new(), support_email: String::new() },
        public_base_url: settings.public_url.clone(),
    }
}

pub struct TaskrBilling {
    /// Billing routes (account-scoped, enrollment, the sample operation) and the
    /// LemonSqueezy webhook, ready to merge into taskr's router.
    pub router: Router,
    pub billing: Arc<Billing>,
    /// The verified restricted runtime pool. Sample consumers that erase an account in the same
    /// transaction as billing (taskr-work) run on this pool, so they share its actual database.
    pub pool: sqlx::PgPool,
    /// taskr's own identity mapped onto billing accounts: the account an erasure removes.
    pub identity: Arc<dyn billing_http::BillingIdentity>,
}

/// Migrates with owner authority when given, connects as the restricted runtime
/// role (anything stronger is refused), and wires LemonSqueezy.
pub async fn compose(settings: Settings, identity: Arc<dyn Identity>) -> Result<TaskrBilling, String> {
    if let Some(url) = &settings.migration_database_url {
        let owner = connect_owner(url).await.map_err(|e| e.to_string())?;
        migrate(&owner).await.map_err(|e| e.to_string())?;
        migrate_host(&owner).await?;
    }
    let options = PgConnectOptions::from_str(&settings.database_url).map_err(|e| format!("TASKR_BILLING_DATABASE_URL: {e}"))?;
    let pool = connect_runtime(options, 8).await.map_err(|e| e.to_string())?;

    let lemon = LemonClient::new(LemonConfig {
        base_url: settings.lemon_base_url.clone(),
        api_key: settings.lemon_api_key.clone(),
        store_id: settings.lemon_store_id.clone(),
        variant_id: settings.lemon_variant_id.clone(),
        signing_secret: settings.lemon_signing_secret.clone(),
        environment: settings.lemon_environment.clone(),
        timeout: Duration::from_secs(20),
    })?;
    let hook = lemon.webhook();
    let directory = Arc::new(PgCustomerDirectory::new(pool.clone()));
    let store: Arc<dyn billing_core::ports::BillingStore> = Arc::new(PgBillingStore::new(pool.clone()));
    let billing = Arc::new(
        Billing::new(store.clone(), Arc::new(SystemClock), directory, billing_config(&settings))
            .with_checkout(Arc::new(lemon))
            .with_order_webhook(Arc::new(hook)),
    );
    let billing_identity: Arc<dyn billing_http::BillingIdentity> = Arc::new(TaskrIdentity::new(identity));
    let state = Arc::new(BillingHttp::new(billing.clone(), billing_identity.clone()));
    let work_state = Arc::new(WorkState { billing: billing.clone(), identity: billing_identity.clone(), pool: pool.clone() });
    let sample = Router::new()
        .route("/billing/enroll", post(enroll))
        .route("/billing/sample/work", post(sample_work))
        .with_state(work_state);
    let router = Router::new()
        .nest("/api", authenticated(state.clone()).merge(sample))
        .merge(provider(state));
    Ok(TaskrBilling { router, billing, pool, identity: billing_identity })
}

/// The sample's schema, with its own migration history.
pub async fn migrate_host(owner: &sqlx::PgPool) -> Result<(), String> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.create_schema("taskr_billing");
    migrator.dangerous_set_table_name("taskr_billing._sqlx_migrations");
    migrator.run(owner).await.map_err(|e| format!("taskr billing migration failed: {e}"))
}

/// What erasing an account's enrolled sample resources removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnrolledSampleErased {
    pub customers: u64,
    pub sample_work: u64,
}

/// Removes the account's enrolled sample resources (its directory row and its metered sample
/// work) in the caller's erasure transaction. It takes the account's sample lock exclusively first:
/// every write of these rows holds that lock shared and reads the erasure barrier afterwards, so a
/// write in flight is committed and removed here, and one that comes later is refused. The
/// database lets the runtime delete these rows only while the billing erasure barrier exists.
/// Ordinary taskr workspaces, tasks and login policy are not touched.
pub async fn erase_enrolled_sample_resources(conn: &mut sqlx::PgConnection, account: billing_core::types::AccountId) -> Result<EnrolledSampleErased, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(taskr_billing.sample_lock_key($1))").bind(account.0).execute(&mut *conn).await?;
    let sample_work = sqlx::query("DELETE FROM taskr_billing.sample_work WHERE account_id = $1").bind(account.0).execute(&mut *conn).await?.rows_affected();
    let customers = sqlx::query("DELETE FROM taskr_billing.customers WHERE account_id = $1").bind(account.0).execute(&mut *conn).await?.rows_affected();
    Ok(EnrolledSampleErased { customers, sample_work })
}
