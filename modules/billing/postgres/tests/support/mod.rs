//! Shared helpers for the Postgres integration tests.
//!
//! These tests need a disposable database named by `BILLING_TEST_DATABASE_URL`
//! and are `#[ignore]`d for ordinary offline runs. Run explicitly with
//! `--include-ignored`; a missing variable then fails the test, it never skips.
//! Every test uses random account ids and never truncates shared tables.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use billing_core::app::{Audience, Billing, BillingConfig, FreemiumPolicy};
use billing_core::calendar::utc;
use billing_core::decimal::Decimal;
use billing_core::freemium::FreemiumLists;
use billing_core::grants::PackageSpec;
use billing_core::notify::NotifyConfig;
use billing_core::ports::{
    CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart, CheckoutStarted, Customer, CustomerDirectory,
    DirectoryError, GatewayError, MailError, MailMessage, Mailer,
};
use billing_core::pricing::{Catalog, PaygEntry, PlanEntry, PlanInterval};
pub use billing_core::types::Clock;
use billing_core::types::{AccountId, BoxFuture, Micros};
use billing_postgres::{PgBillingStore, connect_owner, connect_runtime, migrate, options_for_login, provision_runtime_login};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};
use tokio::sync::OnceCell;
use uuid::Uuid;

pub const RUNTIME_LOGIN: &str = "billing_test_runtime";
pub const RUNTIME_PASSWORD: &str = "billing-test-runtime-password";

pub fn database_url() -> String {
    std::env::var("BILLING_TEST_DATABASE_URL")
        .expect("BILLING_TEST_DATABASE_URL must name a disposable PostgreSQL database for these tests")
}

static PROVISIONED: OnceCell<()> = OnceCell::const_new();

/// Role attributes and grants are shared by every connection, so a test that
/// briefly contaminates the real `billing_runtime` group holds this advisory
/// lock exclusively; provisioning and every runtime connection hold it shared.
/// Advisory locks are per database: other databases on the cluster are not covered.
pub const ROLE_FIXTURE_LOCK: i64 = 0x6269_6c6c_696e_6701;

pub async fn with_role_fixture_shared<T>(owner: &PgPool, work: impl Future<Output = T>) -> T {
    let mut guard = owner.acquire().await.expect("owner connection for the role fixture lock");
    sqlx::query("SELECT pg_advisory_lock_shared($1)").bind(ROLE_FIXTURE_LOCK).execute(&mut *guard).await.expect("shared lock");
    let result = work.await;
    sqlx::query("SELECT pg_advisory_unlock_shared($1)").bind(ROLE_FIXTURE_LOCK).execute(&mut *guard).await.expect("unlock");
    result
}

/// An owner pool with the schema migrated and the restricted login ready.
pub async fn owner() -> PgPool {
    let owner = connect_owner(&database_url()).await.expect("owner connection");
    PROVISIONED
        .get_or_init(|| async {
            with_role_fixture_shared(&owner, async {
                migrate(&owner).await.expect("billing migration");
                provision_runtime_login(&owner, RUNTIME_LOGIN, RUNTIME_PASSWORD).await.expect("runtime login");
            })
            .await;
            // A minimal host: its own schema and a jobs table the billing
            // module never queries. The runtime login acts as the host here.
            for statement in [
                "CREATE SCHEMA IF NOT EXISTS billing_test_host",
                "CREATE TABLE IF NOT EXISTS billing_test_host.jobs (
                   id bigint PRIMARY KEY,
                   account_id uuid NOT NULL,
                   status text NOT NULL,
                   duration integer)",
                "GRANT USAGE ON SCHEMA billing_test_host TO billing_runtime",
                "GRANT SELECT, INSERT, UPDATE, DELETE ON billing_test_host.jobs TO billing_runtime",
            ] {
                sqlx::query(statement).execute(&owner).await.expect("host fixture");
            }
        })
        .await;
    owner
}

/// A pool on the restricted `billing_runtime` login, the authority every
/// server really runs with. `connect_runtime` verifies the role and refuses
/// anything stronger, so a test can never pass on owner rights by accident.
/// Provisioning (migration, login) happens through `owner()` first.
pub async fn runtime() -> PgPool {
    let owner = owner().await;
    let options = options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).expect("runtime login options");
    let pool = with_role_fixture_shared(&owner, connect_runtime(options, 5)).await;
    owner.close().await;
    pool.expect("the restricted runtime connection")
}

pub fn account() -> AccountId {
    AccountId(Uuid::new_v4())
}

pub struct TestClock(pub AtomicI64);

impl TestClock {
    pub fn at(at: Micros) -> Arc<TestClock> {
        Arc::new(TestClock(AtomicI64::new(at.0)))
    }

    pub fn set(&self, at: Micros) {
        self.0.store(at.0, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Micros {
        Micros(self.0.load(Ordering::SeqCst))
    }
}

/// An in-memory customer directory. Registering keeps the original creation
/// instant, like the real host directory must.
#[derive(Default)]
pub struct MemoryDirectory(pub Mutex<Vec<Customer>>);

impl MemoryDirectory {
    pub fn add(&self, customer: Customer) {
        self.0.lock().unwrap().push(customer);
    }
}

impl CustomerDirectory for MemoryDirectory {
    fn by_account(&self, account: AccountId) -> BoxFuture<'_, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().iter().find(|c| c.account == account).cloned()) })
    }

    fn by_email<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move {
            Ok(self.0.lock().unwrap().iter().find(|c| c.email.eq_ignore_ascii_case(email)).cloned())
        })
    }

    fn register<'a>(&'a self, customer: &'a Customer) -> BoxFuture<'a, Result<Customer, DirectoryError>> {
        Box::pin(async move {
            let mut all = self.0.lock().unwrap();
            if let Some(existing) = all.iter().find(|c| c.account == customer.account) {
                return Ok(existing.clone());
            }
            all.push(customer.clone());
            Ok(customer.clone())
        })
    }
}

pub fn customer(account: AccountId, email: &str, created_at: Micros) -> Customer {
    Customer { account, email: email.into(), full_name: Some("Ada Lovelace".into()), created_at }
}

fn d(text: &str) -> Decimal {
    text.parse().unwrap()
}

pub fn catalog() -> Catalog {
    let spec = |name: &str, minutes: i64, count: u32| PackageSpec {
        name: name.into(),
        minutes,
        count,
        interval: "month".into(),
        frequency: 1,
        expiry: 1,
    };
    Catalog {
        plans: vec![
            PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Monthly,
                spec: spec("PREMIUM", 1200, 1),
                usd_price: d("14"),
                struck_price: d("40"),
                plan_code_try: Some("plan-try-premium-monthly".into()),
                plan_code_usd: Some("plan-usd-premium-monthly".into()),
            },
            PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Yearly,
                spec: spec("PREMIUM", 1200, 12),
                usd_price: d("120"),
                struck_price: d("30"),
                plan_code_try: Some("plan-try-premium-yearly".into()),
                plan_code_usd: Some("plan-usd-premium-yearly".into()),
            },
        ],
        payg: Some(PaygEntry {
            stored_name: "Pay-as-you-Go".into(),
            catalog_label: "PAY AS YOU GO".into(),
            usd_per_hour: d("1.49"),
            struck_per_hour: d("2"),
            min_hours: 5,
            max_hours: 9999,
            minutes_per_hour: 60,
        }),
        locale_discount_locales: ["id".to_string(), "pt".to_string()].into_iter().collect(),
        special_offer_try_per_hour: Some(d("14.99")),
    }
}

pub fn config() -> BillingConfig {
    BillingConfig {
        freemium: FreemiumPolicy { name: "freemium".into(), minutes: 20, months: 2 },
        catalog: catalog(),
        rate: d("44"),
        lists: FreemiumLists::new(&["tr", "de"], &["turkish", "german"]),
        audience: Audience::default(),
        special_client_offer: None,
        notify: NotifyConfig {
            brand: "Transgate".into(),
            sales_email: "sales@example.test".into(),
            support_email: "support@example.test".into(),
        },
        public_base_url: "https://billing.example.test".into(),
    }
}

/// A gateway whose answers are scripted per test. It is a deliberate test
/// adapter; nothing selects it implicitly.
pub struct ScriptedGateway {
    pub name: &'static str,
    pub environment: &'static str,
    pub starts: Mutex<VecDeque<Result<CheckoutStarted, GatewayError>>>,
    pub results: Mutex<VecDeque<Result<CheckoutResult, GatewayError>>>,
    pub started: Mutex<Vec<CheckoutStart>>,
    pub result_calls: Mutex<Vec<(String, CheckoutKind)>>,
}

impl ScriptedGateway {
    pub fn new(name: &'static str, environment: &'static str) -> Arc<ScriptedGateway> {
        Arc::new(ScriptedGateway {
            name,
            environment,
            starts: Mutex::default(),
            results: Mutex::default(),
            started: Mutex::default(),
            result_calls: Mutex::default(),
        })
    }

    pub fn will_start(&self, outcome: Result<CheckoutStarted, GatewayError>) {
        self.starts.lock().unwrap().push_back(outcome);
    }

    pub fn will_answer(&self, outcome: Result<CheckoutResult, GatewayError>) {
        self.results.lock().unwrap().push_back(outcome);
    }
}

pub fn started(token: &str) -> CheckoutStarted {
    CheckoutStarted { token: token.into(), form_content: Some("<script>form</script>".into()), url: None }
}

pub fn success(customer: &str, subscription: Option<&str>) -> CheckoutResult {
    CheckoutResult::Success {
        customer_ref: Some(customer.into()),
        subscription_ref: subscription.map(str::to_string),
        observed_amount: None,
        observed_currency: None,
        provider_payment_id: Some("pay-1".into()),
    }
}

impl CheckoutGateway for ScriptedGateway {
    fn gateway(&self) -> &str {
        self.name
    }

    fn environment(&self) -> &str {
        self.environment
    }

    fn start<'a>(&'a self, request: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutStarted, GatewayError>> {
        Box::pin(async move {
            self.started.lock().unwrap().push(request.clone());
            self.starts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(GatewayError::Uncertain("no scripted start".into())))
        })
    }

    fn result<'a>(&'a self, token: &'a str, kind: CheckoutKind) -> BoxFuture<'a, Result<CheckoutResult, GatewayError>> {
        Box::pin(async move {
            self.result_calls.lock().unwrap().push((token.to_string(), kind));
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(GatewayError::Uncertain("no scripted result".into())))
        })
    }
}

#[derive(Default)]
pub struct RecordingMailer {
    pub sent: Mutex<Vec<(MailMessage, String)>>,
    /// Targets (by recipient address) that fail until cleared.
    pub failing: Mutex<Vec<String>>,
}

impl Mailer for RecordingMailer {
    fn send<'a>(&'a self, message: &'a MailMessage, idempotency_key: &'a str) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move {
            if self.failing.lock().unwrap().iter().any(|to| to == &message.to) {
                return Err(MailError(format!("{} unreachable", message.to)));
            }
            self.sent.lock().unwrap().push((message.clone(), idempotency_key.to_string()));
            Ok(())
        })
    }
}

/// A database of one test's own, created from the shared test server and
/// dropped when this guard goes, whether the test passed or panicked. Tests
/// that consume the outbox use it: the relay contract is to claim the whole
/// outbox, and in the shared database that would also claim, lease and back
/// off other tests' events on this test's fixed clock.
pub struct IsolatedDatabase {
    pub name: String,
}

impl Drop for IsolatedDatabase {
    fn drop(&mut self) {
        // Drop cannot await, and the test's own runtime may be the thread that is
        // dropping this, so the cleanup runs on a short-lived runtime of its own.
        let (name, url) = (self.name.clone(), database_url());
        let _ = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            runtime.block_on(async move {
                if let Ok(owner) = connect_owner(&url).await {
                    // `name` is generated below from [a-z0-9_] only.
                    let statement = format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)");
                    let _ = sqlx::query(AssertSqlSafe(statement)).execute(&owner).await;
                    owner.close().await;
                }
            });
        })
        .join();
    }
}

/// A runtime pool on a fresh, migrated database of its own. The roles are
/// cluster-wide and already provisioned by `owner()`; the URL is passed
/// explicitly, nothing in the process environment changes.
pub async fn isolated_runtime() -> (PgPool, IsolatedDatabase) {
    let base = owner().await;
    let name = format!("billing_t_{}", &Uuid::new_v4().simple().to_string()[..16]);
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
        .execute(&base)
        .await
        .expect("create an isolated test database");
    let guard = IsolatedDatabase { name: name.clone() };
    let url = database_url();
    let owner_options = PgConnectOptions::from_str(&url).expect("database url").database(&name);
    let isolated_owner = PgPoolOptions::new().max_connections(2).connect_with(owner_options).await.expect("isolated owner");
    migrate(&isolated_owner).await.expect("billing migration in the isolated database");
    isolated_owner.close().await;
    let options = options_for_login(&url, RUNTIME_LOGIN, RUNTIME_PASSWORD).expect("runtime login options").database(&name);
    let pool = with_role_fixture_shared(&base, connect_runtime(options, 5)).await;
    base.close().await;
    (pool.expect("the restricted runtime connection to the isolated database"), guard)
}

pub struct Fixture {
    pub billing: Billing,
    pub store: Arc<PgBillingStore>,
    pub clock: Arc<TestClock>,
    pub directory: Arc<MemoryDirectory>,
    pub pool: PgPool,
    /// Set for fixtures on a database of their own; dropping it drops the database.
    pub isolated: Option<IsolatedDatabase>,
}

fn assemble(
    pool: PgPool,
    isolated: Option<IsolatedDatabase>,
    gateways: Vec<Arc<ScriptedGateway>>,
    config: BillingConfig,
    configure: impl FnOnce(Billing) -> Billing,
) -> Fixture {
    let store = Arc::new(PgBillingStore::new(pool.clone()));
    let clock = TestClock::at(utc(2026, 1, 15, 12, 0, 0, 0));
    let directory = Arc::new(MemoryDirectory::default());
    let mut billing = Billing::new(store.clone(), clock.clone(), directory.clone(), config);
    for gateway in gateways {
        billing = billing.with_checkout(gateway);
    }
    Fixture { billing: configure(billing), store, clock, directory, pool, isolated }
}

/// A `Billing` over the real runtime pool with a scripted Iyzico-like gateway.
pub async fn fixture(gateways: Vec<Arc<ScriptedGateway>>) -> Fixture {
    assemble(runtime().await, None, gateways, config(), |b| b)
}

/// Like `fixture_with`, on a database of its own: for tests that claim or relay
/// the outbox through the real, global `claim_outbox`.
pub async fn isolated_fixture_with(
    gateways: Vec<Arc<ScriptedGateway>>,
    configure: impl FnOnce(Billing) -> Billing,
) -> Fixture {
    let (pool, guard) = isolated_runtime().await;
    assemble(pool, Some(guard), gateways, config(), configure)
}

/// Provisions an account with the default 20 free minutes.
pub async fn provisioned(fixture: &Fixture, email: &str) -> AccountId {
    let id = account();
    let created = fixture.clock.now();
    fixture.directory.add(customer(id, email, created));
    fixture.billing.provision(customer(id, email, created)).await.expect("provision");
    id
}

pub fn unique_email() -> String {
    format!("{}@example.test", Uuid::new_v4().simple())
}

/// A provider environment of its own for one test. The environment is part of
/// every provider identity (payment token, order reference, subscription
/// reference, held notification), so fixed tokens inside one test never meet
/// another test's rows or an earlier run's, while production uniqueness stays
/// exactly as it is. Leaked on purpose: the scripted adapters hold `&'static str`.
pub fn test_env(base: &str) -> &'static str {
    Box::leak(format!("{base}-{}", &Uuid::new_v4().simple().to_string()[..12]).into_boxed_str())
}

// ---- scripted webhooks and subscription API -------------------------------

use billing_core::ports::{
    Headers, OrderNotification, OrderWebhook, SubscriptionDetail, SubscriptionGateway, SubscriptionNotification,
    SubscriptionWebhook, WebhookError,
};

/// Accepts exactly the notification it is given; `reject` models a missing or
/// wrong signature. Real signature verification is proved in billing-iyzico.
pub struct ScriptedSubscriptionWebhook {
    pub environment: &'static str,
    pub next: Mutex<Option<Result<SubscriptionNotification, WebhookError>>>,
}

impl ScriptedSubscriptionWebhook {
    pub fn new(environment: &'static str) -> Arc<ScriptedSubscriptionWebhook> {
        Arc::new(ScriptedSubscriptionWebhook { environment, next: Mutex::new(None) })
    }

    pub fn deliver(&self, notification: SubscriptionNotification) {
        *self.next.lock().unwrap() = Some(Ok(notification));
    }

    pub fn reject(&self) {
        *self.next.lock().unwrap() = Some(Err(WebhookError::Rejected("bad signature".into())));
    }
}

impl SubscriptionWebhook for ScriptedSubscriptionWebhook {
    fn environment(&self) -> &str {
        self.environment
    }

    fn verify(&self, _headers: &Headers, _body: &[u8]) -> Result<SubscriptionNotification, WebhookError> {
        self.next.lock().unwrap().clone().unwrap_or(Err(WebhookError::Rejected("nothing scripted".into())))
    }
}

pub struct ScriptedSubscriptionGateway {
    pub environment: &'static str,
    pub detail: Mutex<Result<SubscriptionDetail, GatewayError>>,
}

impl ScriptedSubscriptionGateway {
    pub fn new(environment: &'static str, detail: Result<SubscriptionDetail, GatewayError>) -> Arc<Self> {
        Arc::new(ScriptedSubscriptionGateway { environment, detail: Mutex::new(detail) })
    }
}

impl SubscriptionGateway for ScriptedSubscriptionGateway {
    fn environment(&self) -> &str {
        self.environment
    }

    fn detail<'a>(&'a self, _subscription_ref: &'a str) -> BoxFuture<'a, Result<SubscriptionDetail, GatewayError>> {
        Box::pin(async move { self.detail.lock().unwrap().clone() })
    }
}

pub struct ScriptedOrderWebhook {
    pub environment: &'static str,
    pub store: &'static str,
    pub variant: &'static str,
    pub next: Mutex<Option<Result<OrderNotification, WebhookError>>>,
}

impl ScriptedOrderWebhook {
    pub fn new(environment: &'static str) -> Arc<ScriptedOrderWebhook> {
        Arc::new(ScriptedOrderWebhook { environment, store: "store-1", variant: "variant-1", next: Mutex::new(None) })
    }

    pub fn deliver(&self, order: OrderNotification) {
        *self.next.lock().unwrap() = Some(Ok(order));
    }
}

impl OrderWebhook for ScriptedOrderWebhook {
    fn environment(&self) -> &str {
        self.environment
    }

    fn expected_store(&self) -> &str {
        self.store
    }

    fn expected_variant(&self) -> &str {
        self.variant
    }

    fn verify<'a>(&'a self, _headers: &'a Headers, _body: &'a [u8]) -> BoxFuture<'a, Result<OrderNotification, WebhookError>> {
        let scripted = self.next.lock().unwrap().clone().unwrap_or(Err(WebhookError::Rejected("nothing scripted".into())));
        Box::pin(std::future::ready(scripted))
    }
}

/// A paid order that matches a pending payment of `cents` for `account`.
pub fn paid_order(payment_id: i64, account: AccountId, order_id: &str, cents: i64) -> OrderNotification {
    OrderNotification {
        event_name: "order_created".into(),
        order_id: order_id.into(),
        status: "paid".into(),
        store_id: "store-1".into(),
        variant_id: "variant-1".into(),
        quantity: 1,
        currency: "USD".into(),
        subtotal_cents: cents,
        discount_cents: 0,
        tax_cents: 0,
        total_cents: cents,
        tax_inclusive: false,
        test_mode: true,
        payment_id: Some(payment_id),
        account_id: Some(account.to_string()),
        receipt_url: Some("https://receipts.example.test/r/1".into()),
        raw: serde_json::json!({}),
    }
}

pub fn job_base() -> i64 {
    (Uuid::new_v4().as_u128() as i64) & 0x0fff_ffff_ffff_ff00
}

pub async fn count(pool: &PgPool, sql: &'static str, account: AccountId) -> i64 {
    sqlx::query_scalar(sql).bind(account.0).fetch_one(pool).await.expect("count query")
}

/// Like `fixture`, but lets a test add webhooks, the subscription API or a mailer.
pub async fn fixture_with(
    gateways: Vec<Arc<ScriptedGateway>>,
    configure: impl FnOnce(Billing) -> Billing,
) -> Fixture {
    assemble(runtime().await, None, gateways, config(), configure)
}

/// Like `fixture_with`, with a custom configuration (audience lists and so on).
pub async fn fixture_config(
    gateways: Vec<Arc<ScriptedGateway>>,
    config: BillingConfig,
    configure: impl FnOnce(Billing) -> Billing,
) -> Fixture {
    assemble(runtime().await, None, gateways, config, configure)
}
