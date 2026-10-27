//! P7 public API over real HTTP and the restricted PostgreSQL role: a signed-in owner manages keys,
//! a key reads a transcription, and every refusal keeps its status. The crypto is a deterministic
//! test double (the real digests have their own vector tests in `transgate-server`).
use std::sync::atomic::{AtomicU64, Ordering};

use transgate_core::AccountId;
use transgate_core::api::app::{Api, ApiServices, CallbackServices, Callbacks, JobsCallbackContent};
use transgate_core::jobs::events::JobEvent;
use transgate_core::JobId;
use transgate_core::api::callbacks::{DELIVERY_ID_HEADER, SIGNATURE_HEADER};
use transgate_core::api::ports::{CallbackRequest, CallbackTransport};
use transgate_core::api::callbacks::SendOutcome;
use transgate_core::api::limits::RateLimits;
use transgate_core::api::ports::ApiCrypto;
use transgate_core::jobs::domain::{JobSource, JobStatus, NewJob, StorageProvider};
use transgate_core::jobs::ports::{BoxFuture, JobStore};
use transgate_core::transcript::NewLine;
use transgate_jobs_postgres::{PgApiRateLimiter, PgCallbackOutbox, PgJobStore};

use transgate_http::TransgateHttp;

use super::*;

#[derive(Default)]
struct TestCrypto(AtomicU64);
impl ApiCrypto for TestCrypto {
    fn random_token(&self) -> String {
        // 43 alphanumerics then `=`: the shape `keys::well_formed` accepts.
        format!("{:043}=", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn sha256_hex(&self, data: &[u8]) -> String {
        format!("digest:{}", data.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }
    fn hmac_sha256_hex(&self, secret: &[u8], message: &[u8]) -> String {
        format!("mac:{}:{}", secret.len(), message.len())
    }
    fn verify_legacy<'a>(&'a self, _key: &'a str, _hash: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async { false })
    }
}

async fn fixture(limits: RateLimits) -> (App, PgJobStore, Arc<Callbacks>) {
    let clock: Arc<dyn Clock> = Arc::new(transgate_billing::SystemClock);
    let mut app = start_in(clock.clone(), runtime_fixture().await).await;
    transgate_jobs_postgres::migrate(&app.owner).await.unwrap();
    transgate_jobs_postgres::verify_runtime(&app.transgate.pool).await.unwrap();
    let pool = app.transgate.pool.clone();
    let store = PgJobStore::new(pool.clone());
    let limiter: Arc<dyn transgate_core::jobs::ports::RateLimiter> = Arc::new(transgate_jobs_postgres::PgRateLimiter::new(pool.clone()));
    let jobs = Arc::new(transgate_core::jobs::app::Jobs::new(
        transgate_core::jobs::app::JobsServices {
            store: Arc::new(store.clone()), clock: clock.clone(), objects: vec![], transcribers: vec![],
            stager: None, sleeper: Arc::new(super::erasure::RuntimeSleeper), rate_limiter: limiter.clone(),
            paid_captions: None, video_metadata: None,
        },
        transgate_core::jobs::app::JobsConfig {
            routing: transgate_core::jobs::routing::RoutingConfig { soniox_max_duration_minutes: 200 },
            models: transgate_core::jobs::routing::ModelConfig { soniox: "m".into(), soniox_v5: "m".into(), deepgram: "m".into(), deepgram_account_overrides: vec![] },
            upload_storage: StorageProvider::Gcs, upload_bucket: "uploads".into(),
            archive_store: media_ports::StoreKind::Gcs, archive_bucket: "archive".into(),
            staging_store: media_ports::StoreKind::Gcs, staging_bucket: "staging".into(),
            callback_base_url: "https://transgate.example.test".into(), soniox_auto_delete: true,
            ssrf_protection: true, freemium_lists: Default::default(),
        },
    ));
    let privacy = Arc::new(transgate_core::privacy::app::Privacy::new(transgate_core::privacy::app::PrivacyServices {
        store: Arc::new(store.clone()), clock: clock.clone(), locks: Arc::new(transgate_jobs_postgres::PgLocks::new(pool.clone())),
        objects: vec![], inventories: vec![], transcribers: vec![],
        identity: Arc::new(super::erasure::IdentityDeletion::default()), billing: Arc::new(billing_postgres::PgBillingStore::new(pool.clone())),
        terminators: vec![], exceptions: transgate_core::privacy::erasure::ExceptionPolicy::default(),
    }));
    let api = Arc::new(Api::new(ApiServices {
        store: Arc::new(store.clone()), jobs: jobs.clone(), crypto: Arc::new(TestCrypto::default()),
        limiter: Arc::new(PgApiRateLimiter::new(pool.clone())), clock: clock.clone(),
        outbox: Arc::new(PgCallbackOutbox::new(pool.clone())), limits,
    }));
    let ai = Arc::new(transgate_core::ai::Ai::new(
        transgate_core::ai::AiServices {
            store: Arc::new(store.clone()), clock: clock.clone(), llm: None,
            sleeper: Arc::new(super::erasure::RuntimeSleeper), live: Arc::new(transgate_core::ai::live::LiveHub::new(64 * 1024, 1024 * 1024)),
        },
        transgate_core::ai::AiConfig {
            features_enabled: false, chat_enabled: false, chat_validation: true,
            limits: transgate_core::ai::ChatLimits::default(), model: transgate_core::ai::ModelSettings::source(), bounds: transgate_core::ai::Bounds::default(),
        },
    ));
    let callbacks = Arc::new(Callbacks::new(CallbackServices {
        outbox: Arc::new(PgCallbackOutbox::new(pool.clone())),
        transport: Arc::new(LoopbackTransport(reqwest::Client::new())),
        content: Arc::new(JobsCallbackContent { store: Arc::new(store.clone()), jobs: jobs.clone(), ai }),
        crypto: Arc::new(TestCrypto::default()),
        clock: clock.clone(),
    }));
    let state = Arc::new(TransgateHttp {
        ai: None, jobs, privacy,
        public_transcript: Arc::new(transgate_core::public_transcript::PublicTranscript::new(Arc::new(super::erasure::NoPublicCaptions), limiter, clock.clone())),
        queries: Arc::new(store.clone()), transcribers: vec![], identity: app.transgate.identity.clone(),
        clock, cron_secret: None, downloads: None, api: Some(api), callbacks: Some(callbacks.clone()),
    });
    let server = serve(app.transgate.router.clone().merge(transgate_http::router(state))).await;
    app.base = server.url.clone();
    app._servers.push(server);
    (app, store, callbacks)
}

/// Posts the delivery to its URL as core built it. The SSRF pin has its own tests; a loopback
/// receiver is exactly what that pin refuses, so this double only does the HTTP.
struct LoopbackTransport(reqwest::Client);
impl CallbackTransport for LoopbackTransport {
    fn post<'a>(&'a self, request: &'a CallbackRequest) -> BoxFuture<'a, SendOutcome> {
        Box::pin(async move {
            let sent = self.0.post(&request.url)
                .header(DELIVERY_ID_HEADER, request.delivery.0.to_string())
                .header(SIGNATURE_HEADER, &request.signature)
                .body(request.body.clone())
                .send().await;
            match sent {
                Ok(response) => SendOutcome::Status { code: response.status().as_u16(), excerpt: String::new() },
                Err(e) => SendOutcome::Network { error: e.to_string() },
            }
        })
    }
}

/// A loopback receiver that records the delivery id of every POST and answers 200.
async fn receiver() -> (String, Arc<Mutex<Vec<String>>>) {
    async fn handle(State(seen): State<Arc<Mutex<Vec<String>>>>, request: axum::extract::Request) -> StatusCode {
        let id = request.headers().get(DELIVERY_ID_HEADER).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        seen.lock().unwrap().push(id);
        StatusCode::OK
    }
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let app = Router::new().fallback(handle).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

/// A completed job with a callback URL; its completion makes one delivery, as the event relay does.
async fn delivery_for_a_completed_job(store: &PgJobStore, owner: &User, url: &str) -> Uuid {
    let job = completed(store, owner).await;
    let at = Micros(transgate_billing::SystemClock.now().0 - 3_600_000_000);
    let event = JobEvent::Completed { job: JobId(job), account: AccountId(owner.id), provider: None, language: None, error: None, callback_url: Some(url.into()), at };
    let event_id = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs();
    let mut uow = store.begin().await.unwrap();
    let created = Callbacks::fan_out(uow.as_mut(), event_id, &event, transgate_billing::SystemClock.now()).await.unwrap();
    uow.commit().await.unwrap();
    assert!(created);
    sqlx::query_scalar("SELECT id FROM transgate_api.callback_deliveries WHERE event_id = $1")
        .bind(event_id).fetch_one(store.pool()).await.unwrap()
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn an_admin_replay_appends_an_attempt_to_the_same_delivery_and_nobody_else_can() {
    let (app, store, callbacks) = fixture(RateLimits::default()).await;
    let owner = signed_in(&app).await;
    let admin_id = Uuid::new_v4();
    let admin = User { id: admin_id, email: ADMIN.into(), jwt: token(&admin_id, ADMIN, 3600) };
    super::erasure::register_auth(&app, &admin);
    let (url, seen) = receiver().await;
    let delivery = delivery_for_a_completed_job(&store, &owner, &url).await;
    let attempts = |pool: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM transgate_api.callback_attempts WHERE delivery_id = $1").bind(delivery).fetch_one(&pool).await.unwrap()
    };
    let path = format!("/api/admin/callbacks/{delivery}/replay");

    // First delivery: one attempt, the receiver answered 200, the delivery is final.
    assert_eq!(callbacks.dispatch_pass(10).await.delivered, 1);
    assert_eq!(attempts(app.owner.clone()).await, 1);

    // Only a verified administrator may replay; the id is not even read for anyone else.
    assert_eq!(app.call("POST", &path, Some(&owner.jwt), None).await.0, 403);
    assert_eq!(app.call("POST", &path, None, None).await.0, 401);
    assert_eq!(attempts(app.owner.clone()).await, 1, "a refused replay changes nothing");
    // An unknown delivery is a 404, and a value that is not an id names none.
    assert_eq!(app.call("POST", &format!("/api/admin/callbacks/{}/replay", Uuid::new_v4()), Some(&admin.jwt), None).await.0, 404);
    assert_eq!(app.call("POST", "/api/admin/callbacks/not-an-id/replay", Some(&admin.jwt), None).await.0, 404);

    // The admin replay reopens the same delivery; the next pass appends attempt 2 to it.
    let (status, body) = app.call("POST", &path, Some(&admin.jwt), None).await;
    assert_eq!((status, body["status"].clone()), (200, json!("queued")), "{body}");
    assert_eq!(callbacks.dispatch_pass(10).await.delivered, 1);
    assert_eq!(attempts(app.owner.clone()).await, 2);
    let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_api.callback_deliveries WHERE account_id = $1")
        .bind(owner.id).fetch_one(&app.owner).await.unwrap();
    assert_eq!(deliveries, 1, "a replay never creates a second delivery");
    assert_eq!(seen.lock().unwrap().clone(), vec![delivery.to_string(), delivery.to_string()], "the receiver saw the same delivery id twice");

    // Nothing is pending now, so a dispatch pass sends nothing more.
    assert_eq!(callbacks.dispatch_pass(10).await.claimed, 0);
}

async fn signed_in(app: &App) -> User {
    let person = user();
    super::erasure::register_auth(app, &person);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await.0, 200);
    person
}

async fn completed(store: &PgJobStore, person: &User) -> i64 {
    let mut uow = store.begin().await.unwrap();
    let job = uow.insert_job(&NewJob {
        account: AccountId(person.id), status: JobStatus::Completed, name: Some("meeting.mp3".into()),
        source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
        language: None, translate_to: vec![], speakers: false, fix_grammer: None,
        duration_minutes: Some(2), size_bytes: Some(10), callback_url: None, whisper_params: None,
        created_at: transgate_billing::SystemClock.now(),
    }).await.unwrap();
    uow.insert_lines(&job, &[NewLine {
        start_time: "00:00:00".into(), end_time: "00:00:05".into(), line: "Hello world".into(),
        speaker: None, corrected_text: None, split_on_speaker_change: None, language: None, translation_status: None,
    }]).await.unwrap();
    uow.commit().await.unwrap();
    job.id.0
}

async fn new_key(app: &App, person: &User, scopes: &str) -> (String, String) {
    let body = json!({"name": "CI", "scopes": serde_json::from_str::<Value>(scopes).unwrap(), "expirationPolicy": "never"});
    let (status, created) = app.call("POST", "/api/keys", Some(&person.jwt), Some(body)).await;
    assert_eq!(status, 201, "{created}");
    (created["apiKey"].as_str().unwrap().to_string(), created["key"]["id"].as_str().unwrap().to_string())
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn an_owner_manages_keys_and_a_key_reads_only_its_own_accounts_transcription() {
    let (app, store, _) = fixture(RateLimits::default()).await;
    let (owner, stranger) = (signed_in(&app).await, signed_in(&app).await);
    let job = completed(&store, &owner).await;
    let (secret, id) = new_key(&app, &owner, r#"["transcriptions:read"]"#).await;

    // The list names the key and never its secret.
    let (status, listed) = app.call("GET", "/api/keys", Some(&owner.jwt), None).await;
    assert_eq!(status, 200);
    assert_eq!(listed["keys"][0]["id"], id);
    assert!(!listed.to_string().contains(&secret), "a stored key is never shown again");

    // The key reads its own account's job, with the rate headers.
    let url = format!("{}/api/v1/transcriptions?jobId={job}", app.base);
    let response = app.http.get(&url).bearer_auth(&secret).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.headers()["x-ratelimit-limit-minute"], "60");
    assert_eq!(response.headers()["x-ratelimit-remaining-minute"], "59");
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"]["job"]["status"], "completed");
    assert_eq!(body["data"]["result"][0]["text"], "Hello world");

    // Another account's key sees it as not found, never as forbidden.
    let (other_secret, _) = new_key(&app, &stranger, r#"["transcriptions:read"]"#).await;
    let foreign = app.http.get(&url).bearer_auth(&other_secret).send().await.unwrap();
    assert_eq!(foreign.status().as_u16(), 404);

    // A missing, malformed or session credential never reaches a key actor (no fallback).
    for bearer in [None, Some("sk_nope"), Some(owner.jwt.as_str())] {
        let mut request = app.http.get(&url);
        if let Some(value) = bearer { request = request.bearer_auth(value); }
        assert_eq!(request.send().await.unwrap().status().as_u16(), 401, "{bearer:?}");
    }
    // A key cannot manage keys, and a read-only key cannot create work.
    assert_eq!(app.call("GET", "/api/keys", Some(&secret), None).await.0, 401);
    let start = app.http.post(format!("{}/api/v1/transcriptions", app.base)).bearer_auth(&secret).json(&json!({"jobIds": [job]})).send().await.unwrap();
    assert_eq!(start.status().as_u16(), 403);

    // Another account cannot revoke it; the owner can, and the key stops working.
    assert_eq!(app.call("DELETE", &format!("/api/keys/{id}"), Some(&stranger.jwt), None).await.0, 404);
    assert_eq!(app.call("DELETE", "/api/keys/not-an-id", Some(&owner.jwt), None).await.0, 404);
    let (status, revoked) = app.call("DELETE", &format!("/api/keys/{id}"), Some(&owner.jwt), None).await;
    assert_eq!((status, revoked["key"]["isActive"].clone()), (200, json!(false)));
    assert_eq!(app.http.get(&url).bearer_auth(&secret).send().await.unwrap().status().as_u16(), 401);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_limit_refuses_with_retry_after_and_the_signing_secret_is_stable_and_owner_only() {
    let (app, store, _) = fixture(RateLimits { per_minute: 2, per_hour: 100 }).await;
    let owner = signed_in(&app).await;
    let job = completed(&store, &owner).await;
    let (secret, _) = new_key(&app, &owner, r#"["transcriptions:read"]"#).await;
    let url = format!("{}/api/v1/transcriptions?jobId={job}", app.base);
    let statuses = [
        app.http.get(&url).bearer_auth(&secret).send().await.unwrap().status().as_u16(),
        app.http.get(&url).bearer_auth(&secret).send().await.unwrap().status().as_u16(),
    ];
    assert_eq!(statuses, [200, 200]);
    let limited = app.http.get(&url).bearer_auth(&secret).send().await.unwrap();
    assert_eq!(limited.status().as_u16(), 429);
    assert!(limited.headers().contains_key("retry-after"));
    assert_eq!(limited.headers()["x-ratelimit-remaining-minute"], "0");

    let (status, first) = app.call("GET", "/api/keys/signing-secret", Some(&owner.jwt), None).await;
    assert_eq!(status, 200);
    let (_, again) = app.call("GET", "/api/keys/signing-secret", Some(&owner.jwt), None).await;
    assert_eq!(first, again, "created once, then stable");
    assert_eq!(app.call("GET", "/api/keys/signing-secret", Some(&secret), None).await.0, 401);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_document_is_served_from_the_same_table_that_mounts_the_routes() {
    let (app, _, _) = fixture(RateLimits::default()).await;
    let served: Value = app.http.get(format!("{}/api/v1/openapi.json", app.base)).send().await.unwrap().json().await.unwrap();
    assert_eq!(served, transgate_api::openapi::document());
    let yaml = app.http.get(format!("{}/openapi/openapi.yaml", app.base)).send().await.unwrap();
    assert_eq!(yaml.status().as_u16(), 200);
    // Every documented operation is mounted: an unmounted one would answer a router 404 with no body.
    for op in transgate_api::openapi::OPERATIONS {
        let path = op.path.replace("{id}", "00000000-0000-0000-0000-000000000000");
        let method = reqwest::Method::from_bytes(op.method.to_uppercase().as_bytes()).unwrap();
        let response = app.http.request(method, format!("{}{path}", app.base)).send().await.unwrap();
        let status = response.status().as_u16();
        assert!(matches!(status, 400 | 401 | 403), "{}: unauthenticated call answered {status}", op.id);
    }
}

// API-BIL-012: a key whose account has no balance left (zero or less) starting transcriptions gets 402
// `insufficient_balance`, in both start modes, and nothing is created: no job, no charge, no delivery.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn api_bil_012_a_key_with_zero_balance_gets_402_and_nothing_is_created() {
    let (app, store, _) = fixture(RateLimits::default()).await;
    let owner = user();
    assert_eq!(app.signup(&owner, now_micros()).await.0, 200, "an account with an open free package");
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&owner.jwt), None).await.0, 200);
    let (secret, _) = new_key(&app, &owner, r#"["transcriptions:create"]"#).await;
    let draft = {
        let mut uow = store.begin().await.unwrap();
        let job = uow.insert_job(&NewJob {
            account: AccountId(owner.id), status: JobStatus::Draft, name: Some("d.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: None, translate_to: vec![], speakers: false, fix_grammer: None,
            duration_minutes: Some(3), size_bytes: Some(10), callback_url: None, whisper_params: None,
            created_at: transgate_billing::SystemClock.now(),
        }).await.unwrap();
        uow.commit().await.unwrap();
        job.id.0
    };
    // Spend exactly everything the free package holds: the balance is zero.
    sqlx::query("INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at) SELECT $1, id, 999999, minutes_included, now() FROM billing.packages WHERE account_id = $1")
        .bind(owner.id).execute(&app.owner).await.unwrap();
    let counts = || async {
        let one = |sql: &'static str| {
            let (pool, id) = (app.owner.clone(), owner.id);
            async move { sqlx::query_scalar::<_, i64>(sql).bind(id).fetch_one(&pool).await.unwrap() }
        };
        (
            one("SELECT count(*) FROM transgate_jobs.jobs WHERE account_id = $1").await,
            one("SELECT COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1").await,
            one("SELECT count(*) FROM transgate_api.callback_deliveries WHERE account_id = $1").await,
        )
    };
    let before = counts().await;
    for (mode, body) in [
        ("jobIds", json!({"jobIds": [draft]})),
        ("audioUrls", json!({"audioUrls": [{"audioUrl": "https://example.com/a.mp3", "callbackUrl": "https://example.com/hook"}]})),
    ] {
        let response = app.http.post(format!("{}/api/v1/transcriptions", app.base)).bearer_auth(&secret).json(&body).send().await.unwrap();
        let status = response.status().as_u16();
        let answer: Value = response.json().await.unwrap();
        assert_eq!((status, answer["code"].clone()), (402, json!("insufficient_balance")), "{mode}: {answer}");
        assert_eq!(counts().await, before, "{mode}: no job, charge or delivery row was created");
    }
    let status: String = sqlx::query_scalar("SELECT status FROM transgate_jobs.jobs WHERE id = $1").bind(draft).fetch_one(&app.owner).await.unwrap();
    assert_eq!(status, "draft", "the draft did not start");
}
