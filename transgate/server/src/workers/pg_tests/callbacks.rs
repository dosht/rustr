//! P7 D5/D7 end to end on a real database: the event relay creates one delivery per completed
//! event with a callback URL, the dispatcher POSTs a signed body to a loopback receiver, and the
//! delivery, attempt record and retry schedule follow the policy. The clock is the test's, so no
//! test waits real minutes.

use super::*;
use std::sync::atomic::AtomicU16;
use transgate_core::api::app::{CallbackServices, Callbacks, JobsCallbackContent};
use transgate_core::api::callbacks::{DELIVERY_ID_HEADER, SIGNATURE_HEADER, signed_message};
use transgate_core::api::ports::ApiCrypto;
use transgate_core::jobs::events::JobEvent;
use transgate_core::transcript::NewLine;

use crate::callbacks::HostCallbackTransport;
use crate::crypto::HostCrypto;

type Seen = Arc<Mutex<Vec<(Vec<(String, String)>, Vec<u8>)>>>;

/// A loopback receiver that answers with whatever status the test currently sets.
async fn receiver() -> (String, Seen, Arc<AtomicU16>) {
    use axum::{Router, body::{Body, to_bytes}, extract::{Request, State}, response::Response};
    let seen: Seen = Arc::default();
    let status = Arc::new(AtomicU16::new(200));
    async fn handle(State((seen, status)): State<(Seen, Arc<AtomicU16>)>, request: Request) -> Response {
        let (parts, body) = request.into_parts();
        let headers = parts.headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string())).collect();
        let bytes = to_bytes(body, 1 << 20).await.unwrap().to_vec();
        seen.lock().unwrap().push((headers, bytes));
        Response::builder().status(status.load(Ordering::SeqCst)).body(Body::from("received")).unwrap()
    }
    let app = Router::new().fallback(handle).with_state((seen.clone(), status.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen, status)
}

fn workers_with_callbacks(base: &Workers, pool: &PgPool, clock: Arc<dyn Clock>) -> Arc<Workers> {
    let job_store: Arc<dyn JobStore> = Arc::new(base.store.clone());
    let callbacks = Arc::new(Callbacks::new(CallbackServices {
        outbox: Arc::new(transgate_jobs_postgres::PgCallbackOutbox::new(pool.clone())),
        transport: Arc::new(HostCallbackTransport::new(true)),
        content: Arc::new(JobsCallbackContent { store: job_store, jobs: base.jobs.clone(), ai: base.ai.clone() }),
        crypto: Arc::new(HostCrypto),
        clock: clock.clone(),
    }));
    Arc::new(Workers {
        jobs: base.jobs.clone(), privacy: base.privacy.clone(), ai: base.ai.clone(), store: base.store.clone(), pool: pool.clone(),
        mailer: base.mailer.clone(), directory: PgCustomerDirectory::new(pool.clone()), clock, locks: base.locks.clone(),
        callbacks: Some(callbacks),
    })
}

/// A completed job with one line and the callback URL, and its completion event in the outbox.
async fn completed_with_callback(store: &PgJobStore, url: &str, at: Micros) -> (AccountId, transgate_core::JobId) {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    assert!(uow.billing().await.unwrap().create_account(account, at).await.unwrap());
    let job = uow
        .insert_job(&NewJob {
            account,
            status: JobStatus::Completed,
            name: Some("meeting.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: Language::from_code("en"),
            translate_to: vec![],
            speakers: false,
            fix_grammer: None,
            duration_minutes: Some(2),
            size_bytes: Some(10),
            callback_url: Some(url.into()),
            whisper_params: None,
            created_at: at,
        })
        .await
        .unwrap();
    uow.insert_lines(&job, &[NewLine {
        start_time: "00:00:00".into(), end_time: "00:00:05".into(), line: "Hello world".into(), speaker: None, corrected_text: None,
        split_on_speaker_change: None, language: None, translation_status: None,
    }]).await.unwrap();
    uow.emit(&JobEvent::Completed {
        job: job.id, account, provider: None, language: Language::from_code("en"), error: None, callback_url: Some(url.into()), at,
    }).await.unwrap();
    uow.commit().await.unwrap();
    (account, job.id)
}

async fn deliveries(pool: &PgPool, account: AccountId) -> Vec<(String, i32, i64)> {
    sqlx::query_as("SELECT state, attempts, transgate_jobs.ts_to_us(next_attempt_at) FROM transgate_api.callback_deliveries WHERE account_id = $1")
        .bind(account.0).fetch_all(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn a_completed_event_makes_one_delivery_and_the_dispatcher_posts_a_signed_body_then_retries_by_the_schedule() {
    let (pool, _database) = isolated().await;
    let now = Micros(1_900_000_000 * SECOND);
    let clock = Arc::new(Injected(AtomicI64::new(now.0)));
    let base = replica(&pool, clock.clone(), Arc::new(HeldListing::new(Arc::new(Notify::new()), Arc::new(Notify::new()))));
    let workers = workers_with_callbacks(&base, &pool, clock.clone());
    let (url, seen, status) = receiver().await;
    let (account, job) = completed_with_callback(&base.store, &url, now).await;

    // The relay creates exactly one delivery; relaying the same event again creates no second one.
    assert_eq!(workers.deliver_events().await.unwrap(), 1);
    assert_eq!(deliveries(&pool, account).await, vec![("pending".to_string(), 0, now.0)]);
    let event_id: i64 = sqlx::query_scalar("SELECT id FROM transgate_jobs.events WHERE account_id = $1").bind(account.0).fetch_one(&pool).await.unwrap();
    let mut uow = base.store.begin().await.unwrap();
    let again = Callbacks::fan_out(uow.as_mut(), event_id, &JobEvent::Completed {
        job, account, provider: None, language: None, error: None, callback_url: Some(url.clone()), at: now,
    }, now).await.unwrap();
    uow.commit().await.unwrap();
    assert!(!again, "a relay retry of the same event inserts nothing");
    assert_eq!(deliveries(&pool, account).await.len(), 1);

    // A receiver that fails: the first attempt is recorded and the next is a minute later.
    status.store(500, Ordering::SeqCst);
    let pass = workers.callback_tick().await.unwrap();
    assert_eq!((pass.claimed, pass.retrying, pass.delivered), (1, 1, 0));
    assert_eq!(deliveries(&pool, account).await, vec![("pending".to_string(), 1, now.0 + 60 * SECOND)]);
    let (headers, body) = seen.lock().unwrap()[0].clone();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default();
    let delivery_id: Uuid = sqlx::query_scalar("SELECT id FROM transgate_api.callback_deliveries WHERE account_id = $1").bind(account.0).fetch_one(&pool).await.unwrap();
    assert_eq!(header(&DELIVERY_ID_HEADER.to_ascii_lowercase()), delivery_id.to_string());
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body_json["job"]["id"], job.0);
    assert_eq!(body_json["result"][0]["text"], "Hello world");
    assert!(body_json["job"]["audio_url"].is_null(), "no stored audio, so none is signed in");

    // The signature verifies with the account's stored secret over `"<t>." + body`.
    let secret: String = sqlx::query_scalar("SELECT secret FROM transgate_api.signing_secrets WHERE account_id = $1").bind(account.0).fetch_one(&pool).await.unwrap();
    let signature = header(&SIGNATURE_HEADER.to_ascii_lowercase());
    let (t, v1) = signature.strip_prefix("t=").and_then(|s| s.split_once(",v1=")).expect("t=,v1= format");
    assert_eq!(t.parse::<i64>().unwrap(), now.0 / SECOND);
    assert_eq!(v1, HostCrypto.hmac_sha256_hex(secret.as_bytes(), &signed_message(t.parse().unwrap(), &body)));

    // Before the retry time nothing is claimed; at it, a healthy receiver ends the delivery.
    clock.0.store(now.0 + 59 * SECOND, Ordering::SeqCst);
    assert_eq!(workers.callback_tick().await.unwrap().claimed, 0);
    clock.0.store(now.0 + 60 * SECOND, Ordering::SeqCst);
    status.store(200, Ordering::SeqCst);
    let pass = workers.callback_tick().await.unwrap();
    assert_eq!((pass.claimed, pass.delivered), (1, 1));
    assert_eq!(deliveries(&pool, account).await[0].0, "delivered");
    assert_eq!(seen.lock().unwrap().len(), 2);
    let attempts: Vec<(i32, Option<serde_json::Value>)> =
        sqlx::query_as("SELECT n, outcome FROM transgate_api.callback_attempts WHERE delivery_id = $1 ORDER BY n").bind(delivery_id).fetch_all(&pool).await.unwrap();
    assert_eq!(attempts.iter().map(|(n, o)| (*n, o.as_ref().map(|o| o["code"].clone()))).collect::<Vec<_>>(), vec![(1, Some(500.into())), (2, Some(200.into()))]);
    // Delivered is final: no later pass sends it again.
    clock.0.store(now.0 + 86_400 * SECOND, Ordering::SeqCst);
    assert_eq!(workers.callback_tick().await.unwrap().claimed, 0);
}

#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn an_event_without_a_callback_url_or_for_a_failed_job_makes_no_delivery_and_a_final_refusal_is_not_retried() {
    let (pool, _database) = isolated().await;
    let now = Micros(1_900_000_000 * SECOND);
    let clock = Arc::new(Injected(AtomicI64::new(now.0)));
    let base = replica(&pool, clock.clone(), Arc::new(HeldListing::new(Arc::new(Notify::new()), Arc::new(Notify::new()))));
    let workers = workers_with_callbacks(&base, &pool, clock.clone());

    // D6: an empty callback URL delivers nothing, and neither does a failed job's event.
    let (silent, _) = completed_with_callback(&base.store, "", now).await;
    let (failed_account, failed_job) = completed_with_callback(&base.store, "https://unused.example.test/x", now).await;
    sqlx::query("DELETE FROM transgate_jobs.events WHERE account_id = $1").bind(failed_account.0).execute(&pool).await.unwrap();
    let mut uow = base.store.begin().await.unwrap();
    uow.emit(&JobEvent::Failed { job: failed_job, account: failed_account, language: None, error: None, at: now }).await.unwrap();
    uow.commit().await.unwrap();
    workers.deliver_events().await.unwrap();
    assert!(deliveries(&pool, silent).await.is_empty());
    assert!(deliveries(&pool, failed_account).await.is_empty());

    // D5: any other 4xx is final. One attempt, state `failed`, and no later pass sends it again.
    let (url, seen, status) = receiver().await;
    status.store(404, Ordering::SeqCst);
    let (account, _) = completed_with_callback(&base.store, &url, now).await;
    workers.deliver_events().await.unwrap();
    let pass = workers.callback_tick().await.unwrap();
    assert_eq!((pass.claimed, pass.failed, pass.retrying), (1, 1, 0));
    assert_eq!(deliveries(&pool, account).await[0].0, "failed");
    clock.0.store(now.0 + 86_400 * SECOND, Ordering::SeqCst);
    assert_eq!(workers.callback_tick().await.unwrap().claimed, 0);
    assert_eq!(seen.lock().unwrap().len(), 1, "a final refusal is never retried");
}
