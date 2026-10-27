//! P7 persistence against a real database through the restricted billing runtime
//! login: keys, idempotency, the D3 limiter, the D5 outbox and erasure.
//!
//! Run with `TRANSGATE_JOBS_TEST_DATABASE_URL=... cargo test -p transgate-jobs-postgres --test api -- --include-ignored`.
//! The URL names a disposable database with owner authority; a missing variable
//! fails the test, it never skips. Every test uses random accounts and keys.

use std::sync::Arc;

use billing_core::ports::StoreError;
use billing_core::types::{AccountId, Micros};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::{Mutex, OnceCell};
use transgate_core::api::KeyId;
use transgate_core::api::callbacks::{CallbackDelivery, DeliveryId, DeliveryState, SendOutcome, Settlement};
use transgate_core::api::idempotency::{IdempotencyRecord, expired_before};
use transgate_core::api::keys::{ApiKey, ExpirationPolicy};
use transgate_core::api::limits::{self, RateLimits, RateWindows};
use transgate_core::api::ports::{ApiRateLimiter, ApiTx, CallbackAttempt, CallbackOutbox, Reopen};
use transgate_core::ids::{JobId, MICROS_PER_DAY, MICROS_PER_MINUTE};
use transgate_core::jobs::app::{JobResult, StartAnswer, StartMeta};
use transgate_core::jobs::domain::JobStatus;
use transgate_core::jobs::ports::JobUow;
use transgate_jobs_postgres::{PgApiRateLimiter, PgCallbackOutbox, PgJobUow};
use uuid::Uuid;

const RUNTIME_LOGIN: &str = "transgate_jobs_test_runtime";
const RUNTIME_PASSWORD: &str = "transgate-jobs-test-runtime-password";
const NOW: Micros = Micros(1_768_478_400_000_000); // 2026-01-15T12:00:00Z
const SECOND: i64 = 1_000_000;

/// `JobUow::commit` consumes a boxed unit; these tests hold a concrete `PgJobUow`.
trait CommitExt {
    fn commit(self) -> transgate_core::jobs::ports::BoxFuture<'static, Result<(), StoreError>>;
}

impl CommitExt for PgJobUow {
    fn commit(self) -> transgate_core::jobs::ports::BoxFuture<'static, Result<(), StoreError>> {
        <PgJobUow as JobUow>::commit(Box::new(self))
    }
}

fn database_url() -> String {
    std::env::var("TRANSGATE_JOBS_TEST_DATABASE_URL")
        .expect("TRANSGATE_JOBS_TEST_DATABASE_URL must name a disposable PostgreSQL database for these tests")
}

static PROVISIONED: OnceCell<()> = OnceCell::const_new();

/// The outbox claims every due delivery, so tests that claim must not run concurrently.
static OUTBOX: Mutex<()> = Mutex::const_new(());

async fn owner() -> PgPool {
    let owner = billing_postgres::connect_owner(&database_url()).await.expect("owner connection");
    PROVISIONED
        .get_or_init(|| async {
            billing_postgres::migrate(&owner).await.expect("billing migrations");
            transgate_jobs_postgres::migrate(&owner).await.expect("jobs migrations");
            billing_postgres::provision_runtime_login(&owner, RUNTIME_LOGIN, RUNTIME_PASSWORD).await.expect("runtime login");
        })
        .await;
    owner
}

async fn runtime() -> PgPool {
    owner().await;
    let options = billing_postgres::options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap();
    let pool = billing_postgres::connect_runtime(options, 12).await.expect("restricted runtime pool");
    transgate_jobs_postgres::verify_runtime(&pool).await.expect("restricted transgate runtime");
    pool
}

fn key(account: AccountId, created_at: Micros) -> ApiKey {
    let id = KeyId(Uuid::new_v4());
    ApiKey {
        id,
        account,
        name: "ci".into(),
        created_at,
        expires_at: None,
        last_used_at: None,
        active: true,
        scopes: vec!["transcriptions:read".into(), "legacy:unknown".into()],
        expiration_policy: ExpirationPolicy::Days90,
        metadata: json!({"source": "test"}),
        digest: Some(format!("{:0>64}", id.0.simple())),
        legacy_bcrypt: None,
        hint: Some("sk_AbCdE".into()),
    }
}

fn legacy_key(account: AccountId, created_at: Micros) -> ApiKey {
    ApiKey { digest: None, legacy_bcrypt: Some("$2a$06$abcdefghijklmnopqrstuuabcdefghijklmnopqrstuvwxyz01234".into()), hint: None, ..key(account, created_at) }
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql).bind(id).fetch_one(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn keys_round_trip_and_the_legacy_scan_set_is_exactly_active_unexpired_and_not_upgraded() {
    let pool = runtime().await;
    let account = AccountId(Uuid::new_v4());
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    let older = key(account, Micros(NOW.0 - 2 * MICROS_PER_DAY));
    let newer = ApiKey { expires_at: Some(Micros(NOW.0 + MICROS_PER_DAY)), last_used_at: Some(NOW), ..key(account, NOW) };
    uow.insert_key(&older).await.unwrap();
    uow.insert_key(&newer).await.unwrap();
    assert_eq!(uow.lock_key(newer.id).await.unwrap().unwrap(), newer, "every field, unknown scopes included, reads back");
    assert!(uow.lock_key(KeyId(Uuid::new_v4())).await.unwrap().is_none());
    assert_eq!(uow.key_by_digest(newer.digest.as_deref().unwrap()).await.unwrap().unwrap().id, newer.id);
    assert!(uow.key_by_digest("no-such-digest").await.unwrap().is_none());
    assert_eq!(uow.list_keys(account).await.unwrap().iter().map(|k| k.id).collect::<Vec<_>>(), vec![newer.id, older.id], "newest first");

    let duplicate_digest = ApiKey { id: KeyId(Uuid::new_v4()), ..newer.clone() };
    uow.savepoint("dup").await.unwrap();
    assert!(matches!(uow.insert_key(&duplicate_digest).await, Err(StoreError::Conflict(_))), "digest is unique");
    uow.rollback_to_savepoint("dup").await.unwrap();
    let unidentified = ApiKey { digest: None, legacy_bcrypt: None, ..key(account, NOW) };
    uow.savepoint("none").await.unwrap();
    assert!(matches!(uow.insert_key(&unidentified).await, Err(StoreError::Conflict(_))), "a key with neither digest nor hash is refused");
    uow.rollback_to_savepoint("none").await.unwrap();

    // D2 scan set: active, unexpired (equal still counts), not yet upgraded, hash present.
    let scan = legacy_key(account, NOW);
    let expired = ApiKey { expires_at: Some(Micros(NOW.0 - 1)), ..legacy_key(account, NOW) };
    let boundary = ApiKey { expires_at: Some(NOW), ..legacy_key(account, NOW) };
    let revoked = ApiKey { active: false, ..legacy_key(account, NOW) };
    for k in [&scan, &expired, &boundary, &revoked] {
        uow.insert_key(k).await.unwrap();
    }
    let mut ids: Vec<KeyId> = uow.legacy_candidates(NOW).await.unwrap().into_iter().filter(|k| k.account == account).map(|k| k.id).collect();
    ids.sort();
    let mut want = vec![scan.id, boundary.id];
    want.sort();
    assert_eq!(ids, want);

    // First use upgrades the row: digest written, hash nulled, out of the scan set.
    let mut upgraded = uow.lock_key(scan.id).await.unwrap().unwrap();
    upgraded.digest = Some(format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()));
    upgraded.legacy_bcrypt = None;
    upgraded.hint = Some("sk_Hint1".into());
    upgraded.last_used_at = Some(NOW);
    uow.save_key(&upgraded).await.unwrap();
    assert_eq!(uow.lock_key(scan.id).await.unwrap().unwrap(), upgraded);
    assert!(uow.legacy_candidates(NOW).await.unwrap().iter().all(|k| k.id != scan.id));
    let gone = ApiKey { id: KeyId(Uuid::new_v4()), ..upgraded.clone() };
    assert!(matches!(uow.save_key(&gone).await, Err(StoreError::Conflict(_))), "saving a missing key is a conflict, never an insert");
    uow.commit().await.unwrap();
}

fn answer() -> StartAnswer {
    StartAnswer {
        results: vec![JobResult { job_id: JobId(7), status: JobStatus::NotStarted, estimated_completion: Some(NOW), error: None }],
        meta: StartMeta {
            new_balance: 10, is_freemium: false, total_jobs: 1, successful_jobs: 1, failed_jobs: 0, queued_jobs: 1,
            estimated_completion: Some(NOW), request_type: "audioUrls".into(),
        },
    }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn an_idempotency_key_is_live_for_a_day_then_replaced_and_never_shared_between_accounts() {
    let pool = runtime().await;
    let (account, other) = (AccountId(Uuid::new_v4()), AccountId(Uuid::new_v4()));
    let record = |account, digest: &str, at: Micros| IdempotencyRecord {
        account, key: "order-1".into(), request_digest: digest.into(), request_type: "audioUrls".into(), jobs: vec![JobId(4), JobId(5)],
        answer: None, created_at: at,
    };
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    assert!(uow.idempotency(account, "order-1", expired_before(NOW)).await.unwrap().is_none());
    uow.insert_idempotency(&record(account, "aa", NOW), expired_before(NOW)).await.unwrap();
    uow.insert_idempotency(&record(other, "zz", NOW), expired_before(NOW)).await.expect("another account's key is independent");
    assert_eq!(uow.idempotency(account, "order-1", expired_before(NOW)).await.unwrap().unwrap(), record(account, "aa", NOW));
    uow.savepoint("live").await.unwrap();
    let live = uow.insert_idempotency(&record(account, "bb", NOW), expired_before(Micros(NOW.0 + 1_000))).await;
    assert!(matches!(live, Err(StoreError::Conflict(_))), "a live record is never replaced: {live:?}");
    uow.rollback_to_savepoint("live").await.unwrap();
    assert_eq!(uow.idempotency(account, "order-1", expired_before(NOW)).await.unwrap().unwrap().request_digest, "aa");

    uow.save_idempotency_answer(account, "order-1", &answer()).await.unwrap();
    assert_eq!(uow.idempotency(account, "order-1", expired_before(NOW)).await.unwrap().unwrap().answer, Some(answer()));
    assert!(matches!(uow.save_idempotency_answer(account, "unknown", &answer()).await, Err(StoreError::Conflict(_))));

    // A day later the record is forgotten: it reads as absent and the key can be used again.
    let later = Micros(NOW.0 + MICROS_PER_DAY + SECOND);
    assert!(uow.idempotency(account, "order-1", expired_before(later)).await.unwrap().is_none());
    uow.insert_idempotency(&record(account, "cc", later), expired_before(later)).await.unwrap();
    let replaced = uow.idempotency(account, "order-1", expired_before(later)).await.unwrap().unwrap();
    assert_eq!((replaced.request_digest.as_str(), replaced.answer.is_none()), ("cc", true), "the replacement starts clean");
    uow.commit().await.unwrap();
}

fn delivery(account: AccountId, event: i64, at: Micros) -> CallbackDelivery {
    CallbackDelivery {
        id: DeliveryId(Uuid::new_v4()), account, job: JobId(event), event_id: event, url: "https://callback.example.test/hook".into(),
        state: DeliveryState::Pending, attempts: 0, next_attempt_at: at, completed_at: at, created_at: at, delivered_at: None,
        payload: Some(json!({"job": {"id": event}})),
    }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_relay_retry_inserts_no_second_delivery_for_the_same_event() {
    let pool = runtime().await;
    let event = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs();
    let account = AccountId(Uuid::new_v4());
    let first = delivery(account, event, NOW);
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    assert!(uow.insert_delivery(&first).await.unwrap());
    assert!(!uow.insert_delivery(&delivery(account, event, NOW)).await.unwrap(), "ON CONFLICT (event_id) inserts nothing");
    uow.commit().await.unwrap();
    let owner = owner().await;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM transgate_api.callback_deliveries WHERE event_id = $1").bind(event).fetch_all(&owner).await.unwrap();
    assert_eq!(ids, vec![first.id.0]);
}

// D3: both windows are consumed together or not at all, a denied request counts nowhere, and
// windows are the calendar's minute and hour, not first-hit windows.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_limiter_consumes_both_windows_or_neither_on_the_calendars_minute_and_hour() {
    let pool = runtime().await;
    let account = AccountId(Uuid::new_v4());
    let (a, b) = (key(account, NOW), key(account, NOW));
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    uow.insert_key(&a).await.unwrap();
    uow.insert_key(&b).await.unwrap();
    uow.commit().await.unwrap();
    let limiter = PgApiRateLimiter::new(pool.clone());
    let at = Micros(NOW.0 - NOW.0.rem_euclid(3_600_000_000) + 10 * MICROS_PER_MINUTE + 59 * SECOND); // hh:10:59
    let windows = RateWindows::at(at);
    let tight = RateLimits { per_minute: 3, per_hour: 5 };

    // The reference semantics (core `limits::consume`) and the store agree step by step.
    let (mut minute, mut hour) = (0u32, 0u32);
    for step in 0..5 {
        let counts = limiter.consume(a.id, windows, tight).await.unwrap();
        let reference = limits::consume(tight, minute, hour);
        assert_eq!(counts, reference, "step {step}");
        if counts.allowed { minute = counts.minute; hour = counts.hour; }
    }
    assert_eq!((minute, hour), (3, 3), "the minute window refused the 4th and 5th request; they counted nowhere");

    // The next minute starts a fresh minute count, while the hour keeps adding up to its own limit.
    let next_minute = RateWindows::at(Micros(at.0 + SECOND));
    assert_eq!(limiter.consume(a.id, next_minute, tight).await.unwrap(), limits::consume(tight, 0, 3));
    assert_eq!(limiter.consume(a.id, next_minute, tight).await.unwrap().hour, 5);
    let denied = limiter.consume(a.id, next_minute, tight).await.unwrap();
    assert!(!denied.allowed, "the hour is spent");
    assert_eq!((denied.minute, denied.hour), (2, 5), "a denial by the hour does not spend the minute either");
    let owner = owner().await;
    let minute_row: i32 = sqlx::query_scalar("SELECT count FROM transgate_api.rate_windows WHERE key_id = $1 AND kind = 'minute' AND window_start = transgate_jobs.us_to_ts($2)")
        .bind(a.id.0).bind(next_minute.minute_start.0).fetch_one(&owner).await.unwrap();
    assert_eq!(minute_row, 2);

    // Another key has its own windows; the next hour has its own count.
    assert_eq!(limiter.consume(b.id, windows, tight).await.unwrap().hour, 1);
    let next_hour = RateWindows::at(Micros(windows.hour_start.0 + 3_600_000_000 + SECOND));
    assert_eq!(limiter.consume(a.id, next_hour, tight).await.unwrap(), limits::consume(tight, 0, 0));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn concurrent_requests_never_overshoot_a_limit() {
    let pool = runtime().await;
    let account = AccountId(Uuid::new_v4());
    let k = key(account, NOW);
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    uow.insert_key(&k).await.unwrap();
    uow.commit().await.unwrap();
    let limiter = Arc::new(PgApiRateLimiter::new(pool.clone()));
    let windows = RateWindows::at(NOW);
    let limits = RateLimits { per_minute: 7, per_hour: 1000 };
    let tasks: Vec<_> = (0..24)
        .map(|_| {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.consume(k.id, windows, limits).await.unwrap().allowed })
        })
        .collect();
    let mut allowed = 0;
    for task in tasks {
        allowed += usize::from(task.await.unwrap());
    }
    assert_eq!(allowed, 7, "exactly the limit is admitted across 24 racing requests");
    let owner = owner().await;
    let stored: i32 = sqlx::query_scalar("SELECT count FROM transgate_api.rate_windows WHERE key_id = $1 AND kind = 'hour'").bind(k.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!(stored, 7);
}

async fn due_delivery(pool: &PgPool, account: AccountId, event: i64, at: Micros) -> CallbackDelivery {
    let d = delivery(account, event, at);
    let mut uow = PgJobUow::begin(pool).await.unwrap();
    assert!(uow.insert_delivery(&d).await.unwrap());
    uow.commit().await.unwrap();
    d
}

fn attempt(d: &CallbackDelivery, n: u32, at: Micros) -> CallbackAttempt {
    CallbackAttempt { delivery: d.id, n, started_at: at, outcome: None, finished_at: None }
}

// D5: a claim leases the row; every later write is fenced by that claim's token.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_outbox_leases_fences_and_records_an_attempt_before_the_post() {
    let _serial = OUTBOX.lock().await;
    let pool = runtime().await;
    let owner = owner().await;
    let outbox = PgCallbackOutbox::new(pool.clone());
    let account = AccountId(Uuid::new_v4());
    let event = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs();
    let d = due_delivery(&pool, account, event, NOW).await;

    let not_due = outbox.claim_deliveries(1000, 60, Micros(NOW.0 - SECOND)).await.unwrap();
    assert!(not_due.iter().all(|c| c.delivery.id != d.id), "a delivery is claimed only once it is due");
    let claims = outbox.claim_deliveries(1000, 60, NOW).await.unwrap();
    let first = claims.iter().find(|c| c.delivery.id == d.id).expect("due and unclaimed").clone();
    assert_eq!(first.delivery, d);
    assert!(outbox.claim_deliveries(1000, 60, NOW).await.unwrap().iter().all(|c| c.delivery.id != d.id), "a live lease is not claimed twice");

    // The lease runs out: a second claim supersedes the first, whose writes now change nothing.
    let later = Micros(NOW.0 + 61 * SECOND);
    let second = outbox.claim_deliveries(1000, 60, later).await.unwrap().into_iter().find(|c| c.delivery.id == d.id).unwrap();
    assert_ne!(first.token, second.token);
    let body = json!({"job": {"id": 1}, "result": []});
    assert!(!outbox.begin_attempt(&first, &attempt(&d, 1, later), &body).await.unwrap(), "the superseded claim cannot begin an attempt");
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_attempts WHERE delivery_id = $1", d.id.0).await, 0);

    // Begin commits the attempt row and the counted attempt before any POST.
    assert!(outbox.begin_attempt(&second, &attempt(&d, 1, later), &body).await.unwrap());
    let (attempts, payload): (i32, serde_json::Value) =
        sqlx::query_as("SELECT attempts, payload FROM transgate_api.callback_deliveries WHERE id = $1").bind(d.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!((attempts, payload), (1, body.clone()));
    let started: (i32, Option<serde_json::Value>) =
        sqlx::query_as("SELECT n, outcome FROM transgate_api.callback_attempts WHERE delivery_id = $1").bind(d.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!(started, (1, None), "recorded as started, outcome still unknown");

    // The superseded worker cannot settle either; the live claim can.
    let outcome = SendOutcome::Status { code: 503, excerpt: "busy".into() };
    let mut finished = attempt(&d, 1, later);
    finished.outcome = Some(outcome.clone());
    finished.finished_at = Some(Micros(later.0 + SECOND));
    let retry_at = Micros(later.0 + 60 * SECOND);
    let retry = Settlement { state: DeliveryState::Pending, next_attempt_at: Some(retry_at), delivered_at: None };
    assert!(!outbox.finish_attempt(&first, &finished, &retry).await.unwrap());
    assert!(outbox.finish_attempt(&second, &finished, &retry).await.unwrap());
    let (state, next, token): (String, i64, Option<Uuid>) = sqlx::query_as(
        "SELECT state, transgate_jobs.ts_to_us(next_attempt_at), claim_token FROM transgate_api.callback_deliveries WHERE id = $1",
    ).bind(d.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!((state.as_str(), next, token), ("pending", retry_at.0, None), "retry scheduled, lease released");
    let stored: serde_json::Value = sqlx::query_scalar("SELECT outcome FROM transgate_api.callback_attempts WHERE delivery_id = $1 AND n = 1").bind(d.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!(stored, serde_json::to_value(&outcome).unwrap());

    // The next attempt continues the count and delivers.
    let third = outbox.claim_deliveries(1000, 60, retry_at).await.unwrap().into_iter().find(|c| c.delivery.id == d.id).unwrap();
    assert_eq!(third.delivery.attempts, 1);
    assert!(outbox.begin_attempt(&third, &attempt(&d, 2, retry_at), &body).await.unwrap());
    let mut ok = attempt(&d, 2, retry_at);
    ok.outcome = Some(SendOutcome::Status { code: 204, excerpt: String::new() });
    ok.finished_at = Some(retry_at);
    let delivered = Settlement { state: DeliveryState::Delivered, next_attempt_at: None, delivered_at: Some(retry_at) };
    assert!(outbox.finish_attempt(&third, &ok, &delivered).await.unwrap());
    let (state, at): (String, Option<i64>) = sqlx::query_as("SELECT state, transgate_jobs.ts_to_us(delivered_at) FROM transgate_api.callback_deliveries WHERE id = $1")
        .bind(d.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!((state.as_str(), at), ("delivered", Some(retry_at.0)));
    assert!(outbox.claim_deliveries(1000, 60, Micros(retry_at.0 + 86_400 * SECOND)).await.unwrap().iter().all(|c| c.delivery.id != d.id), "a settled delivery is never claimed again");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn postponing_and_abandoning_release_the_lease_only_for_the_claim_that_holds_it() {
    let _serial = OUTBOX.lock().await;
    let pool = runtime().await;
    let owner = owner().await;
    let outbox = PgCallbackOutbox::new(pool.clone());
    let account = AccountId(Uuid::new_v4());
    let (e1, e2) = (i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs(), i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs());
    let (waiting, gone) = (due_delivery(&pool, account, e1, NOW).await, due_delivery(&pool, account, e2, NOW).await);
    let claims = outbox.claim_deliveries(1000, 60, NOW).await.unwrap();
    let waiting_claim = claims.iter().find(|c| c.delivery.id == waiting.id).unwrap().clone();
    let gone_claim = claims.iter().find(|c| c.delivery.id == gone.id).unwrap().clone();

    // D7: the summary is not decided; the delivery comes back later without an attempt.
    let stale = ClaimedDelivery_with_token(&waiting_claim, Uuid::new_v4());
    assert!(!outbox.postpone(&stale, Micros(NOW.0 + 30 * SECOND)).await.unwrap(), "a wrong token changes nothing");
    assert!(outbox.postpone(&waiting_claim, Micros(NOW.0 + 30 * SECOND)).await.unwrap());
    assert!(outbox.claim_deliveries(1000, 60, Micros(NOW.0 + 29 * SECOND)).await.unwrap().iter().all(|c| c.delivery.id != waiting.id));
    assert!(outbox.claim_deliveries(1000, 60, Micros(NOW.0 + 30 * SECOND)).await.unwrap().iter().any(|c| c.delivery.id == waiting.id));
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_attempts WHERE delivery_id = $1", waiting.id.0).await, 0, "postponing is not an attempt");

    assert!(!outbox.abandon(&ClaimedDelivery_with_token(&gone_claim, Uuid::new_v4()), "job deleted", NOW).await.unwrap());
    assert!(outbox.abandon(&gone_claim, "job deleted", NOW).await.unwrap());
    let (state, note): (String, Option<String>) = sqlx::query_as("SELECT state, note FROM transgate_api.callback_deliveries WHERE id = $1").bind(gone.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!((state.as_str(), note.as_deref()), ("abandoned", Some("job deleted")));
    assert!(!outbox.postpone(&gone_claim, NOW).await.unwrap(), "an ended delivery cannot be postponed back to life");
}

#[allow(non_snake_case)]
fn ClaimedDelivery_with_token(claim: &transgate_core::api::ports::ClaimedDelivery, token: Uuid) -> transgate_core::api::ports::ClaimedDelivery {
    transgate_core::api::ports::ClaimedDelivery { delivery: claim.delivery.clone(), token }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_signing_secret_is_created_once_per_account_even_when_created_concurrently() {
    let pool = runtime().await;
    let outbox = Arc::new(PgCallbackOutbox::new(pool.clone()));
    let account = AccountId(Uuid::new_v4());
    let tasks: Vec<_> = (0..8)
        .map(|n| {
            let outbox = outbox.clone();
            tokio::spawn(async move { outbox.signing_secret(account, &format!("candidate-{n}")).await.unwrap() })
        })
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for task in tasks {
        seen.insert(task.await.unwrap());
    }
    assert_eq!(seen.len(), 1, "every caller gets the one stored secret: {seen:?}");
    assert_eq!(outbox.signing_secret(account, "later").await.unwrap(), seen.into_iter().next().unwrap());
    let other = AccountId(Uuid::new_v4());
    assert_eq!(outbox.signing_secret(other, "mine").await.unwrap(), "mine", "secrets are per account");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn account_erasure_removes_every_api_row_of_that_account_only() {
    let _serial = OUTBOX.lock().await;
    let pool = runtime().await;
    let owner = owner().await;
    let (account, bystander) = (AccountId(Uuid::new_v4()), AccountId(Uuid::new_v4()));
    let outbox = PgCallbackOutbox::new(pool.clone());
    let limiter = PgApiRateLimiter::new(pool.clone());
    let mine = key(account, NOW);
    let theirs = key(bystander, NOW);
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    for k in [&mine, &theirs] {
        uow.insert_key(k).await.unwrap();
        let record = IdempotencyRecord { account: k.account, key: "k".into(), request_digest: "d".into(), request_type: "jobIds".into(), jobs: vec![], answer: None, created_at: NOW };
        uow.insert_idempotency(&record, expired_before(NOW)).await.unwrap();
    }
    uow.commit().await.unwrap();
    let (mine_event, their_event) = (i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs(), i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs());
    let (my_delivery, their_delivery) = (due_delivery(&pool, account, mine_event, NOW).await, due_delivery(&pool, bystander, their_event, NOW).await);
    for k in [&mine, &theirs] {
        limiter.consume(k.id, RateWindows::at(NOW), RateLimits::default()).await.unwrap();
    }
    let claims = outbox.claim_deliveries(1000, 60, NOW).await.unwrap();
    for d in [&my_delivery, &their_delivery] {
        let claim = claims.iter().find(|c| c.delivery.id == d.id).expect("both are due and were claimed together");
        assert!(outbox.begin_attempt(claim, &attempt(d, 1, NOW), &json!({})).await.unwrap());
    }
    outbox.signing_secret(account, "mine").await.unwrap();
    outbox.signing_secret(bystander, "theirs").await.unwrap();

    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    JobUow::erase_account_rows(&mut uow, account).await.unwrap();
    uow.commit().await.unwrap();

    for (table, column) in [("api_keys", "account_id"), ("idempotency", "account_id"), ("callback_deliveries", "account_id"), ("signing_secrets", "account_id")] {
        let sql: &'static str = Box::leak(format!("SELECT count(*) FROM transgate_api.{table} WHERE {column} = $1").into_boxed_str());
        assert_eq!(count(&owner, sql, account.0).await, 0, "{table} of the erased account");
        assert_eq!(count(&owner, sql, bystander.0).await, 1, "{table} of the other account is untouched");
    }
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_attempts WHERE delivery_id = $1", my_delivery.id.0).await, 0, "attempts go with their delivery");
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_attempts WHERE delivery_id = $1", their_delivery.id.0).await, 1);
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.rate_windows WHERE key_id = $1", mine.id.0).await, 0, "rate windows go with their key");
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.rate_windows WHERE key_id = $1", theirs.id.0).await, 2);
}

// Runtime verification covers the new schema in the same pattern as transgate_jobs.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn runtime_verification_flags_a_role_that_can_edit_what_the_api_schema_forbids() {
    let pool = runtime().await;
    let owner = owner().await;
    assert_eq!(transgate_jobs_postgres::runtime_authority_problems(&pool, transgate_jobs_postgres::RUNTIME_ROLE).await.unwrap(), Vec::<String>::new());
    let role = format!("p1t_api_probe_{}", Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE ROLE \"{role}\" NOLOGIN IN ROLE billing_runtime"))).execute(&owner).await.unwrap();
    for grant in [
        format!("GRANT UPDATE (account_id) ON transgate_api.api_keys TO \"{role}\""),
        format!("GRANT UPDATE (key_id) ON transgate_api.rate_windows TO \"{role}\""),
        format!("GRANT TRUNCATE ON transgate_api.callback_attempts TO \"{role}\""),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(grant)).execute(&owner).await.unwrap();
    }
    let mut problems = transgate_jobs_postgres::runtime_authority_problems(&pool, &role).await.unwrap();
    problems.sort();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP OWNED BY \"{role}\""))).execute(&owner).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE \"{role}\""))).execute(&owner).await.unwrap();
    assert_eq!(
        problems,
        vec![
            "can TRUNCATE transgate_api.callback_attempts".to_string(),
            "can UPDATE transgate_api.api_keys.account_id".to_string(),
            "can UPDATE transgate_api.rate_windows.key_id".to_string(),
        ]
    );
}

async fn stored_payload(owner: &PgPool, delivery: DeliveryId) -> Option<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM transgate_api.callback_deliveries WHERE id = $1").bind(delivery.0).fetch_one(owner).await.unwrap()
}

// D10 (1): retention deletes the lines a stored callback body copied, so the body goes in the same
// transaction. Only deliveries of the listed jobs are touched; the delivery itself stays.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn retention_clears_the_stored_callback_body_of_exactly_the_jobs_whose_lines_it_deleted() {
    let pool = runtime().await;
    let owner = owner().await;
    let account = AccountId(Uuid::new_v4());
    let (e1, e2) = (i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs(), i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs());
    let (listed, unlisted) = (due_delivery(&pool, account, e1, NOW).await, due_delivery(&pool, account, e2, NOW).await);
    assert!(stored_payload(&owner, listed.id).await.is_some());
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    JobUow::delete_lines_of(&mut uow, &[listed.job]).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(stored_payload(&owner, listed.id).await, None);
    assert!(stored_payload(&owner, unlisted.id).await.is_some(), "another job's body is untouched");
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_deliveries WHERE id = $1", listed.id.0).await, 1, "the delivery stays");

    // A rolled-back retention run clears nothing.
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    JobUow::delete_lines_of(&mut uow, &[unlisted.job]).await.unwrap();
    <PgJobUow as JobUow>::rollback(Box::new(uow)).await.unwrap();
    assert!(stored_payload(&owner, unlisted.id).await.is_some());
}

// D10 (2): the consent purge clears every stored body of the account (they may carry the
// consent-gated summary and deliveries have no epoch), and no other account's.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_consent_purge_clears_every_stored_callback_body_of_the_account_only() {
    use transgate_core::ai::ports::AiTx;
    let pool = runtime().await;
    let owner = owner().await;
    let (account, bystander) = (AccountId(Uuid::new_v4()), AccountId(Uuid::new_v4()));
    let event = || i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs();
    let (a, b) = (due_delivery(&pool, account, event(), NOW).await, due_delivery(&pool, account, event(), NOW).await);
    let theirs = due_delivery(&pool, bystander, event(), NOW).await;
    let mut uow = PgJobUow::begin(&pool).await.unwrap();
    let removed = AiTx::purge_ai_artifacts(&mut uow, account, 0).await.unwrap();
    assert_eq!(removed, 0, "the AI row count is unchanged by the payload clearing");
    uow.commit().await.unwrap();
    for d in [&a, &b] {
        assert_eq!(stored_payload(&owner, d.id).await, None);
        assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_deliveries WHERE id = $1", d.id.0).await, 1, "deliveries are kept");
    }
    assert!(stored_payload(&owner, theirs.id).await.is_some(), "another account's body is untouched");
}

// D8: reopening an ended delivery makes it due again under the same id with its attempts kept; a
// pending or unknown delivery is left alone.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn reopen_makes_an_ended_delivery_due_again_keeping_its_id_and_attempts() {
    let _serial = OUTBOX.lock().await;
    let pool = runtime().await;
    let owner = owner().await;
    let outbox = PgCallbackOutbox::new(pool.clone());
    let account = AccountId(Uuid::new_v4());
    let event = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap()).abs();
    let d = due_delivery(&pool, account, event, NOW).await;
    assert_eq!(outbox.reopen(d.id, NOW).await.unwrap(), Reopen::StillPending, "a pending delivery is not reopened");
    assert_eq!(outbox.reopen(DeliveryId(Uuid::new_v4()), NOW).await.unwrap(), Reopen::Missing, "an unknown id is missing");

    let claim = outbox.claim_deliveries(1000, 60, NOW).await.unwrap().into_iter().find(|c| c.delivery.id == d.id).unwrap();
    assert!(outbox.begin_attempt(&claim, &attempt(&d, 1, NOW), &json!({})).await.unwrap());
    let mut finished = attempt(&d, 1, NOW);
    finished.outcome = Some(SendOutcome::Status { code: 404, excerpt: String::new() });
    finished.finished_at = Some(NOW);
    let failed = Settlement { state: DeliveryState::Failed, next_attempt_at: None, delivered_at: None };
    assert!(outbox.finish_attempt(&claim, &finished, &failed).await.unwrap());
    assert!(outbox.claim_deliveries(1000, 60, Micros(NOW.0 + 86_400 * SECOND)).await.unwrap().iter().all(|c| c.delivery.id != d.id));

    let later = Micros(NOW.0 + 3_600 * SECOND);
    assert_eq!(outbox.reopen(d.id, later).await.unwrap(), Reopen::Reopened);
    assert_eq!(outbox.reopen(d.id, later).await.unwrap(), Reopen::StillPending, "now pending, so a second reopen changes nothing");
    let (state, attempts, next): (String, i32, i64) = sqlx::query_as(
        "SELECT state, attempts, transgate_jobs.ts_to_us(next_attempt_at) FROM transgate_api.callback_deliveries WHERE id = $1",
    ).bind(d.id.0).fetch_one(&owner).await.unwrap();
    assert_eq!((state.as_str(), attempts, next), ("pending", 1, later.0));
    let again = outbox.claim_deliveries(1000, 60, later).await.unwrap().into_iter().find(|c| c.delivery.id == d.id).expect("due at the reopen time");
    assert_eq!(again.delivery.attempts, 1, "the next attempt continues the count");
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_api.callback_attempts WHERE delivery_id = $1", d.id.0).await, 1, "the old attempt record is kept");
}
