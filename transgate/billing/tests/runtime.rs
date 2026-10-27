//! The composed Transgate billing runtime over real HTTP: the real JWT
//! verifier, the real Iyzico and LemonSqueezy adapters against local protocol
//! fixtures, the real HTTP mailer against a local mail fixture, the host
//! customer directory, and Postgres through the restricted runtime login.
//! Fixtures are documentation- and source-derived, not recordings of provider
//! accounts, and no live provider was contacted.
//!
//! Needs `BILLING_TEST_DATABASE_URL`; ignored offline, failing (never skipping)
//! when run without it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use billing_core::pricing::parse_rate;
use billing_core::types::{Clock, Micros};
use billing_iyzico::sign::subscription_v3;
use billing_lemonsqueezy::signature_hex;
use billing_postgres::{connect_owner, migrate, provision_runtime_login};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use transgate_billing::settings::{
    AnalyticsSettings, AuthDirectorySettings, Eligibility, IdentitySettings, IyzicoSettings, LemonSettings, MailSettings,
    Settings,
};
use transgate_billing::{Transgate, compose, compose_with};
use uuid::Uuid;

const LOGIN: &str = "billing_transgate_runtime";
const PASSWORD: &str = "billing-transgate-runtime-password";
const JWT_SECRET: &str = "transgate-jwt-secret-not-a-credential";
const IYZICO_SECRET: &str = "iyzico-secret-not-a-credential";
const LEMON_SECRET: &str = "lemon-secret-not-a-credential";
const SIGNUP_TOKEN: &str = "signup-token-not-a-credential";
const ADMIN: &str = "admin@example.test";
const AUTH_SERVICE_KEY: &str = "auth-service-key-not-a-credential";

fn database_url() -> String {
    std::env::var("BILLING_TEST_DATABASE_URL").expect("BILLING_TEST_DATABASE_URL must name a disposable PostgreSQL database")
}

fn login_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").expect("a postgres URL");
    let (_, host) = rest.split_once('@').expect("a URL with credentials");
    format!("{scheme}://{LOGIN}:{PASSWORD}@{host}")
}

// ---- fixtures ---------------------------------------------------------------

#[derive(Default)]
struct IyzicoState {
    /// Random per fixture instance: provider tokens and references are unique across the
    /// whole shared database and across repeated runs.
    tag: String,
    seen: Vec<(String, String)>,
    subscriptions: HashMap<String, (String, Vec<String>)>,
    counter: u32,
    last_subscription: Option<String>,
    orders: Vec<String>,
    /// The `pricingPlanReferenceCode` of every subscription checkout the provider received.
    plan_codes: Vec<String>,
}

#[derive(Clone)]
struct Iyzico(Arc<Mutex<IyzicoState>>);

impl Default for Iyzico {
    fn default() -> Iyzico {
        Iyzico(Arc::new(Mutex::new(IyzicoState { tag: Uuid::new_v4().simple().to_string(), ..IyzicoState::default() })))
    }
}

async fn iyzico_handler(State(f): State<Iyzico>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_string();
    let bytes = to_bytes(body, 1 << 20).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut s = f.0.lock().unwrap();
    s.seen.push((parts.method.to_string(), path.clone()));
    s.counter += 1;
    let answer = match (parts.method.as_str(), path.as_str()) {
        ("POST", "/payment/iyzipos/checkoutform/initialize/auth/ecom") => {
            json!({"status": "success", "token": format!("pay-token-{}-{}", s.tag, s.counter), "checkoutFormContent": "<script>iyzico form</script>"})
        }
        ("POST", "/payment/iyzipos/checkoutform/auth/ecom/detail") => {
            json!({"status": "success", "paymentStatus": "SUCCESS", "paidPrice": "14.9", "currency": "USD", "paymentId": 5150})
        }
        ("POST", "/v2/subscription/checkoutform/initialize") => {
            let n = s.counter;
            let sub = format!("sub-{}-{n}", s.tag);
            let form = format!("sub-token-{}-{n}", s.tag);
            let plan = body["pricingPlanReferenceCode"].as_str().expect("a plan code").to_string();
            s.plan_codes.push(plan);
            s.subscriptions.insert(form.clone(), (sub.clone(), vec![]));
            s.last_subscription = Some(sub);
            json!({"status": "success", "data": {"token": form, "checkoutFormContent": "<script>sub form</script>"}})
        }
        ("GET", p) if p.starts_with("/v2/subscription/checkoutform/") => {
            let token = p.rsplit('/').next().unwrap().to_string();
            match s.subscriptions.get(&token) {
                Some((sub, _)) => json!({"status": "success", "data": {"referenceCode": sub, "customerReferenceCode": "customer-iyzico-1"}}),
                None => json!({"status": "failure", "errorCode": "404", "errorMessage": "no such form"}),
            }
        }
        ("GET", p) if p.starts_with("/v2/subscription/subscriptions/") => {
            let sub = p.rsplit('/').next().unwrap().to_string();
            let orders: Vec<Value> = s.orders.iter().map(|o| json!({"referenceCode": o, "orderStatus": "SUCCESS"})).collect();
            json!({"status": "success", "data": {"items": [{
                "referenceCode": sub, "customerReferenceCode": "customer-iyzico-1", "subscriptionStatus": "ACTIVE", "orders": orders
            }]}})
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (StatusCode::OK, [("content-type", "application/json")], answer.to_string()).into_response()
}

#[derive(Clone, Default)]
struct Lemon(Arc<Mutex<Vec<Value>>>);

async fn lemon_handler(State(f): State<Lemon>, request: Request) -> Response {
    let bytes = to_bytes(request.into_body(), 1 << 20).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let mut created = f.0.lock().unwrap();
    created.push(body);
    let n = created.len();
    let unique = Uuid::new_v4().simple();
    (
        StatusCode::CREATED,
        [("content-type", "application/vnd.api+json")],
        json!({"data": {"type": "checkouts", "id": format!("co-{unique}"), "attributes": {"url": format!("https://pay.example.test/c/{unique}-{n}")}}}).to_string(),
    )
        .into_response()
}

type Sent = (Value, Option<String>, Option<String>);

#[derive(Default)]
struct MailState {
    sent: Vec<Sent>,
    attempts: Vec<Sent>,
    /// Recipients whose delivery the service refuses with HTTP 500.
    failing: HashSet<String>,
}

#[derive(Clone, Default)]
struct Mail(Arc<Mutex<MailState>>);

async fn mail_handler(State(f): State<Mail>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 1 << 20).await.unwrap();
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let message: Value = serde_json::from_slice(&bytes).unwrap();
    let record = (message.clone(), header("authorization"), header("idempotency-key"));
    let mut state = f.0.lock().unwrap();
    state.attempts.push(record.clone());
    let refused = message["to"][0].as_str().is_some_and(|to| state.failing.contains(to));
    if refused {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "mail service down"}))).into_response();
    }
    state.sent.push(record);
    (StatusCode::OK, Json(json!({"id": "mail-1"}))).into_response()
}

#[derive(Default)]
struct PostHogState {
    /// Accepted events with the path they arrived on.
    events: Vec<(String, Value)>,
    attempts: usize,
    /// Distinct ids whose events the service refuses with HTTP 503.
    failing: HashSet<String>,
}

#[derive(Clone, Default)]
struct PostHogFixture(Arc<Mutex<PostHogState>>);

async fn posthog_handler(State(f): State<PostHogFixture>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let bytes = to_bytes(request.into_body(), 1 << 20).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut state = f.0.lock().unwrap();
    state.attempts += 1;
    if body["distinct_id"].as_str().is_some_and(|id| state.failing.contains(id)) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    state.events.push((path, body));
    (StatusCode::OK, Json(json!({"status": 1}))).into_response()
}

#[derive(Default)]
struct AuthState {
    users: HashMap<String, Value>,
    /// Account ids whose lookup the directory answers with HTTP 500.
    failing: HashSet<String>,
    /// (path, apikey header, authorization header) of every lookup.
    requests: Vec<(String, Option<String>, Option<String>)>,
}

#[derive(Clone, Default)]
struct Auth(Arc<Mutex<AuthState>>);

async fn auth_handler(State(f): State<Auth>, request: Request) -> Response {
    let (parts, _) = request.into_parts();
    let path = parts.uri.path().to_string();
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let mut state = f.0.lock().unwrap();
    state.requests.push((path.clone(), header("apikey"), header("authorization")));
    let id = path.rsplit('/').next().unwrap_or_default().to_string();
    if state.failing.contains(&id) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let id = id.as_str();
    match state.users.get(id) {
        Some(user) => (StatusCode::OK, Json(user.clone())).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"code": 404, "error_code": "user_not_found", "msg": "User not found"}))).into_response(),
    }
}

use axum::Json;

struct TestServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(router: Router) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    TestServer { url: format!("http://{address}"), task }
}

struct App {
    base: String,
    http: reqwest::Client,
    owner: sqlx::PgPool,
    iyzico: Iyzico,
    lemon: Lemon,
    mail: Mail,
    posthog: PostHogFixture,
    auth: Auth,
    transgate: Transgate,
    _servers: Vec<TestServer>,
    _fixture: Arc<Shared>,
}

impl Drop for App {
    fn drop(&mut self) {
        self.transgate.stop();
    }
}

/// Real time plus an adjustable offset, so retry back-off can be passed without waiting.
#[derive(Default)]
struct OffsetClock(std::sync::atomic::AtomicI64);

impl OffsetClock {
    fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Clock for OffsetClock {
    fn now(&self) -> Micros {
        Micros(now_micros() + self.0.load(std::sync::atomic::Ordering::SeqCst) * 1_000_000)
    }
}

fn settings(url: &str, iyzico: &str, lemon: &str, shared: &Shared) -> Settings {
    Settings {
        database_url: login_url(url),
        migration_database_url: Some(url.to_string()),
        public_url: "https://transgate.example.test".into(),
        rate: parse_rate(None),
        iyzico: Some(IyzicoSettings {
            base_url: iyzico.into(),
            api_key: "iyzico-key-not-a-credential".into(),
            secret_key: IYZICO_SECRET.into(),
            merchant_id: "merchant-test".into(),
            environment: "sandbox".into(),
        }),
        lemon: Some(LemonSettings {
            base_url: lemon.into(),
            api_key: "lemon-key-not-a-credential".into(),
            store_id: "1".into(),
            variant_id: "7".into(),
            signing_secret: LEMON_SECRET.into(),
            environment: "test".into(),
        }),
        identity: IdentitySettings {
            hs256_secret: Some(JWT_SECRET.into()),
            jwks_url: None,
            dev_tokens: false,
            admin_emails: BTreeSet::from([ADMIN.to_string()]),
        },
        mail: MailSettings {
            url: format!("{}/send", shared.mail_url),
            api_key: "mail-key-not-a-credential".into(),
            from: "billing@example.test".into(),
            sales_email: "sales@example.test".into(),
            support_email: "support@example.test".into(),
        },
        analytics: Some(AnalyticsSettings { host: shared.posthog_url.clone(), api_key: "phc_test_key_not_a_credential".into() }),
        auth_directory: Some(AuthDirectorySettings { url: shared.auth_url.clone(), service_key: AUTH_SERVICE_KEY.into() }),
        signup_token: Some(SIGNUP_TOKEN.into()),
        eligibility: Eligibility::default(),
    }
}

/// One scenario's protocol fixtures, shared only with its restarted process. Every scenario owns
/// its database: global outbox claims must never lease another scenario's or an earlier suite's
/// events. Drop servers before dropping the database guard, after the last App/restart releases it.
struct Shared {
    mail_url: String,
    posthog_url: String,
    auth_url: String,
    mail: Mail,
    posthog: PostHogFixture,
    auth: Auth,
    _servers: Vec<TestServer>,
    database: IsolatedDb,
}

async fn runtime_fixture() -> Arc<Shared> {
    provision_cluster_login().await;
    let database = isolated_database().await;
    let (mail, posthog, auth) = (Mail::default(), PostHogFixture::default(), Auth::default());
    let mail_server = serve(Router::new().fallback(mail_handler).with_state(mail.clone())).await;
    let posthog_server = serve(Router::new().fallback(posthog_handler).with_state(posthog.clone())).await;
    let auth_server = serve(Router::new().fallback(auth_handler).with_state(auth.clone())).await;
    Arc::new(Shared {
        mail_url: mail_server.url.clone(),
        posthog_url: posthog_server.url.clone(),
        auth_url: auth_server.url.clone(),
        mail,
        posthog,
        auth,
        _servers: vec![mail_server, posthog_server, auth_server],
        database,
    })
}

/// A database of one scenario's own, dropped when its last fixture guard goes (also on panic).
/// The same guard is kept across that scenario's process restart, never shared with another test.
struct IsolatedDb {
    name: String,
}

impl Drop for IsolatedDb {
    fn drop(&mut self) {
        let (name, url) = (self.name.clone(), database_url());
        let _ = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            runtime.block_on(async move {
                if let Ok(owner) = connect_owner(&url).await {
                    // `name` is generated below from [a-z0-9_] only.
                    let statement = format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)");
                    let _ = sqlx::query(sqlx::AssertSqlSafe(statement)).execute(&owner).await;
                    owner.close().await;
                }
            });
        })
        .join();
    }
}

async fn isolated_database() -> IsolatedDb {
    let base = connect_owner(&database_url()).await.unwrap();
    let name = format!("transgate_t_{}", Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{name}\""))).execute(&base).await.expect("create an isolated test database");
    base.close().await;
    IsolatedDb { name }
}

/// The same server and credentials, another database.
fn in_database(url: &str, name: &str) -> String {
    let (server, _) = url.rsplit_once('/').expect("a postgres URL with a database");
    format!("{server}/{name}")
}

async fn start() -> App {
    start_in(Arc::new(transgate_billing::SystemClock), runtime_fixture().await).await
}

/// The runtime login and the billing roles are cluster-wide, so they are provisioned exactly
/// once per process: parallel tests re-keying the same login concurrently race inside Postgres.
static LOGIN_PROVISIONED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn provision_cluster_login() {
    LOGIN_PROVISIONED
        .get_or_init(|| async {
            let base = connect_owner(&database_url()).await.unwrap();
            migrate(&base).await.unwrap();
            provision_runtime_login(&base, LOGIN, PASSWORD).await.unwrap();
            base.close().await;
        })
        .await;
}

async fn start_in(clock: Arc<dyn Clock>, fixture: Arc<Shared>) -> App {
    let url = in_database(&database_url(), &fixture.database.name);
    let owner = connect_owner(&url).await.unwrap();
    migrate(&owner).await.unwrap();
    let iyzico = Iyzico::default();
    let lemon = Lemon::default();
    let iyzico_server = serve(Router::new().fallback(iyzico_handler).with_state(iyzico.clone())).await;
    let lemon_server = serve(Router::new().fallback(lemon_handler).with_state(lemon.clone())).await;
    let mut transgate = compose_with(settings(&url, &iyzico_server.url, &lemon_server.url, &fixture), clock, None)
        .await
        .expect("compose on the restricted runtime login");
    // Tests drive the real relay explicitly. A periodic relay in the same fixture must not lease a
    // batch just before an assertion's manual pass, then finish delivering after that assertion.
    transgate.stop();
    let server = serve(transgate.router.clone()).await;
    App {
        base: server.url.clone(),
        http: reqwest::Client::new(),
        owner,
        iyzico,
        lemon,
        mail: fixture.mail.clone(),
        posthog: fixture.posthog.clone(),
        auth: fixture.auth.clone(),
        transgate,
        _servers: vec![server, iyzico_server, lemon_server],
        _fixture: fixture,
    }
}

fn token(sub: &Uuid, email: &str, expires_in: i64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let claims = json!({"sub": sub.to_string(), "email": email, "aud": "authenticated", "exp": now + expires_in});
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(JWT_SECRET.as_bytes())).unwrap()
}

struct User {
    id: Uuid,
    email: String,
    jwt: String,
}

fn user() -> User {
    let id = Uuid::new_v4();
    let email = format!("{}@example.test", Uuid::new_v4().simple());
    let jwt = token(&id, &email, 3600);
    User { id, email, jwt }
}

impl App {
    async fn call(&self, method: &str, path: &str, bearer: Option<&str>, body: Option<Value>) -> (u16, Value) {
        let mut request = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body.to_string());
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// The host's signup system reports a trusted creation instant.
    async fn signup(&self, user: &User, created_micros: i64) -> (u16, Value) {
        // The identity system knows this account, as it does in production; an existing record
        // (possibly with newer facts) is left as it is.
        let at = billing_core::calendar::format_rfc3339(Micros(created_micros));
        self.auth.0.lock().unwrap().users.entry(user.id.to_string()).or_insert_with(|| auth_user(user, &user.email, &at, &at, true));
        self.call(
            "POST",
            "/api/billing/internal/signup",
            Some(SIGNUP_TOKEN),
            Some(json!({"account_id": user.id.to_string(), "email": user.email, "full_name": "Ada Lovelace", "created_at_micros": created_micros})),
        )
        .await
    }

    async fn relay(&self) {
        for _ in 0..5 {
            self.transgate.billing.relay_outbox(1000, 60).await.unwrap();
        }
    }

    fn mail_sent(&self) -> Vec<Sent> {
        self.mail.0.lock().unwrap().sent.clone()
    }

    fn mails_to(&self, to: &str) -> Vec<Sent> {
        self.mail_sent().into_iter().filter(|(m, _, _)| m["to"][0] == to).collect()
    }

    /// Accepted analytics events for one account, oldest first.
    fn events_for(&self, account: &Uuid) -> Vec<Value> {
        let id = account.to_string();
        self.posthog.0.lock().unwrap().events.iter().filter(|(_, b)| b["distinct_id"] == id.as_str()).map(|(_, b)| b.clone()).collect()
    }
}

fn now_micros() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64
}

const DAY: i64 = 86_400 * 1_000_000;

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn readiness_identity_and_provisioning_through_the_composed_runtime() {
    let app = start().await;
    let (status, ready) = app.call("GET", "/api/billing/ready", None, None).await;
    assert_eq!(status, 200);
    let configured: Vec<String> = serde_json::from_value(ready["configured"].clone()).unwrap();
    for needed in ["checkout:iyzico", "checkout:lemonsqueezy", "webhook:iyzico", "webhook:lemonsqueezy", "mail"] {
        assert!(configured.contains(&needed.to_string()), "{needed} in {configured:?}");
    }

    // Credentials: none, garbage, expired, wrong key. The bearer string is never the user.
    let person = user();
    assert_eq!(app.call("GET", "/api/billing/overview", None, None).await.0, 401);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&person.email), None).await.0, 401);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&token(&person.id, &person.email, -3600)), None).await.0, 401);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&format!("dev:{}", person.email)), None).await.0, 401, "dev tokens are off");

    // A valid session with no trusted signup record is refused, never granted a trial.
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await.0, 404);

    // The host's signup system reports the account created four months ago: first access
    // gets the original, already expired window, not a new trial.
    let (status, granted) = app.signup(&person, now_micros() - 120 * DAY).await;
    assert_eq!((status, granted["granted"].clone()), (200, json!(true)));
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await;
    assert_eq!(overview["balance"], Value::Null);
    assert_eq!(overview["packages"].as_array().unwrap().len(), 1);
    // A signup report from a user session is refused.
    assert_eq!(app.call("POST", "/api/billing/internal/signup", Some(&person.jwt), Some(json!({}))).await.0, 401);

    // First authenticated access with no host record: the trusted auth directory supplies the
    // real signup time and verified address. A signup three days ago gets 20 minutes from then.
    let adopted = user();
    let three_days_ago = Micros(now_micros() - 3 * DAY);
    let signed_up = billing_core::calendar::format_rfc3339(three_days_ago);
    app.auth.0.lock().unwrap().users.insert(adopted.id.to_string(), auth_user(&adopted, &adopted.email, &signed_up, &signed_up, true));
    let (status, fresh) = app.call("GET", "/api/billing/overview", Some(&adopted.jwt), None).await;
    assert_eq!((status, fresh["balance"].clone()), (200, json!(20)), "{fresh}");
    let created: i64 = sqlx::query_scalar("SELECT billing.ts_to_us(created_at) FROM transgate_billing.customers WHERE account_id = $1")
        .bind(adopted.id)
        .fetch_one(&app.owner)
        .await
        .unwrap();
    assert_eq!(created, three_days_ago.0, "the identity system's creation time, not the first request");
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&adopted.jwt), None).await.1["packages"].as_array().unwrap().len(), 1, "never a second trial");
    let requests = app.auth.0.lock().unwrap().requests.clone();
    let ours = requests.iter().find(|(path, _, _)| path.ends_with(&adopted.id.to_string())).unwrap();
    assert_eq!(ours.0, format!("/auth/v1/admin/users/{}", adopted.id));
    assert_eq!(ours.1.as_deref(), Some(AUTH_SERVICE_KEY));
    assert_eq!(ours.2.as_deref(), Some(format!("Bearer {AUTH_SERVICE_KEY}").as_str()));

    // An address the identity system has not verified is not adopted: no record, no trial.
    let unverified = user();
    app.auth.0.lock().unwrap().users.insert(unverified.id.to_string(), auth_user(&unverified, &unverified.email, "2026-01-10T09:00:00Z", "2026-01-10T09:00:00Z", false));
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&unverified.jwt), None).await.0, 404);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_billing.customers WHERE account_id = $1")
        .bind(unverified.id)
        .fetch_one(&app.owner)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    // The trusted directory being down is an explicit outage: never "not provisioned", never a
    // trial anchored to now. The same session succeeds once the directory answers.
    let delayed = user();
    app.auth.0.lock().unwrap().users.insert(delayed.id.to_string(), auth_user(&delayed, &delayed.email, "2026-01-11T09:00:00Z", "2026-01-11T09:00:00Z", true));
    app.auth.0.lock().unwrap().failing.insert(delayed.id.to_string());
    let (status, outage) = app.call("GET", "/api/billing/overview", Some(&delayed.jwt), None).await;
    app.auth.0.lock().unwrap().failing.remove(&delayed.id.to_string());
    assert_eq!(status, 503, "{outage}");
    let records: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM transgate_billing.customers WHERE account_id = $1) + (SELECT count(*) FROM billing.accounts WHERE account_id = $1)")
        .bind(delayed.id)
        .fetch_one(&app.owner)
        .await
        .unwrap();
    assert_eq!(records, 0, "an outage creates nothing");
    let (status, recovered) = app.call("GET", "/api/billing/overview", Some(&delayed.jwt), None).await;
    assert_eq!(status, 200, "{recovered}");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_restricted_runtime_role_is_required_by_the_composition() {
    let fixture = runtime_fixture().await;
    let url = in_database(&database_url(), &fixture.database.name);
    let owner = connect_owner(&url).await.unwrap();
    migrate(&owner).await.unwrap();
    let stub = "http://127.0.0.1:9";
    // The owner's own connection is refused as a server connection.
    let mut owner_settings = settings(&url, stub, stub, &fixture);
    owner_settings.database_url = url.clone();
    owner_settings.migration_database_url = None;
    let error = compose(owner_settings, None).await.err().expect("an owner connection is not a runtime connection");
    assert!(error.contains("restricted billing runtime role"), "{error}");
    // A half-configured group is a startup error that names what is missing.
    let partial = transgate_billing::settings_from(&|k| match k {
        "TRANSGATE_DATABASE_URL" => Some("postgres://x".to_string()),
        "TRANSGATE_PUBLIC_URL" => Some("https://x".to_string()),
        "SUPABASE_JWT_SECRET" => Some("s".to_string()),
        "IYZIPAY_API_KEY" => Some("k".to_string()),
        _ => None,
    });
    assert!(partial.unwrap_err().contains("IYZIPAY_SECRET_KEY"));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_066_075_076_checkout_history_admin_lookup_and_actual_buyer_sales_and_support_mail_over_http() {
    let app = start().await;
    let buyer = user();
    app.signup(&buyer, now_micros()).await;
    let admin = {
        let admin_user = User { id: Uuid::new_v4(), email: ADMIN.into(), jwt: String::new() };
        User { jwt: token(&admin_user.id, ADMIN, 3600), ..admin_user }
    };

    // Pay-as-you-go through the real Iyzico client and the browser return.
    let (status, checkout) = app
        .call("POST", "/api/billing/checkout", Some(&buyer.jwt), Some(json!({"pay_as_you_go_hours": 10, "locale": "en", "gateway": "iyzico"})))
        .await;
    assert_eq!(status, 200, "{checkout}");
    assert_eq!(checkout["amount"], "14.90");
    assert!(checkout["form_content"].as_str().unwrap().contains("iyzico form"));
    let token_value = checkout["token"].as_str().unwrap().to_string();
    let confirm = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("token={token_value}"))
        .send()
        .await
        .unwrap();
    assert_eq!(confirm.status().as_u16(), 200);
    let confirmed: Value = confirm.json().await.unwrap();
    assert_eq!((confirmed["success"].clone(), confirmed["changed"].clone()), (json!(true), json!(true)));
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&buyer.jwt), None).await;
    assert_eq!(overview["balance"], 20 + 600);
    let (_, history) = app.call("GET", "/api/billing/history", Some(&buyer.jwt), None).await;
    assert_eq!(history["payments"].as_array().unwrap().len(), 1);
    let payment_id = history["payments"][0]["payment_id"].as_i64().unwrap();

    // The admin lookup returns this customer's payments; others cannot use it.
    let (status, lookup) = app.call("POST", "/api/billing/admin/customer-invoices", Some(&admin.jwt), Some(json!({"email": buyer.email}))).await;
    assert_eq!((status, lookup["payments"].as_array().unwrap().len()), (200, 1));
    assert_eq!(app.call("POST", "/api/billing/admin/customer-invoices", Some(&buyer.jwt), Some(json!({"email": buyer.email}))).await.0, 403);

    // The owner requests cancellation; someone else cannot.
    let stranger = user();
    app.signup(&stranger, now_micros()).await;
    assert_eq!(app.call("POST", "/api/billing/cancel-subscription", Some(&stranger.jwt), Some(json!({"payment_id": payment_id}))).await.0, 404);
    assert_eq!(app.call("POST", "/api/billing/cancel-subscription", Some(&buyer.jwt), Some(json!({"payment_id": payment_id}))).await.0, 200);

    // Committed events are actually delivered over HTTP to the configured mail service.
    app.relay().await;
    let to_buyer = app.mails_to(&buyer.email);
    assert_eq!(to_buyer.len(), 1, "the buyer's purchase email");
    assert_eq!(to_buyer[0].0["subject"], "Woohoo! Your Transgate purchase is complete!");
    assert_eq!(to_buyer[0].1.as_deref(), Some("Bearer mail-key-not-a-credential"));
    assert!(to_buyer[0].2.as_deref().is_some_and(|k| k.ends_with(":buyer")), "a stable idempotency key per recipient");
    let mine = |subject: &str, needle: &str| {
        app.mail_sent()
            .iter()
            .filter(|(m, _, _)| m["subject"] == subject && m["text"].as_str().unwrap_or("").contains(needle))
            .count()
    };
    assert_eq!(mine("Payment Completed", &buyer.id.to_string()), 1, "the sales copy names the real customer from the directory");
    let cancel_text = payment_id.to_string();
    let cancels = app
        .mail_sent()
        .iter()
        .filter(|(m, _, _)| m["subject"] == "!!!CANCEL SUBSCRIPTION!!!" && m["text"] == cancel_text.as_str())
        .count();
    assert_eq!(cancels, 1, "the support request is the payment id");
    // A second pass delivers nothing more.
    app.relay().await;
    assert_eq!(app.mails_to(&buyer.email).len(), 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn subscription_binding_signed_renewals_held_until_an_admin_identifies_the_initial_order() {
    let app = start().await;
    let subscriber = user();
    app.signup(&subscriber, now_micros()).await;
    let admin_id = Uuid::new_v4();
    let admin_jwt = token(&admin_id, ADMIN, 3600);

    // Yearly Premium (index 2), USD: the plan code goes to the provider, the form comes back.
    let (status, checkout) = app
        .call("POST", "/api/billing/checkout", Some(&subscriber.jwt), Some(json!({"package_id": 2, "locale": "en", "gateway": "iyzico"})))
        .await;
    assert_eq!(status, 200, "{checkout}");
    assert_eq!(checkout["amount"].as_str().unwrap().parse::<f64>().unwrap(), 120.0);
    let form_token = checkout["token"].as_str().unwrap().to_string();
    app.http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/json")
        .body(json!({"token": form_token}).to_string())
        .send()
        .await
        .unwrap();
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&subscriber.jwt), None).await;
    assert_eq!(overview["balance"], 20 + 1200, "the first monthly package of the yearly plan is open now");
    let subscription = app.iyzico.0.lock().unwrap().last_subscription.clone().unwrap();

    let renewal = |event: &str, order: &str| {
        let signature = subscription_v3(IYZICO_SECRET, "merchant-test", event, &subscription, order, "customer-iyzico-1");
        let body = json!({
            "iyziEventType": event, "subscriptionReferenceCode": subscription, "orderReferenceCode": order,
            "customerReferenceCode": "customer-iyzico-1", "iyziReferenceCode": "ref", "iyziEventTime": 1
        });
        (signature, body)
    };
    let deliver = |signature: Option<String>, body: Value| {
        let http = app.http.clone();
        let url = format!("{}/api/subscription-callback", app.base);
        async move {
            let mut request = http.post(url).body(body.to_string());
            if let Some(signature) = signature {
                request = request.header("X-IYZ-SIGNATURE-V3", signature);
            }
            let response = request.send().await.unwrap();
            (response.status().as_u16(), response.json::<Value>().await.unwrap_or(Value::Null))
        }
    };

    // Provider order references are unique across the whole shared database, and a repeated suite
    // run must never meet a previous run's `order-a`: they are minted per test.
    let tag = Uuid::new_v4().simple().to_string();
    let (order_a, order_initial) = (format!("order-a-{tag}"), format!("order-initial-{tag}"));
    let counts = |sql: &'static str, bind: String| {
        let pool = app.owner.clone();
        async move { sqlx::query_scalar::<_, i64>(sql).bind(bind).fetch_one(&pool).await.unwrap() }
    };
    let packages = || counts("SELECT count(*) FROM billing.packages WHERE account_id = $1::uuid", subscriber.id.to_string());
    let payments = || counts("SELECT count(*) FROM billing.payments WHERE account_id = $1::uuid", subscriber.id.to_string());
    let renewal_payments = || counts("SELECT count(*) FROM billing.payments WHERE provider_order_ref = $1", order_a.clone());
    let (packages_before, payments_before) = (packages().await, payments().await);
    assert_eq!((packages_before, payments_before), (13, 1), "the free package and the initial plan's twelve");

    // No signature, a wrong signature and a forged order all create nothing.
    let (signature, body) = renewal("subscription.order.success", &order_a);
    assert_eq!(deliver(None, body.clone()).await.0, 401);
    assert_eq!(deliver(Some("00".repeat(32)), body.clone()).await.0, 401);
    let mut forged = body.clone();
    forged["orderReferenceCode"] = json!(format!("order-forged-{tag}"));
    assert_eq!(deliver(Some(signature.clone()), forged).await.0, 401, "the signature covers the identifiers");
    assert_eq!((packages().await, payments().await), (packages_before, payments_before));

    // A genuine signed notification is acknowledged and held: the initial order is unproven.
    let (status, held) = deliver(Some(signature.clone()), body.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(held["Held"]["reason"], "initial_order_unproven");
    let (_, listed) = app.call("GET", "/api/billing/admin/unresolved", Some(&admin_jwt), None).await;
    assert!(
        listed["held"].as_array().unwrap().iter().any(|r| r["order_ref"] == order_a.as_str() && r["subscription_ref"] == subscription.as_str()),
        "this run's own held row is listed"
    );
    assert_eq!(app.call("GET", "/api/billing/admin/unresolved", Some(&subscriber.jwt), None).await.0, 403);
    assert_eq!((packages().await, payments().await, renewal_payments().await), (packages_before, payments_before, 0), "held means nothing granted");

    // An admin identifies the initial order from the provider's own subscription detail.
    app.iyzico.0.lock().unwrap().orders = vec![order_initial.clone(), order_a.clone()];
    let resolve = |order: String| {
        let admin_jwt = admin_jwt.clone();
        let subscription = subscription.clone();
        let app = &app;
        async move {
            app.call(
                "POST",
                "/api/billing/admin/subscriptions/first-order",
                Some(&admin_jwt),
                Some(json!({"subscription_ref": subscription, "order_ref": order, "rationale": "listed first in the dashboard"})),
            )
            .await
        }
    };
    assert_eq!(resolve(format!("order-not-listed-{tag}")).await.0, 400, "the provider does not list it");
    assert_eq!((packages().await, payments().await), (packages_before, payments_before), "a refused resolution changes nothing");
    let (status, resolved) = resolve(order_initial.clone()).await;
    assert_eq!((status, resolved["replayed"].clone(), resolved["already_resolved"].clone()), (200, json!(1), json!(false)), "{resolved}");
    // One initial grant and one renewal grant, exactly: 1 free + 12 + 12 packages, 2 payments.
    assert_eq!((packages().await, payments().await, renewal_payments().await), (25, 2, 1));

    // Duplicate safety: the same signed renewal again, and the initial order's own notification,
    // are acknowledged and grant nothing more.
    let (status, again) = deliver(Some(signature), body).await;
    assert_eq!(status, 200, "{again}");
    let (initial_signature, initial_body) = renewal("subscription.order.success", &order_initial);
    let (status, initial) = deliver(Some(initial_signature), initial_body).await;
    assert_eq!(status, 200, "{initial}");
    assert_eq!((packages().await, payments().await, renewal_payments().await), (25, 2, 1), "no second grant for either order");
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&subscriber.jwt), None).await;
    assert_eq!(overview["packages"].as_array().unwrap().len(), 25);

    // Resolving again with the same order is safe; naming another order is refused.
    let (status, repeat) = resolve(order_initial.clone()).await;
    assert_eq!((status, repeat["already_resolved"].clone()), (200, json!(true)));
    assert_eq!(resolve(order_a.clone()).await.0, 409, "the initial order cannot be changed");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_lemonsqueezy_pack_through_the_composed_runtime_with_a_signed_order() {
    let app = start().await;
    let buyer = user();
    app.signup(&buyer, now_micros()).await;
    let (status, checkout) = app
        .call("POST", "/api/billing/checkout", Some(&buyer.jwt), Some(json!({"pay_as_you_go_hours": 5, "gateway": "lemonsqueezy"})))
        .await;
    assert_eq!(status, 200, "{checkout}");
    assert!(checkout["url"].as_str().unwrap().starts_with("https://pay.example.test/c/"));
    let payment_id = checkout["payment_id"].as_i64().unwrap().to_string();
    let order = json!({
        "meta": {"event_name": "order_created", "custom_data": {"payment_id": payment_id, "account_id": buyer.id.to_string()}},
        "data": {"type": "orders", "id": format!("order-{}", Uuid::new_v4().simple()), "attributes": {
            "store_id": 1, "currency": "USD", "subtotal": 745, "discount_total": 0, "tax": 149, "total": 894,
            "tax_inclusive": false, "status": "paid", "test_mode": true, "urls": {"receipt": "https://receipts.example.test/1"},
            "first_order_item": {"variant_id": 7, "quantity": 1}
        }}
    });
    let bytes = order.to_string().into_bytes();
    let send = |signature: String| {
        let (http, url, bytes) = (app.http.clone(), format!("{}/api/lemonsqueezy-webhook", app.base), bytes.clone());
        async move { http.post(url).header("x-signature", signature).body(bytes).send().await.unwrap().status().as_u16() }
    };
    assert_eq!(send("0".repeat(64)).await, 401);
    assert_eq!(send(signature_hex(LEMON_SECRET, &bytes)).await, 200);
    assert_eq!(send(signature_hex(LEMON_SECRET, &bytes)).await, 200, "a repeat is acknowledged and changes nothing");
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&buyer.jwt), None).await;
    assert_eq!(overview["balance"], 20 + 300);
    let created = app.lemon.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(created["data"]["attributes"]["custom_price"], 745);
}

fn auth_user(user: &User, email: &str, created_at: &str, updated_at: &str, verified: bool) -> Value {
    json!({
        "id": user.id.to_string(),
        "email": email,
        "email_confirmed_at": verified.then_some("2026-01-10T09:00:05Z"),
        "created_at": created_at,
        "updated_at": updated_at,
        "user_metadata": {"full_name": "Ada Lovelace"}
    })
}

fn admin_jwt() -> String {
    token(&Uuid::new_v4(), ADMIN, 3600)
}

async fn buy_payg(app: &App, buyer: &User, hours: i64) -> i64 {
    let (status, checkout) = app
        .call("POST", "/api/billing/checkout", Some(&buyer.jwt), Some(json!({"pay_as_you_go_hours": hours, "locale": "en", "gateway": "iyzico"})))
        .await;
    assert_eq!(status, 200, "{checkout}");
    let token_value = checkout["token"].as_str().unwrap().to_string();
    let confirm = app
        .http
        .post(format!("{}/checkout/confirm", app.base))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("token={token_value}"))
        .send()
        .await
        .unwrap();
    assert_eq!(confirm.status().as_u16(), 200);
    checkout["payment_id"].as_i64().unwrap()
}

async fn make_contact_due(app: &App, user: &User) {
    sqlx::query("UPDATE transgate_billing.customers SET contact_checked_at = now() - interval '2 hours' WHERE account_id = $1")
        .bind(user.id)
        .execute(&app.owner)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_verified_email_change_reaches_lookup_and_purchase_mail_without_a_second_trial_or_a_stale_replay_overwrite() {
    let app = start().await;
    let (admin, person) = (admin_jwt(), user());
    let old = person.email.clone();
    let created = Micros(now_micros() - 10 * DAY);
    let created_text = billing_core::calendar::format_rfc3339(created);
    app.auth.0.lock().unwrap().users.insert(person.id.to_string(), auth_user(&person, &old, &created_text, &created_text, true));
    let (status, first) = app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await;
    assert_eq!((status, first["packages"].as_array().unwrap().len()), (200, 1));

    // The identity system now says the verified address changed.
    let new = format!("changed-{}@example.test", Uuid::new_v4().simple());
    let updated_text = billing_core::calendar::format_rfc3339(Micros(now_micros() - 60_000_000));
    app.auth.0.lock().unwrap().users.insert(person.id.to_string(), auth_user(&person, &new, &created_text, &updated_text, true));
    let lookup = |email: String| {
        let (admin, app) = (admin.clone(), &app);
        async move { app.call("POST", "/api/billing/admin/customer-invoices", Some(&admin), Some(json!({"email": email}))).await }
    };
    // Just confirmed, so not due: nothing changes yet and no lookup is spent.
    assert_eq!(lookup(old.clone()).await.0, 200);
    assert_eq!(lookup(new.clone()).await.0, 404);
    // When the facts are due, the account's own access re-confirms them.
    make_contact_due(&app, &person).await;
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await.0, 200);
    let (status, found) = lookup(new.clone()).await;
    assert_eq!((status, found["customer"]["email"].clone()), (200, json!(new)));
    assert_eq!(lookup(old.clone()).await.0, 404, "the old address no longer names this customer");
    let stored: (i64, i64) = sqlx::query_as(
        "SELECT billing.ts_to_us(created_at), (SELECT count(*) FROM billing.packages WHERE account_id = $1) FROM transgate_billing.customers WHERE account_id = $1",
    )
    .bind(person.id)
    .fetch_one(&app.owner)
    .await
    .unwrap();
    assert_eq!(stored, (created.0, 1), "the creation time never moved and no second trial was granted");

    // A stale signup replay, still carrying the old address and a fresh creation time, replaces nothing.
    let (status, replay) = app.signup(&User { id: person.id, email: old.clone(), jwt: person.jwt.clone() }, now_micros()).await;
    assert_eq!((status, replay["granted"].clone()), (200, json!(false)));
    assert_eq!(lookup(new.clone()).await.0, 200);
    assert_eq!(lookup(old.clone()).await.0, 404);

    let payments = |app: &App| {
        let (pool, id) = (app.owner.clone(), person.id);
        async move { sqlx::query_scalar::<_, i64>("SELECT count(*) FROM billing.payments WHERE account_id = $1").bind(id).fetch_one(&pool).await.unwrap() }
    };

    // Purchase mail follows the confirmed address, not the old one.
    buy_payg(&app, &person, 5).await;
    app.relay().await;
    assert_eq!(app.mails_to(&new).len(), 1, "the buyer's purchase email goes to the verified address");
    assert!(app.mails_to(&old).is_empty());

    // Facts that are due must be re-confirmed, and an address the identity system has not
    // verified cannot confirm them: the request is refused and the stored recipient is not served.
    let unverified = format!("unverified-{}@example.test", Uuid::new_v4().simple());
    let later = billing_core::calendar::format_rfc3339(Micros(now_micros() - 1_000_000));
    app.auth.0.lock().unwrap().users.insert(person.id.to_string(), auth_user(&person, &unverified, &created_text, &later, false));
    make_contact_due(&app, &person).await;
    let before = payments(&app).await;
    let checkout = json!({"pay_as_you_go_hours": 5, "locale": "en", "gateway": "iyzico"});
    let (status, refused) = app.call("POST", "/api/billing/checkout", Some(&person.jwt), Some(checkout.clone())).await;
    assert_eq!(status, 503, "{refused}");
    assert_eq!(payments(&app).await, before, "no checkout was started toward a possibly stale address");
    assert_eq!(lookup(unverified).await.0, 404);
    assert_eq!(lookup(new.clone()).await.0, 200, "the stored record is kept, still due");

    // An account the directory no longer knows, and an outage of the directory, cannot confirm either.
    app.auth.0.lock().unwrap().users.remove(&person.id.to_string());
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await.0, 503);
    app.auth.0.lock().unwrap().users.insert(person.id.to_string(), auth_user(&person, &new, &created_text, &updated_text, true));
    app.auth.0.lock().unwrap().failing.insert(person.id.to_string());
    let (status, down) = app.call("POST", "/api/billing/checkout", Some(&person.jwt), Some(checkout.clone())).await;
    assert_eq!(status, 503, "{down}");
    assert_eq!(payments(&app).await, before, "no payment was started");
    // The same lookup is what the mail relay asks for the recipient: while the facts are due and
    // unconfirmable it fails, so the event is retried instead of mailing the stored address.
    let recipient = billing_core::ports::CustomerDirectory::by_account(&**app.transgate.billing.directory(), billing_core::types::AccountId(person.id)).await;
    assert!(recipient.is_err(), "{recipient:?}");

    // Once the directory answers, the account recovers and checks out toward the confirmed address.
    app.auth.0.lock().unwrap().failing.remove(&person.id.to_string());
    buy_payg(&app, &person, 5).await;
    assert_eq!(payments(&app).await, before + 1);
    let recipient = billing_core::ports::CustomerDirectory::by_account(&**app.transgate.billing.directory(), billing_core::types::AccountId(person.id)).await;
    assert_eq!(recipient.unwrap().unwrap().email, new);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_066_failed_buyer_mail_recovers_after_a_restart_sending_only_the_missing_recipient_with_the_same_key() {
    let clock = Arc::new(OffsetClock::default());
    let fixture = runtime_fixture().await;
    let first = start_in(clock.clone(), fixture.clone()).await;
    let buyer = user();
    first.signup(&buyer, now_micros()).await;
    first.mail.0.lock().unwrap().failing.insert(buyer.email.clone());
    let payment_id = buy_payg(&first, &buyer, 10).await;
    first.relay().await;

    let marks = |app: &App| {
        let (pool, id) = (app.owner.clone(), buyer.id);
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT d.target FROM billing.outbox_deliveries d JOIN billing.outbox o USING (event_id)
                 WHERE o.account_id = $1 AND o.kind = 'PaymentCompleted' ORDER BY d.target",
            )
            .bind(id)
            .fetch_all(&pool)
            .await
            .unwrap()
        }
    };
    let sales_mails = |app: &App| {
        app.mail_sent()
            .iter()
            .filter(|(m, _, _)| m["subject"] == "Payment Completed" && m["text"].as_str().unwrap_or("").contains(&buyer.id.to_string()))
            .count()
    };
    let attempts_to_buyer = |app: &App| -> Vec<Sent> {
        app.mail.0.lock().unwrap().attempts.iter().filter(|(m, _, _)| m["to"][0] == buyer.email.as_str()).cloned().collect()
    };

    // The service refused the buyer: sales went out and was marked, the buyer's mail is outstanding.
    assert!(first.mails_to(&buyer.email).is_empty());
    let failed = attempts_to_buyer(&first);
    assert!(!failed.is_empty());
    let key = failed[0].2.clone().unwrap();
    assert!(key.ends_with(":buyer") && failed.iter().all(|a| a.2.as_deref() == Some(key.as_str())), "every attempt carries the same key");
    assert_eq!(sales_mails(&first), 1);
    let before = marks(&first).await;
    assert!(before.contains(&"sales".to_string()) && !before.contains(&"buyer".to_string()), "{before:?}");
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted' AND published_at IS NULL")
        .bind(buyer.id)
        .fetch_one(&first.owner)
        .await
        .unwrap();
    assert_eq!(pending, 1, "the completion is not acknowledged while a recipient is outstanding");

    // Restart: the first process stops, the service is repaired, time passes the back-off, a new
    // process composes against the same database.
    first.mail.0.lock().unwrap().failing.remove(&buyer.email);
    drop(first);
    clock.advance(900);
    let second = start_in(clock.clone(), fixture).await;
    second.relay().await;

    let delivered = second.mails_to(&buyer.email);
    assert_eq!(delivered.len(), 1, "the missing buyer mail is sent exactly once");
    assert_eq!(delivered[0].2.as_deref(), Some(key.as_str()), "with the key of the failed attempts");
    assert_eq!(sales_mails(&second), 1, "sales is not sent again");
    let after = marks(&second).await;
    assert!(after.contains(&"buyer".to_string()) && after.contains(&"sales".to_string()), "{after:?}");
    second.relay().await;
    assert_eq!(second.mails_to(&buyer.email).len(), 1);
    assert!(payment_id > 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_072_purchase_and_signup_events_reach_posthog_once_with_the_sources_ids_and_a_refusing_service_is_retried_with_the_same_identity() {
    let clock = Arc::new(OffsetClock::default());
    let fixture = runtime_fixture().await;
    let app = start_in(clock.clone(), fixture).await;
    let buyer = user();
    // The service refuses this account before anything is emitted for it.
    app.posthog.0.lock().unwrap().failing.insert(buyer.id.to_string());
    app.signup(&buyer, now_micros()).await;
    let payment_id = buy_payg(&app, &buyer, 10).await;
    app.relay().await;
    assert!(app.events_for(&buyer.id).is_empty(), "the service refused every event: none was accepted");
    assert_eq!(app.mails_to(&buyer.email).len(), 1, "mail does not wait for analytics");

    app.posthog.0.lock().unwrap().failing.remove(&buyer.id.to_string());
    clock.advance(900);
    app.relay().await;
    app.relay().await;
    let events = app.events_for(&buyer.id);
    let names: BTreeSet<String> = events.iter().map(|e| e["event"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, BTreeSet::from(["payment_completed".to_string(), "user_signed_up".to_string()]));
    assert_eq!(events.len(), 2, "each event once, however many relays ran");

    let namespace = Uuid::parse_str("4d407b2d-2266-45ca-87fc-b13c44f70f26").unwrap();
    let completed = events.iter().find(|e| e["event"] == "payment_completed").unwrap();
    let expected = Uuid::new_v5(&namespace, format!(r#"["payment_completed","payments","{payment_id}"]"#).as_bytes());
    assert_eq!(completed["uuid"], expected.to_string());
    assert_eq!(completed["api_key"], "phc_test_key_not_a_credential");
    assert_eq!(
        completed["properties"],
        json!({"amount": 14.9, "currency": "USD", "payment_id": payment_id, "payment_method": "card", "is_trial": false, "plan": "pay-as-you-go", "$geoip_disable": true})
    );
    let sent_at = billing_core::calendar::parse_rfc3339(completed["timestamp"].as_str().unwrap()).unwrap();
    assert!((sent_at.0 - now_micros()).abs() < 3_600 * 1_000_000 * 2, "the transition's commit time");
    let signup = events.iter().find(|e| e["event"] == "user_signed_up").unwrap();
    let expected = Uuid::new_v5(&namespace, format!(r#"["user_signed_up","users","{}"]"#, buyer.id).as_bytes());
    assert_eq!(signup["uuid"], expected.to_string());
    assert!(signup.get("timestamp").is_none());
}

/// Iyzico plan reference codes (TRY, USD) and prices (TRY at the default rate 44, USD) per plan
/// index, copied from the discovery ledger's plan table (BIL-050..053, CPN-003), not from the catalog.
const SOURCE_PLANS: [(usize, &str, &str, f64, f64); 4] = [
    (0, "60fb9b3d-c6aa-4357-aa9f-463804d8a169", "9aee031c-1cf3-4921-b4b2-b7c337b94f79", 616.0, 14.0),
    (1, "502aa78e-b325-40fb-9eef-36cf9239d863", "d52214fc-f41d-41d3-a0a3-c4824c1ae35c", 924.0, 21.0),
    (2, "77666eba-6655-4888-8502-7c829759569b", "b36a6079-57d6-45a5-aad1-a1fa7a03c158", 5280.0, 120.0),
    (3, "4b7bdf78-8265-475c-aef7-bc824e962b56", "23f69d4b-6c64-4f3f-b25c-d67c6a491128", 7920.0, 180.0),
];

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_050_053_cpn_003_each_plan_and_currency_sends_its_source_plan_code_and_price_to_the_provider() {
    let app = start().await;
    let buyer = user();
    app.signup(&buyer, now_micros()).await;
    for (index, try_code, usd_code, try_price, usd_price) in SOURCE_PLANS {
        // The Turkish interface sells in TRY under the TRY plan; every other locale in USD.
        for (locale, currency, code, price) in [("tr", "TRY", try_code, try_price), ("en", "USD", usd_code, usd_price)] {
            let before = app.iyzico.0.lock().unwrap().plan_codes.len();
            let (status, view) = app
                .call("POST", "/api/billing/checkout", Some(&buyer.jwt), Some(json!({"package_id": index, "locale": locale, "gateway": "iyzico"})))
                .await;
            assert_eq!(status, 200, "plan {index} {currency}: {view}");
            assert_eq!(view["currency"], currency, "plan {index}");
            assert_eq!(view["amount"].as_str().unwrap().parse::<f64>().unwrap(), price, "plan {index} {currency}");
            let sent = app.iyzico.0.lock().unwrap().plan_codes[before..].to_vec();
            assert_eq!(sent, vec![code.to_string()], "plan {index} {currency} reached the provider under the wrong plan code");
        }
    }
}

#[path = "runtime/erasure.rs"]
mod erasure;

#[path = "runtime/ai.rs"]
mod ai;

#[path = "runtime/api.rs"]
mod api;

#[path = "runtime/p3_close.rs"]
mod p3_close;
