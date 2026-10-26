//! Real JWT verification, contact adoption, billing + jobs HTTP, and the PostgreSQL erasure cascade.
//! External legacy/identity ports are controlled so the owner must continue after contact removal.

use std::sync::atomic::{AtomicBool, Ordering};

use billing_core::types::{AccountId, BoxFuture};
use billing_postgres::PgBillingStore;
use media_ports::{CaptionDocument, CaptionError, ObjectInventory, ObjectRef, PublicCaptions, StoreKind, TransportError};
use transgate_core::jobs::app::{Jobs, JobsConfig, JobsServices};
use transgate_core::jobs::domain::StorageProvider;
use transgate_core::jobs::ports::{IdentityAdmin, IdentityOutcome, JobStore, Locks, RateLimiter, Sleeper};
use transgate_core::jobs::routing::{ModelConfig, RoutingConfig};
use transgate_core::privacy::app::{Privacy, PrivacyServices};
use transgate_core::privacy::erasure::ExceptionPolicy;
use transgate_core::public_transcript::PublicTranscript;
use transgate_http::TransgateHttp;
use transgate_jobs_postgres::{PgJobStore, PgLocks, PgRateLimiter};

use super::*;

#[derive(Default)]
struct LegacyInventory {
    unavailable: AtomicBool,
    owners: Mutex<Vec<String>>,
}

impl ObjectInventory for LegacyInventory {
    fn kind(&self) -> StoreKind { StoreKind::Supabase }
    fn bucket(&self) -> &str { "jobs" }
    fn inventory<'a>(&'a self, owner: &'a str) -> BoxFuture<'a, Result<Vec<ObjectRef>, TransportError>> {
        Box::pin(async move {
            self.owners.lock().unwrap().push(owner.to_string());
            if self.unavailable.load(Ordering::SeqCst) {
                Err(TransportError::Unavailable { status: Some(503), operation: "legacy inventory".into() })
            } else {
                Ok(vec![])
            }
        })
    }
}

#[derive(Default)]
struct IdentityDeletion(Mutex<Vec<AccountId>>);

impl IdentityAdmin for IdentityDeletion {
    fn delete_user(&self, account: AccountId) -> BoxFuture<'_, Result<IdentityOutcome, String>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(account);
            Ok(IdentityOutcome::Deleted)
        })
    }
}

pub(super) struct RuntimeSleeper;
impl Sleeper for RuntimeSleeper {
    fn sleep(&self, millis: u64) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(millis)))
    }
}

struct NoPublicCaptions;
impl PublicCaptions for NoPublicCaptions {
    fn fetch<'a>(&'a self, _: &'a str, _: Option<&'a str>) -> BoxFuture<'a, Result<CaptionDocument, CaptionError>> {
        Box::pin(async { Err(CaptionError::Network) })
    }
}

/// Mount the real jobs/privacy routes alongside the composed billing routes on this fixture's DB.
/// No upload is issued: this scenario can close after legacy inventory and identity are confirmed.
async fn app_with_privacy(clock: Arc<dyn Clock>, legacy: Arc<LegacyInventory>, identity: Arc<IdentityDeletion>) -> App {
    let mut app = start_in(clock.clone(), runtime_fixture().await).await;
    transgate_jobs_postgres::migrate(&app.owner).await.unwrap();
    transgate_jobs_postgres::verify_runtime(&app.transgate.pool).await.unwrap();
    let pool = app.transgate.pool.clone();
    let store: Arc<dyn JobStore> = Arc::new(PgJobStore::new(pool.clone()));
    let locks: Arc<dyn Locks> = Arc::new(PgLocks::new(pool.clone()));
    let limiter: Arc<dyn RateLimiter> = Arc::new(PgRateLimiter::new(pool.clone()));
    let jobs = Arc::new(Jobs::new(
        JobsServices {
            store: store.clone(), clock: clock.clone(), objects: vec![], transcribers: vec![],
            stager: None, sleeper: Arc::new(RuntimeSleeper), rate_limiter: limiter.clone(),
            paid_captions: None, video_metadata: None,
        },
        JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: 200 },
            models: ModelConfig { soniox: "stt-async-v5".into(), soniox_v5: "stt-async-v5".into(), deepgram: "whisper-large".into(), deepgram_account_overrides: vec![] },
            upload_storage: StorageProvider::Gcs, upload_bucket: "uploads".into(),
            archive_store: StoreKind::Gcs, archive_bucket: "archive".into(),
            staging_store: StoreKind::Gcs, staging_bucket: "staging".into(),
            callback_base_url: "https://transgate.example.test".into(), soniox_auto_delete: true,
            ssrf_protection: true, freemium_lists: Default::default(),
        },
    ));
    let privacy = Arc::new(Privacy::new(PrivacyServices {
        store: store.clone(), clock: clock.clone(), locks, objects: vec![], inventories: vec![legacy],
        transcribers: vec![], identity, billing: Arc::new(PgBillingStore::new(pool.clone())),
        terminators: vec![], exceptions: ExceptionPolicy::default(),
    }));
    let account_router = account::router(&app, privacy.clone(), store, clock.clone());
    let state = Arc::new(TransgateHttp {
        ai: None,
        jobs, privacy, public_transcript: Arc::new(PublicTranscript::new(Arc::new(NoPublicCaptions), limiter, clock.clone())),
        queries: Arc::new(PgJobStore::new(pool)), transcribers: vec![],
        identity: app.transgate.identity.clone(), clock, cron_secret: None, downloads: None,
    });
    let server = serve(app.transgate.router.clone().merge(transgate_http::router(state)).merge(account_router)).await;
    app.base = server.url.clone();
    app._servers.push(server);
    app
}

pub(super) fn register_auth(app: &App, person: &User) {
    app.auth.0.lock().unwrap().users.insert(person.id.to_string(), auth_user(
        person, &person.email, "2026-01-10T09:00:00Z", "2026-01-10T09:00:00Z", true,
    ));
}

async fn contact_count(app: &App, person: &User) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transgate_billing.customers WHERE account_id = $1")
        .bind(person.id).fetch_one(&app.owner).await.unwrap()
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn stale_jwt_can_continue_only_its_own_erasure_after_contact_cascade_without_restoring_admin_authority() {
    let clock = Arc::new(OffsetClock::default());
    let legacy = Arc::new(LegacyInventory::default());
    legacy.unavailable.store(true, Ordering::SeqCst);
    let deleted = Arc::new(IdentityDeletion::default());
    let app = app_with_privacy(clock.clone(), legacy.clone(), deleted.clone()).await;
    let admin_id = Uuid::new_v4();
    let admin = User { id: admin_id, email: ADMIN.into(), jwt: token(&admin_id, ADMIN, 3600) };
    let other = user();
    register_auth(&app, &admin);
    register_auth(&app, &other);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&admin.jwt), None).await.0, 200);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&other.jwt), None).await.0, 200);
    assert_eq!(app.call("POST", "/api/billing/admin/customer-invoices", Some(&admin.jwt), Some(json!({"email": other.email}))).await.0, 200);

    let (status, body) = app.call("DELETE", "/api/delete-account", Some(&admin.jwt), None).await;
    assert_eq!((status, body["status"].clone()), (202, json!("local_data_erased")), "{body}");
    assert_eq!(contact_count(&app, &admin).await, 0, "the real local cascade removed the contact");
    assert!(deleted.0.lock().unwrap().is_empty(), "legacy phase zero must finish before identity deletion");
    let directory_calls = app.auth.0.lock().unwrap().requests.len();

    let (status, body) = app.call("GET", "/api/jobs", Some(&admin.jwt), None).await;
    assert_eq!((status, body["code"].clone()), (410, json!("account_deletion_in_progress")), "{body}");
    for (path, body) in [
        ("/api/billing/admin/customer-invoices", json!({"email": other.email})),
        ("/api/billing/admin/erasure/checkouts/1/resolve", json!({
            "checkout_cannot_complete": true, "no_renewing_subscription": true,
            "evidence_ref": "fixture", "rationale": "must never authorize this erased administrator",
        })),
    ] {
        let (status, body) = app.call("POST", path, Some(&admin.jwt), Some(body)).await;
        assert_eq!((status, body["error"]["code"].clone()), (410, json!("account_erased")), "{path}: {body}");
    }
    // Knowing a barred subject or replaying its email does not replace signature/expiry validation.
    let forged_claims = json!({"sub": admin.id.to_string(), "email": ADMIN, "aud": "authenticated", "exp": now_micros() / 1_000_000 + 3600});
    let forged = encode(&Header::new(Algorithm::HS256), &forged_claims, &EncodingKey::from_secret(b"wrong-secret")).unwrap();
    let expired = token(&admin.id, ADMIN, -3600);
    for jwt in [&forged, &expired] {
        assert_eq!(app.call("GET", "/api/jobs", Some(jwt), None).await.0, 401);
        assert_eq!(app.call("DELETE", "/api/delete-account", Some(jwt), None).await.0, 401);
    }

    legacy.unavailable.store(false, Ordering::SeqCst);
    clock.advance(900); // Pass the recorded legacy retry deadline, not a sleep or polling loop.
    let (status, body) = app.call("DELETE", "/api/delete-account", Some(&admin.jwt), Some(json!({"account_id": other.id}))).await;
    assert_eq!((status, body["status"].clone()), (202, json!("local_data_erased")), "the owner advances the empty legacy listing: {body}");
    assert!(deleted.0.lock().unwrap().is_empty(), "identity is the next phase, not legacy's phase");
    let (status, body) = app.call("DELETE", "/api/delete-account", Some(&admin.jwt), None).await;
    assert_eq!((status, body["status"].clone()), (200, json!("fully_erased")), "the next owner request advances identity: {body}");
    assert_eq!(*deleted.0.lock().unwrap(), vec![AccountId(admin.id)], "the body cannot select another account");
    assert!(legacy.owners.lock().unwrap().iter().all(|id| id == &admin.id.to_string()));
    assert_eq!(app.auth.0.lock().unwrap().requests.len(), directory_calls, "neither ordinary refusal nor self-erasure can re-adopt contacts behind the barrier");
    assert_eq!(contact_count(&app, &admin).await, 0);
    assert_eq!(app.call("GET", "/api/jobs", Some(&other.jwt), None).await.0, 200, "the other owner remains usable");
    assert_eq!(app.call("GET", "/api/jobs", Some(&admin.jwt), None).await.0, 410, "the permanent barrier also applies after completion");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn deletion_as_the_first_request_still_adopts_unbarred_contact_and_a_real_directory_outage_stays_503() {
    let app = app_with_privacy(Arc::new(OffsetClock::default()), Arc::new(LegacyInventory::default()), Arc::new(IdentityDeletion::default())).await;
    let person = user();
    register_auth(&app, &person);
    assert_eq!(contact_count(&app, &person).await, 0);
    app.auth.0.lock().unwrap().failing.insert(person.id.to_string());
    let (status, body) = app.call("DELETE", "/api/delete-account", Some(&person.jwt), None).await;
    assert_eq!((status, body["code"].clone()), (503, json!("identity_unavailable")), "{body}");
    assert_eq!(app.call("GET", "/api/jobs", Some(&person.jwt), None).await.0, 503);
    let barriers: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.erasure_barriers WHERE account_id = $1")
        .bind(person.id).fetch_one(&app.owner).await.unwrap();
    assert_eq!(barriers, 0, "an unbarred directory outage must not start deletion");
    assert_eq!(contact_count(&app, &person).await, 0);

    // A different, healthy user has never made any app request or been locally provisioned.
    let first_call = user();
    register_auth(&app, &first_call);
    let lookups_before = app.auth.0.lock().unwrap().requests.len();
    let (status, body) = app.call("DELETE", "/api/delete-account", Some(&first_call.jwt), None).await;
    assert_eq!((status, body["status"].clone()), (202, json!("local_data_erased")), "{body}");
    let contact_lookups = app.auth.0.lock().unwrap().requests.len();
    assert_eq!(contact_lookups, lookups_before + 1, "an unbarred first-call deletion still confirms contact with the trusted directory");
    assert_eq!(contact_count(&app, &first_call).await, 0, "the freshly adopted contact was removed by the cascade");
    assert_eq!(app.call("GET", "/api/jobs", Some(&first_call.jwt), None).await.0, 410);
    let (status, body) = app.call("DELETE", "/api/delete-account", Some(&first_call.jwt), None).await;
    assert_eq!((status, body["status"].clone()), (200, json!("fully_erased")), "continuation no longer needs the erased contact: {body}");
    assert_eq!(app.auth.0.lock().unwrap().requests.len(), contact_lookups);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn exception_approval_requires_an_unbarred_admin_before_touching_core_and_records_only_verified_operator() {
    let clock = Arc::new(OffsetClock::default());
    let legacy = Arc::new(LegacyInventory::default());
    legacy.unavailable.store(true, Ordering::SeqCst);
    let app = app_with_privacy(clock, legacy, Arc::new(IdentityDeletion::default())).await;
    let (owner, ordinary) = (user(), user());
    register_auth(&app, &owner);
    register_auth(&app, &ordinary);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&owner.jwt), None).await.0, 200);
    assert_eq!(app.call("DELETE", "/api/delete-account", Some(&owner.jwt), None).await.0, 202);
    let erasure: Uuid = sqlx::query_scalar("SELECT id FROM transgate_jobs.erasures WHERE account_id = $1")
        .bind(owner.id).fetch_one(&app.owner).await.unwrap();
    let item = Uuid::new_v4();
    sqlx::query("INSERT INTO transgate_jobs.erasure_items (id,erasure_id,target,state,attempts) VALUES ($1,$2,$3,$4,0)")
        .bind(item).bind(erasure)
        .bind(sqlx::types::Json(json!({"kind":"upload_resweep","object":{"store":"gcs","bucket":"uploads","key":format!("{}/late",owner.id),"version":null},"not_before":0})))
        .bind(sqlx::types::Json(json!({"state":"unresolved","reason":"accepted PUT may still commit","next_at":0})))
        .execute(&app.owner).await.unwrap();
    let path = format!("/api/admin/erasures/{erasure}/items/{item}/exception");
    let before: Value = sqlx::query_scalar("SELECT state FROM transgate_jobs.erasure_items WHERE id = $1")
        .bind(item).fetch_one(&app.owner).await.unwrap();
    let (status, _) = app.call("POST", &path, Some(&ordinary.jwt), Some(json!({"rationale":"not authorized"}))).await;
    assert_eq!(status, 403);
    // Even malformed approval data and a nonexistent core identity are not evaluated for this actor.
    let nonexistent = format!("/api/admin/erasures/{}/items/{}/exception", Uuid::new_v4(), Uuid::new_v4());
    let response = app.http.post(format!("{}{}", app.base, nonexistent)).bearer_auth(&ordinary.jwt).body("not JSON").send().await.unwrap();
    assert_eq!(response.status().as_u16(), 403);
    let after: Value = sqlx::query_scalar("SELECT state FROM transgate_jobs.erasure_items WHERE id = $1")
        .bind(item).fetch_one(&app.owner).await.unwrap();
    assert_eq!(after, before, "the non-admin cannot reach core approval or mutate the item");

    let admin_id = Uuid::new_v4();
    let admin = User { id: admin_id, email: ADMIN.into(), jwt: token(&admin_id, ADMIN, 3600) };
    register_auth(&app, &admin);
    assert_eq!(app.call("POST", &path, Some(&admin.jwt), Some(json!({"rationale":"reviewed","operator":"body-supplied operator"}))).await.0, 400);
    assert_eq!(app.call("POST", &path, Some(&admin.jwt), Some(json!({"rationale":"operator accepts the explicitly documented outstanding PUT risk"}))).await.0, 200);
    let approved: Value = sqlx::query_scalar("SELECT state FROM transgate_jobs.erasure_items WHERE id = $1")
        .bind(item).fetch_one(&app.owner).await.unwrap();
    assert_eq!(approved["state"], "approved_exception");
    assert_eq!(approved["approved_by"], ADMIN);
    assert_eq!(approved["reason"], "operator accepts the explicitly documented outstanding PUT risk");
}

#[path = "account.rs"]
mod account;
