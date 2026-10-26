//! The billing router over real TCP, real Postgres (as the restricted runtime
//! login) and scripted providers. Requests exercise authentication, ownership,
//! admin authorization and provider authentication.
//!
//! Needs `BILLING_TEST_DATABASE_URL`; `#[ignore]`d for offline runs and failing,
//! not skipping, when run without it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::HeaderMap;
use billing_core::app::{Audience, Billing, BillingConfig, FreemiumPolicy};
use billing_core::calendar::add_months;
use billing_core::decimal::Decimal;
use billing_core::freemium::FreemiumLists;
use billing_core::grants::PackageSpec;
use billing_core::notify::NotifyConfig;
use billing_core::ports::{
    CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart, CheckoutStarted, Customer,
    CustomerDirectory, DirectoryError, GatewayError, Headers, OrderNotification, OrderWebhook,
    SubscriptionNotification, SubscriptionWebhook, WebhookError,
};
use billing_core::pricing::{Catalog, PaygEntry, PlanEntry, PlanInterval};
use billing_core::types::{AccountId, BoxFuture, Clock, Micros};
use billing_http::{
    AuthError, BillingHttp, BillingIdentity, CHECKOUT_FORM_MAX_BYTES, Caller, CheckoutOutcome, CheckoutPage, ConfirmOutcome, ConfirmPage,
    RenderedCheckout, authenticated, provider,
};
use billing_postgres::{PgBillingStore, connect_owner, connect_runtime, migrate, options_for_login, provision_runtime_login};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use uuid::Uuid;

const RUNTIME_LOGIN: &str = "billing_http_test_runtime";
const RUNTIME_PASSWORD: &str = "billing-http-test-password";
const SIGNUP_TOKEN: &str = "signup-token-not-a-credential";
const NOW: i64 = 1_768_478_400_000_000;

fn database_url() -> String {
    std::env::var("BILLING_TEST_DATABASE_URL").expect("BILLING_TEST_DATABASE_URL must name a disposable PostgreSQL database")
}

static READY: OnceCell<()> = OnceCell::const_new();

async fn owner() -> PgPool {
    let owner = connect_owner(&database_url()).await.unwrap();
    READY
        .get_or_init(|| async {
            migrate(&owner).await.unwrap();
            provision_runtime_login(&owner, RUNTIME_LOGIN, RUNTIME_PASSWORD).await.unwrap();
        })
        .await;
    owner
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Micros {
        Micros(NOW)
    }
}

#[derive(Default)]
struct Directory(Mutex<Vec<Customer>>);

impl CustomerDirectory for Directory {
    fn by_account(&self, account: AccountId) -> BoxFuture<'_, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().iter().find(|c| c.account == account).cloned()) })
    }

    fn by_email<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().iter().find(|c| c.email.eq_ignore_ascii_case(email)).cloned()) })
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

struct Scripted {
    name: &'static str,
    environment: &'static str,
    starts: Mutex<Vec<Result<CheckoutStarted, GatewayError>>>,
    results: Mutex<Vec<Result<CheckoutResult, GatewayError>>>,
    callbacks: Mutex<Vec<String>>,
}

impl CheckoutGateway for Scripted {
    fn gateway(&self) -> &str {
        self.name
    }

    fn environment(&self) -> &str {
        self.environment
    }

    fn start<'a>(&'a self, request: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutStarted, GatewayError>> {
        Box::pin(async move {
            self.callbacks.lock().unwrap().push(request.callback_url.clone());
            self.starts.lock().unwrap().pop().unwrap_or(Err(GatewayError::Uncertain("unscripted".into())))
        })
    }

    fn result<'a>(&'a self, _token: &'a str, _kind: CheckoutKind) -> BoxFuture<'a, Result<CheckoutResult, GatewayError>> {
        Box::pin(async move { self.results.lock().unwrap().pop().unwrap_or(Err(GatewayError::Uncertain("unscripted".into()))) })
    }
}

#[derive(Default)]
struct SubHook {
    next: Mutex<Option<SubscriptionNotification>>,
}

impl SubscriptionWebhook for SubHook {
    fn environment(&self) -> &str {
        "sandbox"
    }

    fn verify(&self, _headers: &Headers, _body: &[u8]) -> Result<SubscriptionNotification, WebhookError> {
        self.next.lock().unwrap().clone().ok_or(WebhookError::Rejected("bad signature".into()))
    }
}

#[derive(Default)]
struct OrderHook {
    next: Mutex<Option<OrderNotification>>,
    seen: Mutex<Vec<(Headers, Vec<u8>)>>,
}

impl OrderWebhook for OrderHook {
    fn environment(&self) -> &str {
        "test"
    }

    fn expected_store(&self) -> &str {
        "store-1"
    }

    fn expected_variant(&self) -> &str {
        "variant-1"
    }

    fn verify<'a>(&'a self, headers: &'a Headers, body: &'a [u8]) -> BoxFuture<'a, Result<OrderNotification, WebhookError>> {
        self.seen.lock().unwrap().push((headers.clone(), body.to_vec()));
        let scripted = self.next.lock().unwrap().clone().ok_or(WebhookError::Rejected("bad signature".into()));
        Box::pin(std::future::ready(scripted))
    }
}

/// Bearer tokens registered by the test. Anything else is not a session.
#[derive(Default)]
struct Tokens(Mutex<HashMap<String, Caller>>);

impl BillingIdentity for Tokens {
    fn authenticate<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Caller, AuthError>> {
        Box::pin(async move {
            let token = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or_else(|| AuthError::Unauthenticated("no bearer token".into()))?;
            if token == "identity-down" {
                return Err(AuthError::Unavailable("identity provider unreachable".into()));
            }
            self.0.lock().unwrap().get(token).cloned().ok_or_else(|| AuthError::Unauthenticated("unknown token".into()))
        })
    }
}

struct App {
    base: String,
    http: reqwest::Client,
    owner: PgPool,
    directory: Arc<Directory>,
    tokens: Arc<Tokens>,
    iyzico: Arc<Scripted>,
    lemon: Arc<Scripted>,
    sub_hook: Arc<SubHook>,
    order_hook: Arc<OrderHook>,
}

/// A provider identity that cannot collide with another test or another run in the shared database.
fn unique(label: &str) -> String {
    format!("{label}-{}", Uuid::new_v4().simple())
}

fn d(text: &str) -> Decimal {
    text.parse().unwrap()
}

fn config() -> BillingConfig {
    let spec = |name: &str, count: u32| PackageSpec { name: name.into(), minutes: 1200, count, interval: "month".into(), frequency: 1, expiry: 1 };
    BillingConfig {
        freemium: FreemiumPolicy { name: "freemium".into(), minutes: 20, months: 2 },
        catalog: Catalog {
            plans: vec![PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Monthly,
                spec: spec("PREMIUM", 1),
                usd_price: d("14"),
                struck_price: d("40"),
                plan_code_try: None,
                plan_code_usd: Some("plan-usd".into()),
            }],
            payg: Some(PaygEntry {
                stored_name: "Pay-as-you-Go".into(),
                catalog_label: "PAY AS YOU GO".into(),
                usd_per_hour: d("1.49"),
                struck_per_hour: d("2"),
                min_hours: 5,
                max_hours: 9999,
                minutes_per_hour: 60,
            }),
            // DEF-010: `id` shows a different total than it is charged.
            locale_discount_locales: ["id".to_string()].into_iter().collect(),
            special_offer_try_per_hour: None,
        },
        rate: d("44"),
        lists: FreemiumLists::new(&["tr"], &["turkish"]),
        audience: Audience::default(),
        special_client_offer: None,
        notify: NotifyConfig { brand: "Transgate".into(), sales_email: "sales@example.test".into(), support_email: "support@example.test".into() },
        public_base_url: "https://billing.example.test".into(),
    }
}

async fn app() -> App {
    app_with(None).await
}

async fn app_with(page: Option<Arc<dyn ConfirmPage>>) -> App {
    app_full(page, None).await
}

const SITE: &str = "http://127.0.0.1";

async fn app_full(page: Option<Arc<dyn ConfirmPage>>, checkout: Option<Arc<dyn CheckoutPage>>) -> App {
    let owner = owner().await;
    let options = options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap();
    let pool = connect_runtime(options, 6).await.expect("the restricted runtime role");
    let directory = Arc::new(Directory::default());
    let iyzico = Arc::new(Scripted { name: "iyzico", environment: "sandbox", starts: Default::default(), results: Default::default(), callbacks: Default::default() });
    let lemon = Arc::new(Scripted { name: "lemonsqueezy", environment: "test", starts: Default::default(), results: Default::default(), callbacks: Default::default() });
    let sub_hook = Arc::new(SubHook::default());
    let order_hook = Arc::new(OrderHook::default());
    let billing = Billing::new(Arc::new(PgBillingStore::new(pool)), Arc::new(FixedClock), directory.clone(), config())
        .with_checkout(iyzico.clone())
        .with_checkout(lemon.clone())
        .with_subscription_webhook(sub_hook.clone())
        .with_order_webhook(order_hook.clone());
    let tokens = Arc::new(Tokens::default());
    let mut http_state = BillingHttp::new(Arc::new(billing), tokens.clone()).with_signup_token(SIGNUP_TOKEN);
    if let Some(page) = page {
        http_state = http_state.with_confirm_page(page);
    }
    if let Some(checkout) = checkout {
        http_state = http_state.with_checkout_page(checkout, SITE).unwrap();
    }
    let state = Arc::new(http_state);
    let router: Router = Router::new().nest("/api", authenticated(state.clone())).merge(provider(state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    App {
        base: format!("http://{address}"),
        http: reqwest::Client::new(),
        owner,
        directory,
        tokens,
        iyzico,
        lemon,
        sub_hook,
        order_hook,
    }
}

struct Person {
    account: AccountId,
    token: String,
    email: String,
}

impl App {
    /// A signed-up customer (directory record at NOW) with a session token.
    fn person(&self, admin: bool) -> Person {
        self.person_created(admin, Micros(NOW))
    }

    /// A customer the identity system created at `created_at`.
    fn person_created(&self, admin: bool, created_at: Micros) -> Person {
        let account = AccountId(Uuid::new_v4());
        let email = format!("{}@example.test", Uuid::new_v4().simple());
        let token = format!("token-{}", Uuid::new_v4().simple());
        self.directory.0.lock().unwrap().push(Customer { account, email: email.clone(), full_name: Some("Ada Lovelace".into()), created_at });
        self.tokens.0.lock().unwrap().insert(token.clone(), Caller { account, email: email.clone(), full_name: None, is_admin: admin });
        Person { account, token, email }
    }

    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
        let mut request = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body.to_string());
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    async fn count(&self, sql: &'static str, account: AccountId) -> i64 {
        sqlx::query_scalar(sql).bind(account.0).fetch_one(&self.owner).await.unwrap()
    }

    async fn count_token(&self, account: AccountId, token: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM billing.payments WHERE account_id = $1 AND token = $2")
            .bind(account.0)
            .bind(token)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn count_pending_token(&self, account: AccountId, token: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM billing.payments WHERE account_id = $1 AND token = $2 AND status = 'pending'")
            .bind(account.0)
            .bind(token)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_068_every_protected_route_refuses_missing_and_invalid_sessions_and_checkout_is_post_only() {
    let app = app().await;
    let routes = [
        ("GET", "/api/billing/overview"),
        ("GET", "/api/billing/history"),
        ("GET", "/api/billing/payments/1/status"),
        ("POST", "/api/billing/quote"),
        ("GET", "/api/billing/coupon/X"),
        ("POST", "/api/billing/checkout"),
        ("POST", "/api/billing/cancel-subscription"),
        ("POST", "/api/billing/admin/customer-invoices"),
        ("POST", "/api/billing/admin/payments/1/reconcile"),
        ("GET", "/api/billing/admin/unresolved"),
        ("POST", "/api/billing/admin/unresolved/1/replay"),
        ("POST", "/api/billing/admin/subscriptions/first-order"),
    ];
    for (method, path) in routes {
        let body = (method == "POST").then(|| json!({}));
        let (status, body_json) = app.call(method, path, None, body.clone()).await;
        assert_eq!(status, 401, "{method} {path} anonymous");
        assert_eq!(body_json["error"]["message"], "Please sign in and try again.");
        assert_eq!(app.call(method, path, Some("made-up-token"), body.clone()).await.0, 401, "{method} {path} invalid");
        assert_eq!(app.call(method, path, Some("identity-down"), body).await.0, 503, "{method} {path} identity outage");
    }
    let person = app.person(false);
    assert_eq!(app.call("GET", "/api/billing/checkout", Some(&person.token), None).await.0, 405);
    assert_eq!(app.call("GET", "/api/billing/cancel-subscription", Some(&person.token), None).await.0, 405);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_019_a_client_cannot_name_another_account_claim_admin_or_set_an_amount() {
    let app = app().await;
    let (a, b) = (app.person(false), app.person(false));
    // B has a purchase, so the two balances differ.
    let tok_b = unique("tok-b");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok_b.clone(), form_content: Some("<form/>".into()), url: None }));
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success { customer_ref: Some("c".into()), subscription_ref: None, observed_amount: None, observed_currency: None, provider_payment_id: None }));
    assert_eq!(app.call("POST", "/api/billing/checkout", Some(&b.token), Some(json!({"pay_as_you_go_hours": 5}))).await.0, 200);
    let confirm = app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/x-www-form-urlencoded").body(format!("token={tok_b}")).send().await.unwrap();
    assert_eq!(confirm.status().as_u16(), 200);

    // A asks for B's account in the query, the body and a header: still A's data.
    let url = format!("/api/billing/overview?account_id={}", b.account);
    let (status, overview) = app.call("GET", &url, Some(&a.token), Some(json!({"account_id": b.account.to_string()}))).await;
    assert_eq!(status, 200);
    assert_eq!(overview["balance"], 20, "A's own free minutes, not B's 320");
    let (_, mine) = app.call("GET", "/api/billing/overview", Some(&b.token), None).await;
    assert_eq!(mine["balance"], 320);

    // A checkout that claims another account, an amount and an admin flag is priced and stored for A alone.
    let tok_a = unique("tok-a");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok_a.clone(), form_content: Some("<form/>".into()), url: None }));
    let forged = json!({"pay_as_you_go_hours": 5, "account_id": b.account.to_string(), "amount": "0.01", "is_admin": true, "user_id": b.account.to_string()});
    let (status, view) = app.call("POST", "/api/billing/checkout", Some(&a.token), Some(forged)).await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(view["amount"], "7.45");
    assert_eq!(app.count_token(a.account, &tok_a).await, 1);
    assert_eq!(app.count_token(b.account, &tok_a).await, 0);
    // Admin rights come from the verified identity, not a flag the client sends.
    let (status, _) = app.call("POST", "/api/billing/admin/customer-invoices", Some(&a.token), Some(json!({"email": b.email, "is_admin": true}))).await;
    assert_eq!(status, 403);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_076_admin_lookup_answers_401_403_400_404_and_200_with_the_customers_own_history() {
    let app = app().await;
    let (admin, customer, other) = (app.person(true), app.person(false), app.person(false));
    let tok_c = unique("tok-c");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok_c.clone(), form_content: None, url: None }));
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success { customer_ref: None, subscription_ref: None, observed_amount: None, observed_currency: None, provider_payment_id: None }));
    app.call("POST", "/api/billing/checkout", Some(&customer.token), Some(json!({"pay_as_you_go_hours": 10}))).await;
    app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/json").body(json!({"token": tok_c}).to_string()).send().await.unwrap();

    let lookup = "/api/billing/admin/customer-invoices";
    assert_eq!(app.call("GET", lookup, Some(&admin.token), None).await.0, 405);
    assert_eq!(app.call("POST", lookup, None, Some(json!({"email": customer.email}))).await.0, 401);
    assert_eq!(app.call("POST", lookup, Some(&other.token), Some(json!({"email": customer.email}))).await.0, 403);
    assert_eq!(app.call("POST", lookup, Some(&admin.token), Some(json!({}))).await.0, 400);
    assert_eq!(app.call("POST", lookup, Some(&admin.token), Some(json!({"email": "   "}))).await.0, 400);
    assert_eq!(app.call("POST", lookup, Some(&admin.token), Some(json!({"email": "nobody@example.test"}))).await.0, 404);
    let (status, body) = app.call("POST", lookup, Some(&admin.token), Some(json!({"email": customer.email.to_uppercase()}))).await;
    assert_eq!(status, 200);
    assert_eq!(body["customer"]["email"], customer.email);
    assert_eq!(body["payments"].as_array().unwrap().len(), 1);
    assert_eq!(body["history"][0]["amount_text"], "14.90 USD");
    // The other customer's lookup has no such payment.
    let (_, empty) = app.call("POST", lookup, Some(&admin.token), Some(json!({"email": other.email}))).await;
    assert_eq!(empty["payments"].as_array().unwrap().len(), 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_062_cancellation_over_http_is_owner_only_and_validates_its_body() {
    let app = app().await;
    let (owner, other) = (app.person(false), app.person(false));
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: unique("tok-cancel"), form_content: None, url: None }));
    let (_, view) = app.call("POST", "/api/billing/checkout", Some(&owner.token), Some(json!({"package_id": 0}))).await;
    let payment_id = view["payment_id"].as_i64().unwrap();
    let cancel = "/api/billing/cancel-subscription";
    assert_eq!(app.call("POST", cancel, Some(&other.token), Some(json!({"payment_id": payment_id}))).await.0, 404, "another account's payment is indistinguishable from none");
    assert_eq!(app.call("POST", cancel, Some(&owner.token), Some(json!({}))).await.0, 400);
    assert_eq!(app.call("POST", cancel, Some(&owner.token), Some(json!({"payment_id": "x"}))).await.0, 400);
    let (status, body) = app.call("POST", cancel, Some(&owner.token), Some(json!({"payment_id": payment_id}))).await;
    assert_eq!((status, body["payment_id"].as_i64()), (200, Some(payment_id)));
    assert_eq!(app.count("SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'CancelRequested'", owner.account).await, 1);
    assert_eq!(app.count("SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'CancelRequested'", other.account).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_059_060_the_browser_return_accepts_a_form_or_json_and_a_missing_or_unknown_token_is_the_failure_page() {
    let app = app().await;
    let person = app.person(false);
    // A token with characters a form must encode.
    let hex = Uuid::new_v4().simple().to_string();
    let token = format!("tok+with/odd chars {hex}");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: token.clone(), form_content: Some("<form/>".into()), url: None }));
    app.call("POST", "/api/billing/checkout", Some(&person.token), Some(json!({"pay_as_you_go_hours": 5}))).await;
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success { customer_ref: None, subscription_ref: None, observed_amount: None, observed_currency: None, provider_payment_id: None }));
    let form = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("token=tok%2Bwith%2Fodd+chars+{hex}&other=1"))
        .send()
        .await
        .unwrap();
    assert_eq!(form.status().as_u16(), 200);
    let body: Value = form.json().await.unwrap();
    assert_eq!((body["success"].clone(), body["changed"].clone(), body["redirect_after_seconds"].clone()), (json!(true), json!(true), json!(5)));
    // The same return again is idempotent and changes nothing.
    let again: Value = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/json")
        .body(json!({"token": token}).to_string())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!((again["success"].clone(), again["changed"].clone()), (json!(true), json!(false)));
    for body in ["", "other=1", "token="] {
        let r = app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/x-www-form-urlencoded").body(body.to_string()).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 404, "{body:?}");
    }
    let unknown = app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/json").body(json!({"token": "nope"}).to_string()).send().await.unwrap();
    assert_eq!(unknown.status().as_u16(), 404);
    // An uncertain provider answer is reported as uncertainty, not as a failed payment.
    let tok_u = unique("tok-uncertain");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok_u.clone(), form_content: None, url: None }));
    app.call("POST", "/api/billing/checkout", Some(&person.token), Some(json!({"pay_as_you_go_hours": 5}))).await;
    let uncertain = app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/json").body(json!({"token": tok_u}).to_string()).send().await.unwrap();
    assert_eq!(uncertain.status().as_u16(), 504);
    assert_eq!(app.count_pending_token(person.account, &tok_u).await, 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_003_provider_routes_authenticate_by_signature_and_pass_raw_bytes_through() {
    let app = app().await;
    let person = app.person(false);
    // Iyzico: nothing scripted means the verifier rejected the signature.
    let rejected = app.http.post(format!("{}/api/subscription-callback", app.base)).body("{}").send().await.unwrap();
    assert_eq!(rejected.status().as_u16(), 401);
    *app.sub_hook.next.lock().unwrap() = Some(SubscriptionNotification {
        outcome: billing_core::ports::SubscriptionOutcome::Success,
        raw_event_type: "subscription.order.success".into(),
        order_ref: format!("ord-{}", Uuid::new_v4().simple()),
        subscription_ref: format!("sub-{}", Uuid::new_v4().simple()),
        customer_ref: "cust".into(),
    });
    let held: Value = app.http.post(format!("{}/api/subscription-callback", app.base)).body("{}").send().await.unwrap().json().await.unwrap();
    assert_eq!(held["Held"]["reason"], "subscription_unbound", "verified but unbound: durable, acknowledged, no grant");

    // LemonSqueezy: an unsigned or wrongly signed request is refused and stores nothing.
    let refused = app.http.post(format!("{}/api/lemonsqueezy-webhook", app.base)).body("{}").send().await.unwrap();
    assert_eq!(refused.status().as_u16(), 401);
    // A paid order for a real pending payment completes it, and the verifier saw the raw bytes and headers.
    app.lemon.starts.lock().unwrap().push(Ok(CheckoutStarted { token: unique("co"), form_content: None, url: Some("https://pay.example.test/c".into()) }));
    let (_, view) = app.call("POST", "/api/billing/checkout", Some(&person.token), Some(json!({"pay_as_you_go_hours": 10, "gateway": "lemonsqueezy"}))).await;
    let payment_id = view["payment_id"].as_i64().unwrap();
    *app.order_hook.next.lock().unwrap() = Some(OrderNotification {
        event_name: "order_created".into(),
        order_id: format!("order-{}", Uuid::new_v4().simple()),
        status: "paid".into(),
        store_id: "store-1".into(),
        variant_id: "variant-1".into(),
        quantity: 1,
        currency: "USD".into(),
        subtotal_cents: 1490,
        discount_cents: 0,
        tax_cents: 0,
        total_cents: 1490,
        tax_inclusive: false,
        test_mode: true,
        payment_id: Some(payment_id),
        account_id: Some(person.account.to_string()),
        receipt_url: None,
        raw: Value::Null,
    });
    let raw = "{\"exact\":  \"bytes\"}\n";
    let ok = app.http.post(format!("{}/api/lemonsqueezy-webhook", app.base)).header("X-Signature", "abc123").body(raw).send().await.unwrap();
    assert_eq!(ok.status().as_u16(), 200);
    let (headers, body) = app.order_hook.seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(body, raw.as_bytes());
    assert!(headers.iter().any(|(k, v)| k == "x-signature" && v == "abc123"), "header names arrive lower-cased");
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&person.token), None).await;
    assert_eq!(overview["balance"], 20 + 600);
    // The same delivery again is acknowledged and changes nothing.
    let again = app.http.post(format!("{}/api/lemonsqueezy-webhook", app.base)).body(raw).send().await.unwrap();
    assert_eq!(again.status().as_u16(), 200);
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&person.token), None).await;
    assert_eq!(overview["balance"], 20 + 600);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_001_signup_provisioning_is_service_authenticated_and_anchored_to_the_reported_instant() {
    let app = app().await;
    let person = app.person(false);
    let account = AccountId(Uuid::new_v4());
    let four_months_ago = NOW - 120 * 86_400 * 1_000_000;
    let body = |created: i64| json!({"account_id": account.to_string(), "email": format!("{}@example.test", Uuid::new_v4().simple()), "created_at_micros": created});

    // A user session is not the signup service.
    assert_eq!(app.call("POST", "/api/billing/internal/signup", Some(&person.token), Some(body(NOW))).await.0, 401);
    assert_eq!(app.call("POST", "/api/billing/internal/signup", None, Some(body(NOW))).await.0, 401);
    assert_eq!(app.call("POST", "/api/billing/internal/signup", Some("wrong"), Some(body(NOW))).await.0, 401);
    // Validation after authentication.
    assert_eq!(app.call("POST", "/api/billing/internal/signup", Some(SIGNUP_TOKEN), Some(json!({"account_id": "nope", "email": "a@b.test", "created_at_micros": NOW}))).await.0, 400);
    assert_eq!(app.call("POST", "/api/billing/internal/signup", Some(SIGNUP_TOKEN), Some(body(NOW + 30 * 86_400 * 1_000_000))).await.0, 400, "a future signup would pre-grant a trial");
    let (status, granted) = app.call("POST", "/api/billing/internal/signup", Some(SIGNUP_TOKEN), Some(body(four_months_ago))).await;
    assert_eq!((status, granted["granted"].clone()), (200, json!(true)));
    // Reported again, later or earlier, it grants nothing and never moves the anchor.
    let (_, repeat) = app.call("POST", "/api/billing/internal/signup", Some(SIGNUP_TOKEN), Some(body(NOW))).await;
    assert_eq!(repeat["granted"], false);
    let (start, end): (i64, i64) = sqlx::query_as("SELECT billing.ts_to_us(start_at), billing.ts_to_us(end_at) FROM billing.packages WHERE account_id = $1")
        .bind(account.0)
        .fetch_one(&app.owner)
        .await
        .unwrap();
    assert_eq!(start, four_months_ago);
    assert_eq!(end, add_months(Micros(four_months_ago), 2).unwrap().0, "two calendar months after signup, already in the past");
    assert!(end < NOW);
    assert_eq!(app.count("SELECT count(*) FROM billing.packages WHERE account_id = $1", account).await, 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn readiness_names_what_is_configured_and_quotes_validate_hours() {
    let app = app().await;
    let (status, body) = app.call("GET", "/api/billing/ready", None, None).await;
    assert_eq!(status, 200);
    let configured: Vec<String> = serde_json::from_value(body["configured"].clone()).unwrap();
    assert!(configured.contains(&"checkout:iyzico".to_string()) && configured.contains(&"webhook:iyzico".to_string()));
    assert!(!configured.contains(&"mail".to_string()), "mail was not configured and is not claimed");

    let person = app.person(false);
    let (status, quote) = app.call("POST", "/api/billing/quote", Some(&person.token), Some(json!({"pay_as_you_go_hours": 10, "locale": "en"}))).await;
    assert_eq!((status, quote["amount"].clone(), quote["currency"].clone()), (200, json!("14.90"), json!("USD")));
    let (status, bad) = app.call("POST", "/api/billing/quote", Some(&person.token), Some(json!({"pay_as_you_go_hours": 4}))).await;
    assert_eq!((status, bad["error"]["code"].clone()), (400, json!("invalid_order")));
    let (status, _) = app.call("POST", "/api/billing/quote", Some(&person.token), Some(json!({"pay_as_you_go_hours": 7.5}))).await;
    assert_eq!(status, 400, "whole hours only");
    // An account with no trusted signup record is refused, never given a trial.
    let stranger = AccountId(Uuid::new_v4());
    app.tokens.0.lock().unwrap().insert("stranger".into(), Caller { account: stranger, email: "s@example.test".into(), full_name: None, is_admin: false });
    assert_eq!(app.call("GET", "/api/billing/overview", Some("stranger"), None).await.0, 404);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn cpn_012_the_coupon_route_reports_the_screen_messages() {
    let app = app().await;
    let person = app.person(false);
    let tag = Uuid::new_v4().simple().to_string();
    for (id, expires) in [(format!("GOOD-{tag}"), "2099-01-01"), (format!("OLD-{tag}"), "2020-01-01")] {
        sqlx::query("INSERT INTO billing.coupons (id, amount, filter, expires_at) VALUES ($1, 0.25::real, '{\"product\":\"Pay-as-you-Go\"}'::jsonb, $2::text::date)")
            .bind(id)
            .bind(expires)
            .execute(&app.owner)
            .await
            .unwrap();
    }
    let (_, unknown) = app.call("GET", &format!("/api/billing/coupon/MISSING-{tag}"), Some(&person.token), None).await;
    assert_eq!((unknown["valid"].clone(), unknown["message"].clone()), (json!(false), json!("Coupon is not valid")));
    let (_, expired) = app.call("GET", &format!("/api/billing/coupon/OLD-{tag}"), Some(&person.token), None).await;
    assert_eq!((expired["valid"].clone(), expired["message"].clone()), (json!(false), json!("Coupon has expired")));
    let (_, good) = app.call("GET", &format!("/api/billing/coupon/GOOD-{tag}"), Some(&person.token), None).await;
    assert_eq!((good["valid"].clone(), good["coupon"]["amount"].clone()), (json!(true), json!("0.25")));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn lemonsqueezy_returns_to_the_payment_status_route_which_is_owner_scoped_pending_until_the_webhook_and_never_grants() {
    let app = app().await;
    let (owner, stranger) = (app.person(false), app.person(false));
    app.lemon.starts.lock().unwrap().push(Ok(CheckoutStarted { token: unique("ls-co"), form_content: None, url: Some("https://pay.example.test/c".into()) }));
    let (status, view) = app
        .call("POST", "/api/billing/checkout", Some(&owner.token), Some(json!({"pay_as_you_go_hours": 10, "gateway": "lemonsqueezy"})))
        .await;
    assert_eq!(status, 200, "{view}");
    let payment_id = view["payment_id"].as_i64().unwrap();
    // The provider is given a return URL that carries our payment id, not the Iyzico confirm route.
    assert_eq!(
        app.lemon.callbacks.lock().unwrap().last().unwrap(),
        &format!("https://billing.example.test/api/billing/payments/{payment_id}/status")
    );
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: "iy-co".into(), form_content: Some("<form/>".into()), url: None }));
    app.call("POST", "/api/billing/checkout", Some(&owner.token), Some(json!({"pay_as_you_go_hours": 5, "gateway": "iyzico"}))).await;
    assert_eq!(app.iyzico.callbacks.lock().unwrap().last().unwrap(), "https://billing.example.test/checkout/confirm");

    // The buyer returns before the webhook: pending, and visiting it grants nothing.
    let path = format!("/api/billing/payments/{payment_id}/status");
    let (status, pending) = app.call("GET", &path, Some(&owner.token), None).await;
    assert_eq!((status, pending["status"].clone(), pending["payment_id"].clone()), (200, json!("pending"), json!(payment_id)));
    let (_, before) = app.call("GET", "/api/billing/overview", Some(&owner.token), None).await;
    assert_eq!(before["balance"], 20);

    // Someone else's payment looks like a missing one; so does an unknown id; no session is refused.
    assert_eq!(app.call("GET", &path, Some(&stranger.token), None).await.0, 404);
    assert_eq!(app.call("GET", "/api/billing/payments/9223372036854775000/status", Some(&owner.token), None).await.0, 404);
    assert_eq!(app.call("GET", &path, None, None).await.0, 401);

    // The signed order completes it; the same route now says so.
    *app.order_hook.next.lock().unwrap() = Some(OrderNotification {
        event_name: "order_created".into(),
        order_id: format!("order-{}", Uuid::new_v4().simple()),
        status: "paid".into(),
        store_id: "store-1".into(),
        variant_id: "variant-1".into(),
        quantity: 1,
        currency: "USD".into(),
        subtotal_cents: 1490,
        discount_cents: 0,
        tax_cents: 0,
        total_cents: 1490,
        tax_inclusive: false,
        test_mode: true,
        payment_id: Some(payment_id),
        account_id: Some(owner.account.to_string()),
        receipt_url: None,
        raw: Value::Null,
    });
    let ok = app.http.post(format!("{}/api/lemonsqueezy-webhook", app.base)).body("{}").send().await.unwrap();
    assert_eq!(ok.status().as_u16(), 200);
    let (_, done) = app.call("GET", &path, Some(&owner.token), None).await;
    assert_eq!(done["status"], "completed");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_008_073_an_account_whose_only_package_expired_shows_zero_minutes_and_no_balance_value() {
    let app = app().await;
    // Signed up long enough ago that the 2-month free package has ended.
    let expired = app.person_created(false, Micros(NOW - 120 * 86_400 * 1_000_000));
    let (status, overview) = app.call("GET", "/api/billing/overview", Some(&expired.token), None).await;
    assert_eq!(status, 200, "{overview}");
    assert_eq!(overview["balance"], Value::Null, "no open package: the balance value is absent");
    assert_eq!(overview["balance_text"], "0 min", "and the account is shown as 0 min, not as a missing read");
    assert_eq!(overview["packages"].as_array().unwrap().len(), 1);
    assert!(overview["open_packages"].as_array().unwrap().is_empty());

    // A signup today has its 20 free minutes open.
    let fresh = app.person(false);
    let (_, open) = app.call("GET", "/api/billing/overview", Some(&fresh.token), None).await;
    assert_eq!((open["balance"].clone(), open["balance_text"].clone()), (json!(20), json!("20 min")));
}

impl App {
    async fn open_barrier(&self, account: AccountId) {
        sqlx::query("INSERT INTO billing.erasure_barriers (account_id, started_at) VALUES ($1, now())")
            .bind(account.0)
            .execute(&self.owner)
            .await
            .unwrap();
    }
}

// D9/D10 over HTTP: an account under erasure gets 410 account_erased, before any provider call.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d9_an_account_under_erasure_is_refused_with_410_and_no_provider_call() {
    let app = app().await;
    let (known, unprovisioned) = (app.person(false), app.person(false));
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&known.token), None).await.0, 200);
    app.open_barrier(known.account).await;
    app.open_barrier(unprovisioned.account).await;
    let tok = unique("tok-barred");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: Some("<form/>".into()), url: None }));
    for person in [&known, &unprovisioned] {
        let (status, body) = app.call("GET", "/api/billing/overview", Some(&person.token), None).await;
        assert_eq!((status, body["error"]["code"].clone()), (410, json!("account_erased")), "{body}");
        let (status, body) = app.call("POST", "/api/billing/checkout", Some(&person.token), Some(json!({"package_id": 0}))).await;
        assert_eq!((status, body["error"]["code"].clone()), (410, json!("account_erased")), "{body}");
    }
    assert_eq!(app.iyzico.starts.lock().unwrap().len(), 1, "the scripted form was never requested");
    assert_eq!(app.count("SELECT count(*) FROM billing.accounts WHERE account_id = $1", unprovisioned.account).await, 0);
    assert_eq!(app.count("SELECT count(*) FROM billing.payments WHERE account_id = $1", known.account).await, 0);
}

// D10 over HTTP: only an admin closes an abandoned plan checkout, and only with both
// verified attestations and an evidence reference; a bare "never paid" is refused.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_the_admin_erasure_checkout_closure_is_admin_only_and_requires_verified_attestations() {
    let app = app().await;
    let (admin, customer) = (app.person(true), app.person(false));
    let tok = unique("tok-plan");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: Some("<form/>".into()), url: None }));
    let (status, view) = app.call("POST", "/api/billing/checkout", Some(&customer.token), Some(json!({"package_id": 0}))).await;
    assert_eq!(status, 200, "{view}");
    let path = format!("/api/billing/admin/erasure/checkouts/{}/resolve", view["payment_id"]);
    let verified = json!({
        "checkout_cannot_complete": true,
        "no_renewing_subscription": true,
        "evidence_ref": "IYZ-TICKET-42",
        "rationale": "provider record: token expired, no subscription for the customer",
    });
    assert_eq!(app.call("POST", &path, Some(&customer.token), Some(verified.clone())).await.0, 403);
    // Not under erasure: nothing to close.
    assert_eq!(app.call("POST", &path, Some(&admin.token), Some(verified.clone())).await.0, 409);
    app.open_barrier(customer.account).await;
    let mut one_attestation = verified.clone();
    one_attestation["no_renewing_subscription"] = json!(false);
    for refused in [json!({"rationale": "Iyzico console: form never paid"}), one_attestation] {
        let (status, body) = app.call("POST", &path, Some(&admin.token), Some(refused)).await;
        assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_request")), "{body}");
    }
    assert_eq!(app.count("SELECT count(*) FROM billing.erasure_checkout_resolutions WHERE account_id = $1", customer.account).await, 0);
    let (status, resolved) = app.call("POST", &path, Some(&admin.token), Some(verified)).await;
    assert_eq!(status, 200, "{resolved}");
    assert_eq!((resolved["remaining_unbound"].clone(), resolved["billing_blockers_cleared"].clone()), (json!([]), json!(true)));
    assert_eq!(
        app.count("SELECT count(*) FROM billing.erasure_checkout_resolutions WHERE account_id = $1", customer.account).await,
        1
    );
}

// D10 over HTTP: the browser return of a plan checkout during erasure answers 410 and
// the provider's subscription is durably kept as a termination target, with no grant.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_late_browser_return_during_erasure_answers_410_and_keeps_the_target() {
    let app = app().await;
    let customer = app.person(false);
    let tok = unique("tok-late");
    let sub = unique("sub-late");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: Some("<form/>".into()), url: None }));
    let (status, _) = app.call("POST", "/api/billing/checkout", Some(&customer.token), Some(json!({"package_id": 0}))).await;
    assert_eq!(status, 200);
    app.open_barrier(customer.account).await;
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success {
        customer_ref: Some("cust-late".into()),
        subscription_ref: Some(sub.clone()),
        observed_amount: None,
        observed_currency: None,
        provider_payment_id: None,
    }));
    let confirm = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/json")
        .body(json!({"token": tok}).to_string())
        .send()
        .await
        .unwrap();
    let status = confirm.status().as_u16();
    let body: Value = serde_json::from_str(&confirm.text().await.unwrap()).unwrap();
    assert_eq!((status, body["error"]["code"].clone()), (410, json!("account_erased")), "{body}");
    assert_eq!(
        app.count("SELECT count(*) FROM billing.erasure_terminations WHERE account_id = $1 AND status = 'pending'", customer.account).await,
        1
    );
    assert_eq!(app.count("SELECT count(*) FROM billing.packages WHERE account_id = $1 AND payment_id IS NOT NULL", customer.account).await, 0);
    assert_eq!(app.count("SELECT count(*) FROM billing.subscriptions WHERE account_id = $1", customer.account).await, 0);
}

/// Renders the outcome as embedded JSON and records every render.
#[derive(Default)]
struct RecordingPage(Mutex<Vec<Value>>);

impl ConfirmPage for RecordingPage {
    fn render(&self, outcome: &ConfirmOutcome) -> String {
        let value = serde_json::to_value(outcome).unwrap();
        self.0.lock().unwrap().push(value.clone());
        format!("<!doctype html><script type=\"application/json\" id=\"tg-confirm\">{value}</script>")
    }
}

// P5 D8, DEF-010: the quote carries the shown total beside the charged amount; neither
// replaces the other.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d8_the_quote_answers_the_shown_total_beside_the_charged_amount() {
    let app = app().await;
    let person = app.person(false);
    let (status, body) = app.call("POST", "/api/billing/quote", Some(&person.token), Some(json!({"pay_as_you_go_hours": 10, "locale": "id"}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!((body["display_amount"].clone(), body["amount"].clone()), (json!("7.40"), json!("7.45")));
    let (_, en) = app.call("POST", "/api/billing/quote", Some(&person.token), Some(json!({"pay_as_you_go_hours": 10}))).await;
    assert_eq!((en["display_amount"].clone(), en["amount"].clone()), (json!("14.90"), json!("14.90")));
    assert_eq!(app.call("POST", "/api/billing/quote", None, Some(json!({"pay_as_you_go_hours": 10}))).await.0, 401);
}

// P5 D7, BIL-075/076: the owner's invoice context carries server-derived facts and the
// Bill To identity; another account sees none of them; the admin lookup carries the same.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d7_the_owner_invoice_context_and_the_admin_lookup_carry_the_same_facts() {
    let app = app().await;
    let (customer, other, admin) = (app.person(false), app.person(false), app.person(true));
    let tok = unique("tok-invoice");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: None, url: None }));
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success { customer_ref: None, subscription_ref: None, observed_amount: None, observed_currency: None, provider_payment_id: None }));
    let (_, view) = app.call("POST", "/api/billing/checkout", Some(&customer.token), Some(json!({"pay_as_you_go_hours": 10}))).await;
    let payment_id = view["payment_id"].as_i64().unwrap();
    app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/json").body(json!({"token": tok}).to_string()).send().await.unwrap();

    assert_eq!(app.call("GET", "/api/billing/invoices", None, None).await.0, 401);
    let (status, own) = app.call("GET", "/api/billing/invoices", Some(&customer.token), None).await;
    assert_eq!(status, 200, "{own}");
    assert_eq!(own["bill_to"]["name"], json!("Ada Lovelace"), "the directory name when no billing profile exists");
    let invoice = &own["invoices"][0];
    assert_eq!(invoice["invoice_id"], json!(format!("TRAN{payment_id:08}")));
    assert_eq!((invoice["subtotal"].clone(), invoice["tax"].clone(), invoice["total"].clone()), (json!("14.90"), json!("0.00"), json!("14.90")));
    assert_eq!((invoice["tax_kind"].clone(), invoice["package_name"].clone()), (json!("None"), json!("Pay-as-you-Go")));
    let (_, theirs) = app.call("GET", "/api/billing/invoices", Some(&other.token), None).await;
    assert_eq!(theirs["invoices"].as_array().unwrap().len(), 0, "an account sees only its own invoices");

    let (_, lookup) = app.call("POST", "/api/billing/admin/customer-invoices", Some(&admin.token), Some(json!({"email": customer.email}))).await;
    assert_eq!(lookup["invoice_context"], own, "the admin sees the same facts the customer does");
}

// P5 D8: a browser form post that accepts HTML gets the host page built from the one
// confirmation result; JSON callers keep JSON; the provider is asked once.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d8_the_browser_confirmation_page_renders_the_single_confirmation_result() {
    let page = Arc::new(RecordingPage::default());
    let app = app_with(Some(page.clone())).await;
    let person = app.person(false);
    let tok = unique("tok-page");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: Some("<form/>".into()), url: None }));
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success { customer_ref: None, subscription_ref: None, observed_amount: None, observed_currency: None, provider_payment_id: None }));
    app.call("POST", "/api/billing/checkout", Some(&person.token), Some(json!({"pay_as_you_go_hours": 5}))).await;

    let html = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "text/html,application/xhtml+xml")
        .body(format!("token={tok}"))
        .send()
        .await
        .unwrap();
    assert_eq!(html.status().as_u16(), 200);
    assert!(html.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert_eq!(html.headers()["cache-control"], "no-store");
    assert!(html.text().await.unwrap().contains("tg-confirm"));
    let rendered = page.0.lock().unwrap().clone();
    assert_eq!(rendered.len(), 1);
    assert_eq!((rendered[0]["outcome"].clone(), rendered[0]["view"]["success"].clone()), (json!("confirmed"), json!(true)));
    assert!(app.iyzico.results.lock().unwrap().is_empty(), "the provider result was read once, by the one confirmation");
    assert_eq!(app.count_token(person.account, &tok).await, 1);

    // A form post without an HTML Accept, and a JSON post that accepts HTML, keep JSON.
    let json_reply = app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/json").header("accept", "text/html").body(json!({"token": tok}).to_string()).send().await.unwrap();
    let again: Value = json_reply.json().await.unwrap();
    assert_eq!((again["success"].clone(), again["changed"].clone()), (json!(true), json!(false)), "repeat confirmation is idempotent");
    let plain_form = app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/x-www-form-urlencoded").body(format!("token={tok}")).send().await.unwrap();
    assert!(plain_form.headers()["content-type"].to_str().unwrap().starts_with("application/json"));
    assert_eq!(page.0.lock().unwrap().len(), 1, "only the HTML request rendered a page");

    // A failure keeps its HTTP status and error inside the page.
    let missing = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "text/html")
        .body("other=1")
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status().as_u16(), 404);
    let failed = page.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!((failed["outcome"].clone(), failed["status"].clone(), failed["code"].clone()), (json!("failed"), json!(404), json!("not_found")));
}

// WEB-BIL-022/023, BIL-062/063: the Subscription tab reads the open plan with its own
// payment and next payment date; cancelling stores the instant and clears the next payment.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_subscription_read_carries_the_plan_payment_and_cancellation() {
    let app = app().await;
    let (customer, other) = (app.person(false), app.person(false));
    let tok = unique("tok-plan");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: None, url: None }));
    app.iyzico.results.lock().unwrap().push(Ok(CheckoutResult::Success { customer_ref: None, subscription_ref: None, observed_amount: None, observed_currency: None, provider_payment_id: None }));
    let (_, view) = app.call("POST", "/api/billing/checkout", Some(&customer.token), Some(json!({"package_id": 0}))).await;
    let payment_id = view["payment_id"].as_i64().unwrap();
    app.http.post(format!("{}/checkout/confirm", app.base)).header("content-type", "application/json").body(json!({"token": tok}).to_string()).send().await.unwrap();

    assert_eq!(app.call("GET", "/api/billing/subscription", None, None).await.0, 401);
    let (status, facts) = app.call("GET", "/api/billing/subscription", Some(&customer.token), None).await;
    assert_eq!(status, 200, "{facts}");
    assert_eq!((facts["package_name"].clone(), facts["payment"]["payment_id"].clone()), (json!("PREMIUM"), json!(payment_id)));
    assert!(facts["payment"]["next_payment_at"].is_number() && facts["payment"]["canceled_at"].is_null());
    assert_eq!(app.call("GET", "/api/billing/subscription", Some(&other.token), None).await.1, Value::Null, "no plan reads as null");

    app.call("POST", "/api/billing/cancel-subscription", Some(&customer.token), Some(json!({"payment_id": payment_id}))).await;
    let (_, after) = app.call("GET", "/api/billing/subscription", Some(&customer.token), None).await;
    assert!(after["payment"]["canceled_at"].is_number() && after["payment"]["next_payment_at"].is_null());
}

/// Records every checkout document it renders; its policy carries a fixed test nonce.
#[derive(Default)]
struct RecordingCheckout(Mutex<Vec<Value>>);

impl CheckoutPage for RecordingCheckout {
    fn render(&self, outcome: &CheckoutOutcome) -> RenderedCheckout {
        let value = serde_json::to_value(outcome).unwrap();
        self.0.lock().unwrap().push(value);
        RenderedCheckout { html: "<!doctype html><title>checkout</title>".into(), content_security_policy: "default-src 'none'; script-src 'nonce-test'".into() }
    }
}

fn order_field(order: Value) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(order.to_string().as_bytes())
}

impl App {
    async fn post_checkout(&self, body: String, origin: Option<&str>, authorization: Option<&str>) -> reqwest::Response {
        let mut request = self.http.post(format!("{}/checkout", self.base)).header("content-type", "application/x-www-form-urlencoded");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(authorization) = authorization {
            request = request.header("authorization", authorization);
        }
        request.body(body).send().await.unwrap()
    }
}

// D17: one hidden same-origin form post starts the checkout exactly once and answers the
// isolated document with the renderer's own policy; a cold GET goes home.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d17_the_checkout_document_post_starts_once_and_carries_its_own_policy() {
    let page = Arc::new(RecordingCheckout::default());
    let app = app_full(None, Some(page.clone())).await;
    let person = app.person(false);
    let no_redirect = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let cold = no_redirect.get(format!("{}/checkout", app.base)).send().await.unwrap();
    assert_eq!((cold.status().as_u16(), cold.headers()["location"].to_str().unwrap()), (303, "/"));

    let tok = unique("tok-doc");
    app.iyzico.starts.lock().unwrap().push(Ok(CheckoutStarted { token: tok.clone(), form_content: Some("<script>iyzi()</script>".into()), url: None }));
    // Unicode stays inside the encoded JSON; the form itself is ASCII.
    let order = order_field(json!({"pay_as_you_go_hours": 5, "locale": "tr", "analytics": {"utm_campaign": "kış"}}));
    let response = app.post_checkout(format!("order={order}&access_token={}", person.token), Some(SITE), None).await;
    assert_eq!(response.status().as_u16(), 200);
    assert!(response.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert_eq!(response.headers()["content-security-policy"], "default-src 'none'; script-src 'nonce-test'");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = response.text().await.unwrap();
    assert!(!body.contains(&person.token), "the token is never echoed");
    let rendered = page.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!((rendered["outcome"].clone(), rendered["content"].clone(), rendered["locale"].clone()), (json!("form"), json!("<script>iyzi()</script>"), json!("tr")));
    assert!(app.iyzico.starts.lock().unwrap().is_empty(), "exactly one provider start");
    assert_eq!(app.count_token(person.account, &tok).await, 1);
    let stored: Value = sqlx::query_scalar("SELECT metadata->'analytics' FROM billing.payments WHERE token = $1").bind(&tok).fetch_one(&app.owner).await.unwrap();
    assert_eq!(stored["utm_campaign"], json!("kış"));

    // A hosted-URL provider answers a 303 to its HTTPS page instead of a document.
    app.lemon.starts.lock().unwrap().push(Ok(CheckoutStarted { token: unique("tok-lemon"), form_content: None, url: Some("https://pay.example.test/c/1".into()) }));
    let order = order_field(json!({"pay_as_you_go_hours": 5, "gateway": "lemonsqueezy"}));
    let lemon = no_redirect
        .post(format!("{}/checkout", app.base))
        .header("content-type", "application/x-www-form-urlencoded")
        .header("origin", SITE)
        .body(format!("order={order}&access_token={}", person.token))
        .send()
        .await
        .unwrap();
    assert_eq!((lemon.status().as_u16(), lemon.headers()["location"].to_str().unwrap()), (303, "https://pay.example.test/c/1"));
}

// D17 refusals: never a provider start, always the failure document with the JSON status.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d17_a_checkout_document_post_refuses_cross_site_mixed_credentials_and_malformed_forms() {
    let page = Arc::new(RecordingCheckout::default());
    let app = app_full(None, Some(page.clone())).await;
    let person = app.person(false);
    let order = order_field(json!({"pay_as_you_go_hours": 5}));
    let good = format!("order={order}&access_token={}", person.token);
    let cases: Vec<(String, Option<&str>, Option<String>, u16)> = vec![
        (good.clone(), None, None, 403),
        (good.clone(), Some("https://evil.example.test"), None, 403),
        (good.clone(), Some(SITE), Some(format!("Bearer {}", person.token)), 400),
        (format!("{good}&extra=1"), Some(SITE), None, 400),
        (format!("{good}&order={order}"), Some(SITE), None, 400),
        (format!("order={order}"), Some(SITE), None, 401),
        (format!("order={order}&access_token=wrong"), Some(SITE), None, 401),
        (format!("order=%%%&access_token={}", person.token), Some(SITE), None, 400),
        (format!("order={}&access_token={}", order_field(json!({})), person.token), Some(SITE), None, 400),
        (format!("{good}&pad={}", "x".repeat(CHECKOUT_FORM_MAX_BYTES)), Some(SITE), None, 413),
    ];
    for (body, origin, authorization, status) in cases {
        let response = app.post_checkout(body.clone(), origin, authorization.as_deref()).await;
        assert_eq!(response.status().as_u16(), status, "{origin:?} {} bytes", body.len());
        let rendered = page.0.lock().unwrap().last().cloned().unwrap();
        assert_eq!((rendered["outcome"].clone(), rendered["status"].clone()), (json!("failed"), json!(status)));
    }
    let json_body = app.http.post(format!("{}/checkout", app.base)).header("content-type", "application/json").header("origin", SITE).body("{}").send().await.unwrap();
    assert_eq!(json_body.status().as_u16(), 415);
    assert_eq!(app.count("SELECT count(*) FROM billing.payments WHERE account_id = $1", person.account).await, 0, "no refusal starts a checkout");

    // Without a configured page the route does not exist; the builder refuses non-HTTPS sites.
    let plain = app().await;
    assert_eq!(plain.post_checkout(good, Some(SITE), None).await.status().as_u16(), 404);
    let state = || BillingHttp::new(Arc::new(Billing::new(Arc::new(PgBillingStore::new(app.owner.clone())), Arc::new(FixedClock), app.directory.clone(), config())), app.tokens.clone());
    for origin in ["http://transgate.ai", "https://transgate.ai/app", "ftp://transgate.ai", "https://"] {
        assert!(state().with_checkout_page(page.clone(), origin).is_err(), "{origin}");
    }
    assert!(state().with_checkout_page(page.clone(), "https://transgate.ai").is_ok());
}
