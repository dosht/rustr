//! taskr's billing sample through the composed server: taskr's own router with
//! its real identity (the explicit dev token), the billing module mounted from
//! `taskr_billing::compose` on the restricted runtime login, real Postgres, and
//! a local LemonSqueezy protocol fixture. Documentation-derived fixture, not a
//! real store.
//!
//! Needs `BILLING_TEST_DATABASE_URL`; ignored offline, failing (never skipping)
//! when run without it.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use billing_lemonsqueezy::signature_hex;
use billing_postgres::{connect_owner, migrate, provision_runtime_login};
use serde_json::{Value, json};
use taskr_billing::{Settings, compose};
use taskr_core::shared::access::Identity;
use taskr_core::shared::telemetry::NoTelemetry;
use taskr_core::testing::{FixedClock, InMemoryTaskListRepository, InMemoryWorkspaceRepository};
use taskr_server::identity::{DevIdentity, Identities, SupabaseJwt};
use taskr_server::{Server, router};
use uuid::Uuid;

const LOGIN: &str = "billing_taskr_runtime";
const PASSWORD: &str = "billing-taskr-runtime-password";
const WEBHOOK_SECRET: &str = "taskr-webhook-secret-not-a-credential";
const JWT_SECRET: &str = "taskr-jwt-secret-for-tests-not-a-credential-0123456789";

fn database_url() -> String {
    std::env::var("BILLING_TEST_DATABASE_URL").expect("BILLING_TEST_DATABASE_URL must name a disposable PostgreSQL database")
}

/// The same server and database with the runtime login's credentials.
fn login_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").expect("a postgres URL");
    let (_, host) = rest.split_once('@').expect("a URL with credentials");
    format!("{scheme}://{LOGIN}:{PASSWORD}@{host}")
}

#[derive(Clone, Default)]
struct Lemon {
    created: Arc<Mutex<Vec<Value>>>,
}

async fn lemon_handler(State(lemon): State<Lemon>, request: Request) -> impl IntoResponse {
    let bytes = to_bytes(request.into_body(), 1 << 20).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    lemon.created.lock().unwrap().push(body);
    let n = lemon.created.lock().unwrap().len();
    // Provider checkout ids are unique across the shared database and repeated runs.
    let unique = Uuid::new_v4().simple();
    (
        StatusCode::CREATED,
        [("content-type", "application/vnd.api+json")],
        json!({"data": {"type": "checkouts", "id": format!("co-{unique}"), "attributes": {"url": format!("https://pay.example.test/c/{unique}-{n}")}}}).to_string(),
    )
}

struct App {
    base: String,
    http: reqwest::Client,
    lemon: Lemon,
    owner: sqlx::PgPool,
}

/// The runtime login is cluster-wide: parallel tests re-keying it at once race inside Postgres,
/// so it is provisioned exactly once per process.
static LOGIN_PROVISIONED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn start() -> App {
    let url = database_url();
    // The real taskr migrator (public history), then billing's own histories beside it.
    taskr_postgres::connect(&url).await.expect("taskr migrations");
    let owner = connect_owner(&url).await.unwrap();
    migrate(&owner).await.unwrap();
    LOGIN_PROVISIONED
        .get_or_init(|| async { provision_runtime_login(&owner, LOGIN, PASSWORD).await.unwrap() })
        .await;

    let lemon = Lemon::default();
    let fixture = Router::new().fallback(lemon_handler).with_state(lemon.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lemon_address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });

    let identity: Arc<dyn Identity> = Arc::new(Identities {
        dev: Some(Arc::new(DevIdentity)),
        real: Some(Arc::new(SupabaseJwt::new(Some(JWT_SECRET), None))),
    });
    let billing = compose(
        Settings {
            database_url: login_url(&url),
            migration_database_url: Some(url.clone()),
            public_url: "https://taskr.example.test".into(),
            lemon_base_url: format!("http://{lemon_address}"),
            lemon_api_key: "key-not-a-credential".into(),
            lemon_store_id: "1".into(),
            lemon_variant_id: "7".into(),
            lemon_signing_secret: WEBHOOK_SECRET.into(),
            lemon_environment: "test".into(),
        },
        identity.clone(),
    )
    .await
    .expect("billing composes on the restricted runtime login");
    // Rerunning both migrators is clean.
    taskr_postgres::connect(&url).await.expect("taskr migrations rerun beside billing's");

    let server = Server {
        task_lists: Arc::new(InMemoryTaskListRepository::default()),
        workspaces: Arc::new(InMemoryWorkspaceRepository::default()),
        identity,
        clock: Arc::new(FixedClock::at(1_000)),
        telemetry: Arc::new(NoTelemetry),
    };
    let app = router(server).merge(billing.router);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    App { base: format!("http://{address}"), http: reqwest::Client::new(), lemon, owner }
}

impl App {
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

    async fn webhook(&self, body: &Value, signature_over: Option<&[u8]>) -> u16 {
        let bytes = body.to_string().into_bytes();
        let signature = signature_hex(WEBHOOK_SECRET, signature_over.unwrap_or(&bytes));
        self.http
            .post(format!("{}/api/lemonsqueezy-webhook", self.base))
            .header("x-signature", signature)
            .body(bytes)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }
}

fn paid_order(payment_id: &str, account: &str, order: &str, cents: i64) -> Value {
    json!({
        "meta": {"event_name": "order_created", "custom_data": {"payment_id": payment_id, "account_id": account}},
        "data": {"type": "orders", "id": order, "attributes": {
            "store_id": 1, "currency": "USD", "subtotal": cents, "discount_total": 0, "tax": 0, "total": cents,
            "tax_inclusive": false, "status": "paid", "test_mode": true,
            "urls": {"receipt": "https://receipts.example.test/1"},
            "first_order_item": {"variant_id": 7, "quantity": 1}
        }}
    })
}

fn work(id: &str, units: i64, operation: &str) -> Value {
    json!({"work_id": id, "units": units, "operation": operation})
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_sample_consumes_ten_rejects_the_eleventh_without_a_write_then_a_signed_order_buys_more() {
    let app = start().await;
    let ada = format!("dev:ada-{}@example.test", Uuid::new_v4().simple());
    let bob = format!("dev:bob-{}@example.test", Uuid::new_v4().simple());

    // No session, no billing; an account that never enrolled is refused, not given credits.
    assert_eq!(app.call("GET", "/api/billing/overview", None, None).await.0, 401);
    assert_eq!(app.call("GET", "/api/billing/overview", Some("not-a-token"), None).await.0, 401);
    let (status, body) = app.call("GET", "/api/billing/overview", Some(&ada), None).await;
    assert_eq!((status, body["error"]["code"].clone()), (404, json!("account_not_provisioned")));
    assert_eq!(app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w0", 1, "x"))).await.0, 404);

    // Enrollment is explicit and idempotent: 10 free credits, once.
    let (status, enrolled) = app.call("POST", "/api/billing/enroll", Some(&ada), None).await;
    assert_eq!((status, enrolled["granted"].clone()), (200, json!(true)));
    assert_eq!(app.call("POST", "/api/billing/enroll", Some(&ada), None).await.1["granted"], false);
    let account = enrolled["account"].as_str().unwrap().to_string();
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&ada), None).await;
    assert_eq!(overview["balance"], 10);

    // Consume all ten in one operation; a retry replays the saved result.
    let (status, first) = app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w1", 10, "render"))).await;
    assert_eq!((status, first["balance_after"].clone(), first["replayed"].clone()), (200, json!(0), json!(false)));
    assert_eq!(first["is_freemium"], true, "only the free credits are open");
    let (_, replay) = app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w1", 10, "render"))).await;
    assert_eq!((replay["balance_after"].clone(), replay["replayed"].clone()), (json!(0), json!(true)));
    let rows = |sql: &'static str| {
        let (pool, account) = (app.owner.clone(), account.clone());
        async move { sqlx::query_scalar::<_, i64>(sql).bind(Uuid::parse_str(&account).unwrap()).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!(rows("SELECT COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1").await, 10, "charged once");

    // The next operation is rejected, and nothing is written for it.
    let (status, refused) = app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w2", 1, "render"))).await;
    assert_eq!((status, refused["error"]["code"].clone()), (402, json!("insufficient_balance")));
    assert_eq!(rows("SELECT count(*) FROM taskr_billing.sample_work WHERE account_id = $1 AND work_key = 'w2'").await, 0, "no pending row survived");
    assert_eq!(rows("SELECT COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1").await, 10);

    // The same work id with a different payload conflicts, even though w1 is done.
    assert_eq!(app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w1", 5, "render"))).await.0, 409);
    assert_eq!(app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w1", 10, "other"))).await.0, 409);
    // Another account has its own w1 and cannot touch ada's row.
    assert_eq!(app.call("POST", "/api/billing/enroll", Some(&bob), None).await.0, 200);
    let (status, bobs) = app.call("POST", "/api/billing/sample/work", Some(&bob), Some(work("w1", 3, "render"))).await;
    assert_eq!((status, bobs["balance_after"].clone(), bobs["replayed"].clone()), (200, json!(7), json!(false)));
    let (_, adas) = app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w1", 10, "render"))).await;
    assert_eq!(adas["balance_after"], 0, "bob's work did not change ada's saved result");

    // Buy a pack through the hosted checkout; the order arrives as a signed webhook.
    let (status, checkout) = app
        .call("POST", "/api/billing/checkout", Some(&ada), Some(json!({"pay_as_you_go_hours": 20, "gateway": "lemonsqueezy"})))
        .await;
    assert_eq!(status, 200, "{checkout}");
    assert_eq!(checkout["amount"], "2.00", "20 credits at 0.10");
    let payment_id = checkout["payment_id"].as_i64().unwrap().to_string();
    let created = app.lemon.created.lock().unwrap().last().cloned().unwrap();
    assert_eq!(created["data"]["attributes"]["custom_price"], 200);
    assert_eq!(created["data"]["attributes"]["checkout_data"]["custom"]["payment_id"], payment_id);
    assert_eq!(created["data"]["attributes"]["checkout_data"]["custom"]["account_id"], account);

    // Forged and unsigned orders buy nothing.
    let order = format!("order-{}", Uuid::new_v4().simple());
    let genuine = paid_order(&payment_id, &account, &order, 200);
    assert_eq!(app.webhook(&genuine, Some(b"not the body")).await, 401, "a signature over other bytes");
    let cheaper = paid_order(&payment_id, &account, &format!("{order}-cheap"), 20);
    assert_eq!(app.webhook(&cheaper, None).await, 200, "verified but held: the amount is not the quote");
    let (_, still) = app.call("GET", "/api/billing/overview", Some(&ada), None).await;
    assert_eq!(still["balance"], 0);
    // The genuine order completes the payment and grants the pack once; a repeat changes nothing.
    assert_eq!(app.webhook(&genuine, None).await, 200);
    assert_eq!(app.webhook(&genuine, None).await, 200);
    let (_, bought) = app.call("GET", "/api/billing/overview", Some(&ada), None).await;
    assert_eq!(bought["balance"], 20);

    // The rejected operation now succeeds; the saved result of w1 is replayed, not recomputed.
    let (status, second) = app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w2", 1, "render"))).await;
    assert_eq!((status, second["balance_after"].clone()), (200, json!(19)));
    assert_eq!(second["is_freemium"], false, "a paid pack is open after the purchase");
    let (_, again) = app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("w1", 10, "render"))).await;
    assert_eq!((again["balance_after"].clone(), again["replayed"].clone()), (json!(0), json!(true)));
    assert_eq!(again["is_freemium"], true, "the saved answer keeps the state at the time of the work");

    // Ordinary taskr behavior is untouched: far more than ten tasks, no metering.
    let (_, me) = app.call("GET", "/api/me", Some(&bob), None).await;
    let workspace = me["workspaces"][0]["id"].as_str().unwrap().to_string();
    for n in 0..12 {
        let (status, _) = app
            .call("POST", &format!("/api/workspaces/{workspace}/tasks"), Some(&bob), Some(json!({"title": format!("task {n}")})))
            .await;
        assert_eq!(status, 201, "task {n}");
    }
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn parallel_identical_work_makes_one_debit_and_the_dev_subject_maps_to_one_account() {
    let app = start().await;
    let ada = format!("dev:ada-{}@example.test", Uuid::new_v4().simple());
    let (_, enrolled) = app.call("POST", "/api/billing/enroll", Some(&ada), None).await;
    let account = Uuid::parse_str(enrolled["account"].as_str().unwrap()).unwrap();
    // The same dev email always reaches the same UUID-shaped account.
    let (_, again) = app.call("POST", "/api/billing/enroll", Some(&ada), None).await;
    assert_eq!(again["account"], enrolled["account"]);

    let app = Arc::new(app);
    let barrier = Arc::new(tokio::sync::Barrier::new(4));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (app, barrier, ada) = (app.clone(), barrier.clone(), ada.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            app.call("POST", "/api/billing/sample/work", Some(&ada), Some(work("same", 6, "render"))).await
        }));
    }
    let mut replayed = 0;
    for h in handles {
        let (status, body) = h.await.unwrap();
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["balance_after"], 4, "every response carries the one saved result");
        if body["replayed"] == true {
            replayed += 1;
        }
    }
    assert_eq!(replayed, 3, "one request did the work, three replayed it");
    let spent: i64 = sqlx::query_scalar("SELECT COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1")
        .bind(account)
        .fetch_one(&app.owner)
        .await
        .unwrap();
    assert_eq!(spent, 6, "one debit");
}

fn jwt(sub: &Uuid, email: &str) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let claims = json!({"sub": sub.to_string(), "email": email, "aud": "authenticated", "exp": now + 3600});
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn re_enrollment_refreshes_the_verified_address_but_never_the_creation_time_or_the_credits() {
    let app = start().await;
    let sub = Uuid::new_v4();
    let old = format!("ada-{}@example.test", Uuid::new_v4().simple());
    let new = format!("renamed-{}@example.test", Uuid::new_v4().simple());
    let (status, first) = app.call("POST", "/api/billing/enroll", Some(&jwt(&sub, &old)), None).await;
    assert_eq!((status, first["granted"].clone(), first["account"].clone()), (200, json!(true), json!(sub.to_string())));
    let stored = |pool: sqlx::PgPool| async move {
        sqlx::query_as::<_, (String, i64)>("SELECT email, billing.ts_to_us(created_at) FROM taskr_billing.customers WHERE account_id = $1")
            .bind(sub)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let (email_before, created_before) = stored(app.owner.clone()).await;
    assert_eq!(email_before, old);
    assert_eq!(app.call("POST", "/api/billing/sample/work", Some(&jwt(&sub, &old)), Some(work("w1", 4, "render"))).await.0, 200);

    // The same account signs in again with a changed verified address.
    let (status, again) = app.call("POST", "/api/billing/enroll", Some(&jwt(&sub, &new)), None).await;
    assert_eq!((status, again["granted"].clone()), (200, json!(false)), "never a second grant");
    let (email_after, created_after) = stored(app.owner.clone()).await;
    assert_eq!((email_after, created_after), (new.clone(), created_before), "the address follows, the creation time stays");
    let (_, overview) = app.call("GET", "/api/billing/overview", Some(&jwt(&sub, &new)), None).await;
    assert_eq!(overview["balance"], 6, "the credits were not reset");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_stale_caller_of_an_erased_account_is_refused_with_410_and_nothing_is_recreated() {
    let app = start().await;
    let ada = format!("dev:ada-{}@example.test", Uuid::new_v4().simple());
    let (status, enrolled) = app.call("POST", "/api/billing/enroll", Some(&ada), None).await;
    assert_eq!(status, 200);
    let account = Uuid::parse_str(enrolled["account"].as_str().unwrap()).unwrap();

    // Erasure has begun for this account (its barrier exists); the caller's session is still valid.
    sqlx::query("INSERT INTO billing.erasure_barriers (account_id, started_at) VALUES ($1, now())").bind(account).execute(&app.owner).await.unwrap();

    for (path, body) in [("/api/billing/enroll", None), ("/api/billing/sample/work", Some(work("after-erasure", 1, "x")))] {
        let (status, answer) = app.call("POST", path, Some(&ada), body).await;
        assert_eq!((status, answer["error"]["code"].clone()), (410, json!("account_erased")), "{path} is a refusal, not a storage outage");
    }
    let created: i64 = sqlx::query_scalar("SELECT count(*) FROM taskr_billing.sample_work WHERE account_id = $1 AND work_key = 'after-erasure'").bind(account).fetch_one(&app.owner).await.unwrap();
    assert_eq!(created, 0, "no sample work is written for an erased account");
}
