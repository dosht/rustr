//! The scheduler's overlap lock against a real database, through the restricted billing runtime
//! login the server uses.
//!
//! Run with `TRANSGATE_SERVER_TEST_DATABASE_URL=... cargo test -p transgate-server -- --include-ignored`.
//! The URL names a disposable database with owner authority; a missing variable fails the test, it
//! never skips. The test seeds its own account and uses slot times in the far future, so a rerun
//! never meets a slot an earlier run finished.

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use billing_core::ports::{MailError, MailMessage, Mailer};
use billing_core::types::{AccountId, BoxFuture, Clock, Micros};
use billing_postgres::PgBillingStore;
use media_ports::{
    CallbackBinding, CallbackError, DeleteOutcome, Headers, ObjectRef, ProviderObservation, RemoteIdentity, RemoteRecord, SecretString, StoreKind,
    Submission, Transcriber, TranscriptionRequest, TransportError,
};
use sqlx::PgPool;
use tokio::sync::{Notify, OnceCell};
use transgate_billing::directory::PgCustomerDirectory;
use transgate_core::ids::{AttemptId, MICROS_PER_MINUTE};
use transgate_core::jobs::app::{Jobs, JobsConfig, JobsServices};
use transgate_core::jobs::domain::{Attempt, AttemptRequest, AttemptState, JobSource, JobStatus, NewJob, Provider, RemoteCleanup, StorageProvider};
use transgate_core::jobs::ports::{IdentityAdmin, IdentityOutcome, JobStore, Locks, RateLimiter, Sleeper};
use transgate_core::jobs::routing::{ModelConfig, RoutingConfig};
use transgate_core::languages::Language;
use transgate_core::privacy::app::{Privacy, PrivacyServices};
use transgate_core::privacy::erasure::ExceptionPolicy;
use transgate_jobs_postgres::{PgJobStore, PgLocks, PgRateLimiter};
use uuid::Uuid;

use super::*;
use crate::ports::TokioSleeper;

const LOGIN: &str = "transgate_server_test_runtime";
const PASSWORD: &str = "transgate-server-test-runtime-password";
const SECOND: i64 = 1_000_000;

fn database_url() -> String {
    std::env::var("TRANSGATE_SERVER_TEST_DATABASE_URL")
        .expect("TRANSGATE_SERVER_TEST_DATABASE_URL must name a disposable PostgreSQL database for these tests")
}

static PROVISIONED: OnceCell<()> = OnceCell::const_new();

/// The server's pool: billing's verified runtime login, then the job crate's check.
async fn runtime() -> PgPool {
    let owner = billing_postgres::connect_owner(&database_url()).await.expect("owner connection");
    PROVISIONED
        .get_or_init(|| async {
            billing_postgres::migrate(&owner).await.expect("billing migrations");
            transgate_jobs_postgres::migrate(&owner).await.expect("jobs migrations");
            billing_postgres::provision_runtime_login(&owner, LOGIN, PASSWORD).await.expect("runtime login");
        })
        .await;
    let options = billing_postgres::options_for_login(&database_url(), LOGIN, PASSWORD).unwrap();
    let pool = billing_postgres::connect_runtime(options, 8).await.expect("restricted runtime pool");
    transgate_jobs_postgres::verify_runtime(&pool).await.expect("restricted transgate runtime");
    pool
}
struct Database(String);
impl Drop for Database {
    fn drop(&mut self) {
        let (name, url) = (self.0.clone(), database_url());
        let _ = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            runtime.block_on(async move {
                if let Ok(owner) = billing_postgres::connect_owner(&url).await {
                    let _ = sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))).execute(&owner).await;
                    owner.close().await;
                }
            });
        }).join();
    }
}
async fn isolated() -> (PgPool, Database) {
    // Existing fixture owns cluster-wide login provisioning exactly once.
    runtime().await.close().await;
    let base = database_url();
    let owner = billing_postgres::connect_owner(&base).await.unwrap();
    let name = format!("transgate_ai_mail_{}", Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{name}\""))).execute(&owner).await.unwrap();
    owner.close().await;
    let guard = Database(name.clone());
    let url = format!("{}/{}", base.rsplit_once('/').unwrap().0, name);
    let owner = billing_postgres::connect_owner(&url).await.unwrap();
    billing_postgres::migrate(&owner).await.unwrap();
    // Mail uses the real host customer directory, not only the billing-core schema.
    transgate_billing::migrate_host(&owner).await.expect("billing customer-directory migrations");
    transgate_jobs_postgres::migrate(&owner).await.unwrap();
    owner.close().await;
    let pool = billing_postgres::connect_runtime(billing_postgres::options_for_login(&url, LOGIN, PASSWORD).unwrap(), 8).await.unwrap();
    transgate_jobs_postgres::verify_runtime(&pool).await.unwrap();
    (pool, guard)
}

/// The instant both workers read, moved by the test.
struct Injected(AtomicI64);

impl Clock for Injected {
    fn now(&self) -> Micros {
        Micros(self.0.load(Ordering::SeqCst))
    }
}

/// A Soniox transport whose listing is the provider I/O the first pass sits in: the first
/// `find_by_reference` signals `entered` and waits for `release`; every later one answers at once.
struct HeldListing {
    calls: AtomicUsize,
    gate: Mutex<Option<(Arc<Notify>, Arc<Notify>)>>,
}

impl HeldListing {
    fn new(entered: Arc<Notify>, release: Arc<Notify>) -> HeldListing {
        HeldListing { calls: AtomicUsize::new(0), gate: Mutex::new(Some((entered, release))) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn unused<T: Send + 'static>() -> BoxFuture<'static, Result<T, TransportError>> {
    Box::pin(async { Err(TransportError::Unsupported("not used by this test".into())) })
}

impl Transcriber for HeldListing {
    fn provider(&self) -> media_ports::Provider {
        media_ports::Provider::Soniox
    }
    fn project(&self) -> &str {
        "project"
    }
    fn submit<'a>(&'a self, _request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        unused()
    }
    fn status<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        unused()
    }
    fn result<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        unused()
    }
    fn delete_remote<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        unused()
    }
    fn find_by_reference<'a>(&'a self, _reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let gate = self.gate.lock().unwrap().take();
            if let Some((entered, release)) = gate {
                entered.notify_one();
                release.notified().await;
            }
            Ok(vec![])
        })
    }
    fn verify_callback(&self, _binding: &CallbackBinding, _headers: &Headers, _body: &[u8]) -> Result<ProviderObservation, CallbackError> {
        Err(CallbackError::Unauthorized)
    }
    fn complete_callback(&self, observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        Box::pin(async move { Ok(observation) })
    }
}

struct NoMail;

impl Mailer for NoMail {
    fn send<'a>(&'a self, _message: &'a MailMessage, _idempotency_key: &'a str) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async { Ok(()) })
    }
}

struct NoIdentity;

impl IdentityAdmin for NoIdentity {
    fn delete_user(&self, _account: AccountId) -> BoxFuture<'_, Result<IdentityOutcome, String>> {
        Box::pin(async { Ok(IdentityOutcome::Absent) })
    }
}

/// One replica: its own core services, its own lock connections, the shared database, clock and transport.
fn replica(pool: &PgPool, clock: Arc<dyn Clock>, transcriber: Arc<HeldListing>) -> Arc<Workers> {
    let store = PgJobStore::new(pool.clone());
    let job_store: Arc<dyn JobStore> = Arc::new(store.clone());
    let locks: Arc<dyn Locks> = Arc::new(PgLocks::new(pool.clone()));
    let limiter: Arc<dyn RateLimiter> = Arc::new(PgRateLimiter::new(pool.clone()));
    let sleeper: Arc<dyn Sleeper> = Arc::new(TokioSleeper);
    let ai = crate::ai::compose(crate::ai::Settings::from_env(&|_| None).unwrap(), job_store.clone(), clock.clone(), sleeper.clone()).unwrap();
    let transcribers: Vec<Arc<dyn Transcriber>> = vec![transcriber];
    let jobs = Arc::new(Jobs::new(
        JobsServices {
            store: job_store.clone(),
            clock: clock.clone(),
            objects: vec![],
            transcribers: transcribers.clone(),
            stager: None,
            sleeper,
            rate_limiter: limiter,
            paid_captions: None,
            video_metadata: None,
        },
        JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: 200 },
            models: ModelConfig { soniox: "stt-async-v5".into(), soniox_v5: "stt-async-v5".into(), deepgram: "whisper-large".into(), deepgram_account_overrides: vec![] },
            upload_storage: StorageProvider::Gcs,
            upload_bucket: "uploads".into(),
            archive_store: StoreKind::Gcs,
            archive_bucket: "archive".into(),
            staging_store: StoreKind::Gcs,
            staging_bucket: "staging".into(),
            callback_base_url: "https://transgate.example.test".into(),
            soniox_auto_delete: true,
            ssrf_protection: true,
            freemium_lists: Default::default(),
        },
    ));
    let privacy = Arc::new(Privacy::new(PrivacyServices {
        store: job_store,
        clock: clock.clone(),
        locks: locks.clone(),
        objects: vec![],
        inventories: vec![],
        transcribers,
        identity: Arc::new(NoIdentity),
        billing: Arc::new(PgBillingStore::new(pool.clone())),
        terminators: vec![],
        exceptions: ExceptionPolicy::default(),
    }));
    Arc::new(Workers { jobs, privacy, ai, store, pool: pool.clone(), mailer: Arc::new(NoMail), directory: PgCustomerDirectory::new(pool.clone()), clock, locks, callbacks: None })
}

fn object(key: &str) -> ObjectRef {
    ObjectRef { store: StoreKind::Gcs, bucket: "uploads".into(), key: key.into(), version: None }
}

/// A running job whose only attempt was claimed and never answered, its lease long expired: the
/// reconcile pass must ask the provider's listing about it.
async fn seed_unanswered_claim(store: &PgJobStore, claimed_at: Micros) {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    assert!(uow.billing().await.unwrap().create_account(account, claimed_at).await.unwrap());
    let job = uow
        .insert_job(&NewJob {
            account,
            status: JobStatus::Running,
            name: Some("interview.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: Some(object(&format!("{}/a.mp3", account.0))) },
            language: Language::from_code("en"),
            translate_to: vec![],
            speakers: true,
            fix_grammer: None,
            duration_minutes: Some(3),
            size_bytes: Some(1024),
            callback_url: None,
            whisper_params: None,
            created_at: claimed_at,
        })
        .await
        .unwrap();
    let claim = Attempt {
        id: AttemptId::new(),
        job: job.id,
        account,
        number: 1,
        provider: Provider::Soniox,
        project: "project".into(),
        model: "stt-async-v5".into(),
        state: AttemptState::Claimed,
        claimed_at,
        lease_until: Micros(claimed_at.0 + 2 * MICROS_PER_MINUTE),
        request: AttemptRequest {
            language_code: Some("en".into()),
            translation_target: None,
            diarize: true,
            audio: object("audio"),
            client_reference: format!("job_{}", job.id.0),
        },
        callback_secret: SecretString::new("per-attempt-secret"),
        remote_id: None,
        accepted_at: None,
        last_error: None,
        remote_cleanup: RemoteCleanup::NotRequested,
    };
    uow.insert_attempt(&claim).await.unwrap();
    let mut job = uow.lock_job(job.id).await.unwrap().unwrap();
    job.current_attempt = Some(claim.id);
    uow.save_job(&job, job.revision).await.unwrap();
    uow.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn a_jobs_pass_still_inside_provider_io_keeps_the_next_slot_unclaimed_until_it_ends() {
    let pool = runtime().await;
    // Slot times far in the future and random, so no earlier run or other schedule user owns them.
    let random = (Uuid::new_v4().as_u128() % 1_000_000) as i64;
    let first_slot = Schedule::Jobs.slot(Micros(1_900_000_000 * SECOND + random * 180 * SECOND));
    let first_time = Micros(first_slot.0 + 30 * SECOND);
    let next_slot = Schedule::Jobs.next_slot(first_time);
    let next_time = Micros(next_slot.0 + 30 * SECOND);
    assert_ne!(first_slot, next_slot, "the second pass lies in a DIFFERENT adjacent slot, the case the slot claim alone does not fence");
    assert_eq!(Schedule::Jobs.slot(next_time), next_slot);

    let clock = Arc::new(Injected(AtomicI64::new(first_time.0)));
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let provider = Arc::new(HeldListing::new(entered.clone(), release.clone()));
    let first = replica(&pool, clock.clone(), provider.clone());
    let second = replica(&pool, clock.clone(), provider.clone());
    seed_unanswered_claim(&PgJobStore::new(pool.clone()), Micros(first_time.0 - 10 * MICROS_PER_MINUTE)).await;

    // The first replica claims its slot and sits in the provider's listing.
    let running = tokio::spawn({
        let first = first.clone();
        async move { first.run_schedules(&[Schedule::Jobs]).await }
    });
    tokio::time::timeout(Duration::from_secs(60), entered.notified()).await.expect("the first pass reached the provider listing");
    assert_eq!(provider.calls(), 1);
    let held = media_postgres::slot_state(&pool, Schedule::Jobs.name(), first_slot.0).await.unwrap().expect("the first replica claimed its slot");
    assert_eq!((held.finished_at_us, held.outcome), (None, None), "its pass is still running");

    // Time moves into the next slot while that pass is still running; a second replica ticks.
    clock.0.store(next_time.0, Ordering::SeqCst);
    let skipped = second.run_schedules(&[Schedule::Jobs]).await;
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(media_postgres::slot_state(&pool, Schedule::Jobs.name(), next_slot.0).await.unwrap(), None, "the next slot stays unclaimed while the first pass owns the lock");
    assert_eq!(provider.calls(), 1, "the second replica ran no pass: the provider heard nothing from it");

    // The first pass ends; its slot is finished.
    release.notify_one();
    running.await.unwrap();
    let finished = media_postgres::slot_state(&pool, Schedule::Jobs.name(), first_slot.0).await.unwrap().unwrap();
    assert!(finished.finished_at_us.is_some() && finished.outcome.is_some(), "{finished:?}");
    let calls_when_released = provider.calls();

    // With the lock free the second replica takes the next slot and runs its pass.
    second.run_schedules(&[Schedule::Jobs]).await;
    let taken = media_postgres::slot_state(&pool, Schedule::Jobs.name(), next_slot.0).await.unwrap().expect("the next slot is claimed once the lock is free");
    assert!(taken.finished_at_us.is_some(), "{taken:?}");
    assert!(provider.calls() > calls_when_released, "and its pass asked the provider about the unanswered claim");
}

#[path = "pg_tests/ai.rs"]
mod ai;

#[path = "pg_tests/support.rs"]
mod support;

#[path = "pg_tests/callbacks.rs"]
mod callbacks;

#[path = "pg_tests/schedule.rs"]
mod schedule;
