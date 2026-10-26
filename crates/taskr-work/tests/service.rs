//! taskr-work's use cases over real Postgres: the real store on the billing sample's runtime
//! login and database, the real billing module for the erasure saga, and in-process
//! implementations of the external ports (the object store and the two transcription
//! providers) standing in for the providers themselves. They record what the service sent and
//! answer from a script, so every lost answer and refusal is a deliberate one.
//!
//! Needs `TASKR_WORK_TEST_DATABASE_URL` naming a disposable PostgreSQL database whose user may
//! create roles and databases. `#[ignore]`d offline; with `--include-ignored` a missing
//! variable fails, it never skips.

use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use billing_core::erasure::open_account_erasure;
use billing_core::ports::{Customer, CustomerDirectory};
use billing_core::types::{AccountId, Micros as BillingMicros};
use billing_postgres::PgBillingStore;
use media_ports::{
    AccessMethod, BoxFuture, CallbackBinding, CallbackError, DeleteOutcome, Headers, ObjectMetadata, ObjectRef, ObjectStore, ObservationOrigin, Provider,
    ProviderObservation, ProviderPayload, PutObject, RemoteIdentity, RemoteRecord, SensitiveUrl, StoreKind, Submission, TemporaryAccess, Transcriber,
    TranscriptionRequest, TransportError,
};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};
use taskr_work::app::{CallbackOutcome, Config, ErasureView, Service, ServiceError};
use taskr_work::store::{Store, grant_runtime, migrate};
use taskr_work::work::{Applied, AttemptPhase, Micros, Status};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard, Notify, OnceCell};
use uuid::Uuid;

const LOGIN: &str = "taskr_work_test_billing_runtime";
const PASSWORD: &str = "taskr-work-test-runtime-password";
const SECOND: i64 = 1_000_000;
const DAY: i64 = 86_400 * SECOND;

fn url() -> String {
    std::env::var("TASKR_WORK_TEST_DATABASE_URL").expect("TASKR_WORK_TEST_DATABASE_URL must name a disposable PostgreSQL database")
}

static READY: OnceCell<()> = OnceCell::const_new();

/// The database is shared by every test in this file, so state that is global to it must not be read or
/// swept by two tests at once. Two things are global: the retention sweep (it purges every old
/// completed work, whoever made it) and the erasure queue (its order spans every account). Every test
/// that does either holds this guard for its whole length. All other tests stay on their own account:
/// they use the per-work and per-account entry points (`dispatch_work`, `recover_work`,
/// `erase_account`) and never `dispatch_pass`, `recovery_pass`, `retention_pass` or `erasure_pass`,
/// which would also run with that test's provider doubles against another test's rows. Pool and role
/// provisioning happen once, inside `READY`.
static WORKERS: AsyncMutex<()> = AsyncMutex::const_new(());

async fn exclusive_workers() -> MutexGuard<'static, ()> {
    WORKERS.lock().await
}

/// The owner connection. The test database holds the billing module, the billing sample and the
/// audio-work sample together, as production does; the runtime acts as the billing runtime group.
async fn owner() -> PgPool {
    let owner = PgPoolOptions::new().max_connections(4).connect(&url()).await.expect("owner connection");
    READY
        .get_or_init(|| async {
            media_postgres::migrate(&owner).await.expect("media migration");
            migrate(&owner).await.expect("taskr-work migration");
            billing_postgres::migrate(&owner).await.expect("billing migration");
            taskr_billing::migrate_host(&owner).await.expect("billing sample migration");
            billing_postgres::provision_runtime_login(&owner, LOGIN, PASSWORD).await.expect("runtime login");
            grant_runtime(&owner, "billing_runtime").await.expect("taskr-work grants");
            media_postgres::grant_runtime(&owner, "billing_runtime").await.expect("media grants");
        })
        .await;
    owner
}

async fn runtime() -> PgPool {
    owner().await.close().await;
    let options = PgConnectOptions::from_str(&url()).unwrap().username(LOGIN).password(PASSWORD);
    PgPoolOptions::new().max_connections(6).connect_with(options).await.expect("runtime connection")
}

// ---- the object store double ------------------------------------------------------

#[derive(Default)]
struct Objects {
    /// key -> (size, generation): every PUT to a key makes a new generation, as GCS does.
    stored: Mutex<HashMap<String, (u64, u64)>>,
    generations: AtomicI64,
    deletes: Mutex<Vec<ObjectRef>>,
    /// What `delete` answers next (without touching the stored object); the real behaviour once empty.
    delete_answers: Mutex<VecDeque<DeleteOutcome>>,
}

impl Objects {
    /// The client's PUT to a signed upload link.
    fn upload(&self, object: &ObjectRef) {
        let generation = self.generations.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        self.stored.lock().unwrap().insert(object.key.clone(), (1024, generation));
    }
    fn holds(&self, object: &ObjectRef) -> bool {
        self.stored.lock().unwrap().contains_key(&object.key)
    }
}

impl ObjectStore for Objects {
    fn kind(&self) -> StoreKind {
        StoreKind::R2
    }
    fn bucket(&self) -> &str {
        "work-test"
    }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: Micros, ttl_seconds: u32) -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        Box::pin(async move {
            Ok(TemporaryAccess {
                object: object.clone(),
                method,
                url: SensitiveUrl::new(format!("https://objects.test/{}?sig=x", object.key)),
                expires_at: Micros(now.0 + i64::from(ttl_seconds) * SECOND),
                headers: vec![],
            })
        })
    }
    fn head<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        Box::pin(async move {
            let found = self.stored.lock().unwrap().get(&object.key).copied();
            Ok(found.map(|(size, generation)| ObjectMetadata {
                object: ObjectRef { version: Some(generation.to_string()), ..object.clone() },
                size,
                content_type: None,
                etag: None,
            }))
        })
    }
    fn put(&self, _request: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("the service never writes objects itself".into())) })
    }
    /// A delete naming a generation removes only that generation; none names the live object.
    fn delete<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async move {
            self.deletes.lock().unwrap().push(object.clone());
            if let Some(answer) = self.delete_answers.lock().unwrap().pop_front() {
                return Ok(answer);
            }
            let mut stored = self.stored.lock().unwrap();
            let live_generation = stored.get(&object.key).map(|(_, generation)| generation.to_string());
            Ok(match (live_generation, &object.version) {
                (None, _) => DeleteOutcome::Absent,
                (Some(live), Some(version)) if live != *version => DeleteOutcome::Absent,
                (Some(_), _) => {
                    stored.remove(&object.key);
                    DeleteOutcome::Deleted
                }
            })
        })
    }
}

// ---- a scripted provider ----------------------------------------------------------

type CallbackHold = (String, Arc<Notify>, Arc<Notify>);

struct Scripted {
    provider: Provider,
    secret_header: &'static str,
    answers: Mutex<VecDeque<Result<Submission, TransportError>>>,
    requests: Mutex<Vec<TranscriptionRequest>>,
    listing: Mutex<Result<Vec<RemoteRecord>, TransportError>>,
    deleted: Mutex<Vec<RemoteIdentity>>,
    /// What `delete_remote` answers next; `Deleted` once the script is empty.
    delete_answers: Mutex<VecDeque<DeleteOutcome>>,
    counter: AtomicI64,
    /// Pauses `complete_callback` for the observation of this remote id: it signals `entered`,
    /// then waits for `release`. This is the window between verification and application.
    hold: Mutex<Option<CallbackHold>>,
    /// Pauses the next `submit` after the request was recorded: the answer is held until `release`.
    submit_hold: Mutex<Option<(Arc<Notify>, Arc<Notify>)>>,
    /// What `result` answers: the recovery poll of a finished job. `Unsupported` while unset.
    result_answer: Mutex<Option<ProviderObservation>>,
    /// Makes this double's remote ids its own: `attempts_remote_identity` is unique per provider,
    /// project and id across the shared test database, so two tests must never mint the same id.
    scope: String,
}

impl Scripted {
    fn new(provider: Provider) -> Arc<Scripted> {
        Arc::new(Scripted {
            provider,
            secret_header: "x-webhook-secret",
            answers: Mutex::default(),
            requests: Mutex::default(),
            listing: Mutex::new(Ok(vec![])),
            deleted: Mutex::default(),
            delete_answers: Mutex::default(),
            counter: AtomicI64::new(0),
            hold: Mutex::new(None),
            submit_hold: Mutex::new(None),
            result_answer: Mutex::new(None),
            scope: Uuid::new_v4().simple().to_string(),
        })
    }
    fn answer(&self, answer: Result<Submission, TransportError>) {
        self.answers.lock().unwrap().push_back(answer);
    }
    fn sent(&self) -> Vec<TranscriptionRequest> {
        self.requests.lock().unwrap().clone()
    }
    fn remote(&self, id: &str) -> RemoteIdentity {
        RemoteIdentity { provider: self.provider, project: "project".into(), id: format!("{id}-{}", self.scope) }
    }
}

impl Transcriber for Scripted {
    fn provider(&self) -> Provider {
        self.provider
    }
    fn project(&self) -> &str {
        "project"
    }
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            let gate = self.submit_hold.lock().unwrap().take();
            if let Some((entered, release)) = gate {
                entered.notify_one();
                release.notified().await;
            }
            match self.answers.lock().unwrap().pop_front() {
                Some(answer) => answer,
                None => {
                    let n = self.counter.fetch_add(1, Ordering::SeqCst);
                    Ok(Submission::Accepted { remote: self.remote(&format!("remote-{}-{n}", Uuid::new_v4().simple())) })
                }
            }
        })
    }
    fn status<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("no status".into())) })
    }
    fn result<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async move {
            match self.result_answer.lock().unwrap().clone() {
                Some(observation) => Ok(observation),
                None => Err(TransportError::Unsupported("no result".into())),
            }
        })
    }
    fn delete_remote<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async move {
            self.deleted.lock().unwrap().push(remote.clone());
            Ok(self.delete_answers.lock().unwrap().pop_front().unwrap_or(DeleteOutcome::Deleted))
        })
    }
    fn find_by_reference<'a>(&'a self, _reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        Box::pin(async move { self.listing.lock().unwrap().clone() })
    }
    fn verify_callback(&self, binding: &CallbackBinding, headers: &Headers, body: &[u8]) -> Result<ProviderObservation, CallbackError> {
        let presented = headers.iter().find(|(name, _)| name == self.secret_header).map(|(_, v)| v.as_str());
        if presented != Some(binding.secret.expose()) {
            return Err(CallbackError::Unauthorized);
        }
        let observation: ProviderObservation = serde_json::from_slice(body).map_err(|_| CallbackError::Malformed)?;
        let wrong_remote = binding.expected_remote_id.as_deref().is_some_and(|id| id != observation.remote.id);
        if observation.remote.provider != binding.provider || observation.remote.project != binding.project || wrong_remote {
            return Err(CallbackError::IdentityMismatch);
        }
        Ok(observation)
    }
    fn complete_callback(&self, observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        Box::pin(async move {
            let gate = self.hold.lock().unwrap().as_ref().filter(|(id, _, _)| *id == observation.remote.id).map(|(_, entered, release)| (entered.clone(), release.clone()));
            if let Some((entered, release)) = gate {
                entered.notify_one();
                release.notified().await;
            }
            Ok(observation)
        })
    }
}

fn processing(remote: &RemoteIdentity) -> Vec<u8> {
    let payload = ProviderPayload::Soniox { reported: serde_json::from_value(json!({"id": remote.id, "status": "processing"})).unwrap(), transcript: None };
    serde_json::to_vec(&ProviderObservation { remote: remote.clone(), origin: ObservationOrigin::Webhook, payload }).unwrap()
}

fn completed(remote: &RemoteIdentity, text: &str) -> Vec<u8> {
    let payload = ProviderPayload::Soniox {
        reported: serde_json::from_value(json!({"id": remote.id, "status": "completed"})).unwrap(),
        transcript: Some(serde_json::from_value(json!({"id": remote.id, "tokens": [{"text": text}]})).unwrap()),
    };
    serde_json::to_vec(&ProviderObservation { remote: remote.clone(), origin: ObservationOrigin::Webhook, payload }).unwrap()
}

// ---- the harness ------------------------------------------------------------------

struct Harness {
    service: Service,
    objects: Arc<Objects>,
    soniox: Arc<Scripted>,
    deepgram: Arc<Scripted>,
    clock: Arc<AtomicI64>,
    account: Uuid,
    owner: PgPool,
    runtime: PgPool,
}

fn header(secret: &str) -> Headers {
    vec![("x-webhook-secret".into(), secret.into())]
}

async fn harness(configure: impl FnOnce(&mut Config)) -> Harness {
    let owner = owner().await;
    let runtime = runtime().await;
    let objects = Arc::new(Objects::default());
    let soniox = Scripted::new(Provider::Soniox);
    let deepgram = Scripted::new(Provider::Deepgram);
    // The wall clock, then moved by the tests.
    let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as i64;
    let clock = Arc::new(AtomicI64::new(start));
    let mut config = Config::new("https://taskr.example.test");
    config.retention_schedule = format!("test-{}", Uuid::new_v4().simple());
    configure(&mut config);
    let tick = clock.clone();
    let service = Service::new(
        Store::new(runtime.clone()),
        objects.clone(),
        vec![soniox.clone(), deepgram.clone()],
        vec![],
        config,
        Arc::new(move || Micros(tick.load(Ordering::SeqCst))),
    );
    let account = Uuid::new_v4();
    service.enroll(account, "someone@example.test").await.unwrap();
    Harness { service, objects, soniox, deepgram, clock, account, owner, runtime }
}

impl Harness {
    fn advance(&self, micros: i64) {
        self.clock.fetch_add(micros, Ordering::SeqCst);
    }

    fn now(&self) -> Micros {
        Micros(self.clock.load(Ordering::SeqCst))
    }

    /// Submits this work only: the shared database holds other tests' work, which a global pass would take.
    async fn dispatch(&self, id: Uuid) {
        self.service.dispatch_work(self.account, id).await.unwrap();
    }

    async fn recover(&self, id: Uuid) -> bool {
        self.service.recover_work(self.account, id).await.unwrap()
    }

    /// A work whose audio arrived and was confirmed.
    async fn uploaded(&self, provider: Option<Provider>) -> Uuid {
        let created = self.service.create(self.account, "clip.mp3", Some("en".into()), provider).await.unwrap();
        self.objects.upload(&created.work.object);
        self.service.confirm_upload(self.account, created.work.id).await.unwrap();
        created.work.id
    }
}

// ---- tests ------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn an_upload_is_looked_up_never_trusted_and_a_new_link_replaces_an_expired_one() {
    let h = harness(|_| {}).await;
    let created = h.service.create(h.account, "../../clip one.mp3", None, None).await.unwrap();
    assert_eq!(created.work.status, Status::AwaitingUpload);
    assert!(created.work.object.key.starts_with(&format!("work/{}/{}/", h.account, created.work.id)));
    assert!(!created.work.object.key.contains(".."));

    assert!(matches!(h.service.confirm_upload(h.account, created.work.id).await, Err(ServiceError::NotReady(_))), "no object, no confirmation");

    h.advance(1000 * SECOND);
    let again = h.service.reissue_upload(h.account, created.work.id).await.unwrap();
    assert!(again.expires_at.0 > created.upload.expires_at.0);

    h.objects.upload(&created.work.object);
    let work = h.service.confirm_upload(h.account, created.work.id).await.unwrap();
    assert_eq!(work.status, Status::Uploaded);
    assert!(work.object.version.is_some(), "the stored generation is recorded");
    assert!(matches!(h.service.reissue_upload(h.account, created.work.id).await, Err(ServiceError::NotReady(_))));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn work_goes_to_the_provider_with_its_stored_secret_and_a_callback_completes_it_exactly_once() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    h.dispatch(id).await;

    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!(work.status, Status::Running);
    let attempt = work.attempts.last().unwrap().clone();
    assert_eq!((attempt.provider, attempt.phase), (Provider::Soniox, AttemptPhase::Accepted));
    let request = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).expect("the attempt was sent");
    assert_eq!(request.model, "stt-async-v5");
    assert!(request.callback.url.expose().contains(&format!("work={id}&attempt={}", attempt.id)));
    let remote = attempt.remote.clone().unwrap();

    // A wrong secret, another work's id, then the right call.
    let body = completed(&remote, "hello world");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &header("wrong"), &body).await, CallbackOutcome::Unauthorized);
    assert_eq!(h.service.callback(Provider::Soniox, Uuid::new_v4(), attempt.id, &header(request.callback.secret.expose()), &body).await, CallbackOutcome::Mismatch);
    assert_eq!(h.service.callback(Provider::Deepgram, id, attempt.id, &header(request.callback.secret.expose()), &body).await, CallbackOutcome::Mismatch);
    assert_eq!(h.service.get(h.account, id).await.unwrap().status, Status::Running, "nothing changed before a valid callback");

    let right = header(request.callback.secret.expose());
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &body).await, CallbackOutcome::Applied);
    let done = h.service.get(h.account, id).await.unwrap();
    assert_eq!((done.status, done.transcript.as_deref()), (Status::Completed, Some("hello world")));
    let revision = done.revision;
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &body).await, CallbackOutcome::Ignored);
    assert_eq!(h.service.get(h.account, id).await.unwrap().revision, revision, "a replay changes nothing");
    assert_eq!(h.service.callback(Provider::Soniox, id, Uuid::new_v4(), &right, &body).await, CallbackOutcome::Unauthorized, "an unknown attempt has no binding");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_refusal_falls_back_to_the_other_provider_and_a_second_refusal_fails_the_work() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Ok(Submission::Rejected { status: Some(400), code: "bad_request".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let work = h.service.get(h.account, id).await.unwrap();
    let phases: Vec<_> = work.attempts.iter().map(|a| (a.provider, a.phase)).collect();
    assert_eq!(phases, vec![(Provider::Soniox, AttemptPhase::Rejected), (Provider::Deepgram, AttemptPhase::Accepted)]);
    assert_eq!(work.status, Status::Running);

    let h2 = harness(|_| {}).await;
    h2.soniox.answer(Ok(Submission::Rejected { status: Some(400), code: "a".into() }));
    h2.deepgram.answer(Ok(Submission::Rejected { status: Some(400), code: "b".into() }));
    let id = h2.uploaded(None).await;
    h2.dispatch(id).await;
    let failed = h2.service.get(h2.account, id).await.unwrap();
    assert_eq!((failed.status, failed.error.as_deref()), (Status::Failed, Some("all_providers_rejected")));
}


#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_callback_that_beats_the_submission_answer_is_authenticated_adopts_the_identity_and_completes() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    assert_eq!((attempt.phase, attempt.remote.is_none()), (AttemptPhase::Uncertain, true));
    let sent = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap();
    let body = completed(&h.soniox.remote("early-bird"), "first words");

    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &header("not-the-secret"), &body).await, CallbackOutcome::Unauthorized);
    assert!(h.service.get(h.account, id).await.unwrap().attempts[0].remote.is_none(), "an unauthenticated callback adopts nothing");

    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &header(sent.callback.secret.expose()), &body).await, CallbackOutcome::Applied);
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.status, work.transcript.as_deref()), (Status::Completed, Some("first words")));
    assert_eq!(work.attempts[0].remote, Some(h.soniox.remote("early-bird")), "the identity is kept for erasure");

    // From here the identity is bound: another remote id on the same attempt is refused.
    let other = completed(&h.soniox.remote("somebody-else"), "x");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &header(sent.callback.secret.expose()), &other).await, CallbackOutcome::Mismatch);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_callback_naming_another_identity_than_one_recorded_while_it_was_in_flight_is_refused_under_the_lock() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    assert!(attempt.remote.is_none(), "the callback below is verified before any identity is stored");
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    let (a, b) = (h.soniox.remote("remote-a"), h.soniox.remote("remote-b"));

    // B is verified with no bound identity (so it passes), then held between verification and
    // application while an authenticated notice binds A.
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    *h.soniox.hold.lock().unwrap() = Some((b.id.clone(), entered.clone(), release.clone()));
    let b_headers = header(secret.expose());
    let b_body = completed(&b, "text of b");
    let b_call = h.service.callback(Provider::Soniox, id, attempt.id, &b_headers, &b_body);
    let a_notice = async {
        entered.notified().await;
        let outcome = h.service.callback(Provider::Soniox, id, attempt.id, &header(secret.expose()), &processing(&a)).await;
        release.notify_one();
        outcome
    };
    let (b_outcome, a_outcome) = tokio::join!(b_call, a_notice);

    assert_eq!(a_outcome, CallbackOutcome::Applied, "the first authenticated identity is adopted");
    assert_eq!(b_outcome, CallbackOutcome::Mismatch, "B no longer matches the identity bound while it was in flight");
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.status, work.transcript), (Status::Running, None), "B's transcript never reaches the work");
    assert_eq!(work.attempts[0].remote, Some(a));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_superseded_attempts_late_callback_is_acknowledged_and_changes_nothing() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let first = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let first_request = h.soniox.sent().into_iter().find(|r| r.attempt == first.id).unwrap();
    // Stuck past the window with no way to ask Soniox (the double has no result): given up.
    h.advance(31 * 60 * SECOND);
    h.recover(id).await;
    h.dispatch(id).await;
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.attempts[0].phase, work.attempts[1].provider), (AttemptPhase::Superseded, Provider::Deepgram));
    let revision = work.revision;

    let late = completed(first.remote.as_ref().unwrap(), "stale text");
    let outcome = h.service.callback(Provider::Soniox, id, first.id, &header(first_request.callback.secret.expose()), &late).await;
    assert_eq!(outcome, CallbackOutcome::Ignored);
    let after = h.service.get(h.account, id).await.unwrap();
    assert_eq!((after.revision, after.transcript), (revision, None));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn retention_removes_old_transcripts_once_per_day_and_honors_the_opt_out() {
    let _workers = exclusive_workers().await;
    let h = harness(|c| c.retention_days = 30).await;
    // Far in the past, so the daily sweep reaches only this test's work and never another test's.
    h.advance(-100 * DAY);
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let work = h.service.get(h.account, id).await.unwrap();
    let attempt = work.attempts[0].clone();
    let request = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap();
    let body = completed(attempt.remote.as_ref().unwrap(), "kept for now");
    h.service.callback(Provider::Soniox, id, attempt.id, &header(request.callback.secret.expose()), &body).await;

    h.advance(30 * DAY);
    h.service.retention_pass().await.unwrap();
    assert!(h.service.get(h.account, id).await.unwrap().transcript.is_some(), "exactly 30 days old is kept");

    h.service.set_auto_delete(h.account, false).await.unwrap();
    h.advance(2 * DAY);
    h.service.retention_pass().await.unwrap();
    assert!(h.service.get(h.account, id).await.unwrap().transcript.is_some(), "an account that opted out keeps its transcripts");

    h.service.set_auto_delete(h.account, true).await.unwrap();
    h.advance(DAY);
    let report = h.service.retention_pass().await.unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let after = h.service.get(h.account, id).await.unwrap();
    assert_eq!((after.status, after.transcript), (Status::Completed, None), "the work stays, its transcript goes");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn erasure_puts_up_a_barrier_removes_every_external_resource_and_survives_completion() {
    let h = harness(|_| {}).await;
    let done = h.uploaded(None).await;
    let waiting = h.service.create(h.account, "later.mp3", None, None).await.unwrap();
    enroll_billing_sample(&h).await;
    h.dispatch(done).await;
    let first = h.service.get(h.account, done).await.unwrap().attempts[0].clone();
    let sent = h.soniox.sent().into_iter().find(|r| r.attempt == first.id).unwrap();
    let object = h.service.get(h.account, done).await.unwrap().object;
    assert!(h.objects.holds(&object));

    let view = h.service.erase_account(h.account).await.unwrap();
    // The barrier and the local deletion are immediate; the object waits out issued upload links.
    assert!(!view.complete, "an upload link issued before the barrier can still create the object");
    assert!(matches!(h.service.list(h.account).await, Err(ServiceError::Erased)));
    assert!(matches!(h.service.create(h.account, "new.mp3", None, None).await, Err(ServiceError::Erased)));
    assert!(matches!(h.service.enroll(h.account, "someone@example.test").await, Err(ServiceError::Erased)));
    assert!(matches!(h.service.reissue_upload(h.account, waiting.work.id).await, Err(ServiceError::Erased)));
    let outcome = h.service.callback(Provider::Soniox, done, first.id, &header(sent.callback.secret.expose()), &completed(first.remote.as_ref().unwrap(), "x")).await;
    assert_eq!(outcome, CallbackOutcome::Unauthorized, "the attempt has no binding after erasure");
    assert!(!h.objects.holds(&object));
    assert_eq!(sample_rows(&h).await, (0, 0), "the enrolled sample rows left with the account");
    assert_eq!(h.soniox.deleted.lock().unwrap().as_slice(), &[first.remote.clone().unwrap()]);
    let deletes = h.objects.deletes.lock().unwrap().clone();
    assert!(deletes.iter().any(|o| o.version.is_some()), "the generation confirmed at upload is deleted by name, so an older version is not abandoned");
    assert!(deletes.iter().any(|o| o.version.is_none()), "and the current key is swept as well, for a newer generation");

    // The late upload that the issued link still allows is removed once it has expired, but the key
    // stays under sweep: nothing proves that no other accepted upload will still commit.
    h.objects.upload(&object);
    h.advance(901 * SECOND);
    h.service.erase_account(h.account).await.unwrap();
    assert!(!h.objects.holds(&object));
    let view = h.service.erasure_view(h.account).await.unwrap();
    only_upload_sweeps_open(&view, 2);

    // The barrier and the local deletion stay.
    assert!(matches!(h.service.enroll(h.account, "someone@example.test").await, Err(ServiceError::Erased)));
    assert!(h.service.store().erased(h.account).await.unwrap());
    assert!(!h.service.erase_account(h.account).await.unwrap().complete, "repeating an unfinished erasure does not finish it");
}

/// Everything outside the object store is confirmed and the only things still open are the key
/// sweeps of issued upload links, one per work: explicit and unresolved, never closed by an expiry
/// and an absent key alone (an upload accepted before the expiry can still commit).
fn only_upload_sweeps_open(view: &ErasureView, works: usize) {
    assert!(!view.complete, "no expiry proves that an issued upload link will never commit: {:?}", view.open);
    assert_eq!(view.open.len(), works, "{:?}", view.open);
    for open in &view.open {
        assert_eq!((open.kind.as_str(), open.state.as_str()), ("object", "unresolved"), "{open:?}");
        assert!(open.reason.as_deref().is_some_and(|why| why.contains("until finality is proved")), "{open:?}");
    }
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn an_upload_that_commits_after_its_link_expired_and_the_sweep_found_nothing_is_still_deleted_and_never_final() {
    let h = harness(|_| {}).await;
    let created = h.service.create(h.account, "late.mp3", None, None).await.unwrap();
    let object = created.work.object.clone();
    h.service.erase_account(h.account).await.unwrap();

    // The link has expired and the key is absent. That proves nothing: the erasure stays open, says
    // why, is a normal answer (not a storage error) and is scheduled to be swept again.
    h.advance(901 * SECOND);
    let view = h.service.erase_account(h.account).await.unwrap();
    assert!(!h.objects.holds(&object));
    only_upload_sweeps_open(&view, 1);
    assert!(view.open[0].reason.as_deref().unwrap().contains("may still complete"), "{:?}", view.open);
    let sweep = h.service.store().erasure_resources(h.account).await.unwrap().into_iter().find(|r| r.kind == "object").expect("the key sweep is still a resource");
    assert!(sweep.blocked_until.is_some(), "it is swept again later");

    // An upload accepted before the expiry commits afterwards, beyond that boundary and the sweep that
    // found nothing. The next sweep removes it, and the erasure still is not final.
    h.objects.upload(&object);
    assert!(h.objects.holds(&object));
    h.advance(16 * 60 * SECOND);
    let view = h.service.erase_account(h.account).await.unwrap();
    assert!(!h.objects.holds(&object), "the re-sweep deletes the late object");
    only_upload_sweeps_open(&view, 1);
    assert!(view.open[0].reason.as_deref().unwrap().contains("completed after its link expired"), "{:?}", view.open);
}

/// The billing sample's enrolled rows for this account, as the P2 sample creates them.
async fn enroll_billing_sample(h: &Harness) {
    let customer = Customer { account: AccountId(h.account), email: format!("{}@example.test", h.account.simple()), full_name: Some("Someone".into()), created_at: BillingMicros(1_700_000_000_000_000) };
    taskr_billing::work::PgCustomerDirectory::new(h.runtime.clone()).register(&customer).await.unwrap();
    sqlx::query("INSERT INTO taskr_billing.sample_work (account_id, work_key, units, operation, status, created_at) VALUES ($1, 'w', 1, 'op', 'pending', now())")
        .bind(h.account)
        .execute(&h.runtime)
        .await
        .unwrap();
}

async fn sample_rows(h: &Harness) -> (i64, i64) {
    let customers = sqlx::query_scalar("SELECT count(*) FROM taskr_billing.customers WHERE account_id = $1").bind(h.account).fetch_one(&h.runtime).await.unwrap();
    let work = sqlx::query_scalar("SELECT count(*) FROM taskr_billing.sample_work WHERE account_id = $1").bind(h.account).fetch_one(&h.runtime).await.unwrap();
    (customers, work)
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn account_erasure_is_one_transaction_over_billing_the_enrolled_sample_rows_and_the_audio_work() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    enroll_billing_sample(&h).await;
    assert_eq!(sample_rows(&h).await, (1, 1));

    h.service.erase_account(h.account).await.unwrap();
    assert_eq!(sample_rows(&h).await, (0, 0), "the directory row (email, name) and the sample work are gone with the account");
    let works: i64 = sqlx::query_scalar("SELECT count(*) FROM taskr_work.works WHERE account_id = $1").bind(h.account).fetch_one(&h.runtime).await.unwrap();
    assert_eq!(works, 0);
    let billing_erased: Option<bool> = sqlx::query_scalar("SELECT billing_erased_at IS NOT NULL FROM billing.erasure_barriers WHERE account_id = $1").bind(h.account).fetch_optional(&h.runtime).await.unwrap();
    assert_eq!(billing_erased, Some(true), "billing's cascade committed with the rest");
    let barrier: Option<Option<i64>> = sqlx::query_scalar("SELECT data_erased_us FROM taskr_work.erasures WHERE account_id = $1").bind(h.account).fetch_optional(&h.runtime).await.unwrap();
    assert!(matches!(barrier, Some(Some(_))));

    // A stale enrollment or contact refresh cannot bring any of it back.
    let customer = Customer { account: AccountId(h.account), email: "again@example.test".into(), full_name: None, created_at: BillingMicros(1_700_000_000_000_000) };
    assert!(taskr_billing::work::PgCustomerDirectory::new(h.runtime.clone()).register(&customer).await.is_err());
    assert!(matches!(h.service.get(h.account, id).await, Err(ServiceError::Erased)));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn nothing_is_erased_while_a_subscription_may_still_renew_and_everything_is_once_it_is_stopped() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    enroll_billing_sample(&h).await;
    // A bound external subscription no configured gateway can confirm stopped.
    open_account_erasure(&PgBillingStore::new(h.owner.clone()), AccountId(h.account), BillingMicros(1_700_000_002_000_000)).await.unwrap();
    sqlx::query(
        "INSERT INTO billing.erasure_terminations (account_id, payment_gateway, environment, subscription_ref, customer_ref, updated_at)
         VALUES ($1, 'unconfigured-gateway', 'test', $2, 'cust-1', now())",
    )
    .bind(h.account)
    .bind(format!("sub-{}", h.account.simple()))
    .execute(&h.owner)
    .await
    .unwrap();

    let view = h.service.erase_account(h.account).await.unwrap();
    assert!(!view.complete);
    assert_eq!(view.open.first().map(|r| r.kind.as_str()), Some("billing"), "{:?}", view.open);
    // The barrier stops new activity, but no data was erased: the transaction did not commit.
    assert!(matches!(h.service.get(h.account, id).await, Err(ServiceError::Erased)));
    assert_eq!(sample_rows(&h).await, (1, 1));
    let ready = h.service.store().dispatchable(100_000).await.unwrap();
    assert!(!ready.iter().any(|(work, _)| *work == id), "a blocked erasure's work is not offered to the dispatcher, so it cannot crowd out live accounts");
    let works: i64 = sqlx::query_scalar("SELECT count(*) FROM taskr_work.works WHERE account_id = $1").bind(h.account).fetch_one(&h.runtime).await.unwrap();
    assert_eq!(works, 1, "the audio work stays until the subscription is confirmed stopped");

    // Confirmed stopped (the provider's authenticated answer, recorded by the billing module).
    sqlx::query("UPDATE billing.erasure_terminations SET status = 'confirmed', last_outcome = 'confirmed' WHERE account_id = $1").bind(h.account).execute(&h.owner).await.unwrap();
    h.service.erase_account(h.account).await.unwrap();
    assert_eq!(sample_rows(&h).await, (0, 0));
    let works: i64 = sqlx::query_scalar("SELECT count(*) FROM taskr_work.works WHERE account_id = $1").bind(h.account).fetch_one(&h.runtime).await.unwrap();
    assert_eq!(works, 0);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_pool_whose_database_lacks_the_billing_or_audio_work_schema_is_refused() {
    let h = harness(|_| {}).await;
    taskr_work::store::verify_same_database(&h.runtime).await.expect("the test database holds every schema");

    // A second database (a stale migration URL, a mistaken runtime URL) is detected by asking it.
    let scratch = format!("taskr_work_scratch_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {scratch}"))).execute(&h.owner).await.expect("the test login may create databases");
    let options = PgConnectOptions::from_str(&url()).unwrap().database(&scratch);
    let elsewhere = PgPoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
    let refused = taskr_work::store::verify_same_database(&elsewhere).await.unwrap_err().to_string();
    elsewhere.close().await;
    sqlx::query(AssertSqlSafe(format!("DROP DATABASE {scratch}"))).execute(&h.owner).await.unwrap();
    assert!(refused.contains("must share one database") && refused.contains("taskr_work") && refused.contains("billing"), "{refused}");
}

/// A provider listing entry that matches everything attempt `sent` carried.
fn record(h: &Harness, id: Uuid, key: &str, sent: &TranscriptionRequest, remote: &str) -> RemoteRecord {
    RemoteRecord {
        remote: h.soniox.remote(remote),
        status: "queued".into(),
        model: "stt-async-v5".into(),
        client_reference: Some(format!("work_{}", id.simple())),
        audio_url: Some(SensitiveUrl::new(format!("https://objects.test/{key}?sig=x"))),
        callback_url: Some(SensitiveUrl::new(sent.callback.url.expose().to_string())),
        created_at: taskr_work::correlate::format_rfc3339(h.now()),
    }
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_terminal_callback_cannot_hide_a_different_identity_that_arrived_while_it_was_in_flight() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    let (a, b) = (h.soniox.remote("remote-a"), h.soniox.remote("remote-b"));

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    *h.soniox.hold.lock().unwrap() = Some((b.id.clone(), entered.clone(), release.clone()));
    let b_headers = header(secret.expose());
    let b_body = completed(&b, "text of b");
    let b_call = h.service.callback(Provider::Soniox, id, attempt.id, &b_headers, &b_body);
    let a_call = async {
        entered.notified().await;
        // A COMPLETES the work while B is held: the attempt is finished, not live, when B resumes.
        let outcome = h.service.callback(Provider::Soniox, id, attempt.id, &header(secret.expose()), &completed(&a, "text of a")).await;
        release.notify_one();
        outcome
    };
    let (b_outcome, a_outcome) = tokio::join!(b_call, a_call);

    assert_eq!(a_outcome, CallbackOutcome::Applied);
    assert_eq!(b_outcome, CallbackOutcome::Mismatch, "a finished attempt still has its identity: B is a conflict, not a duplicate");
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.status, work.transcript.as_deref()), (Status::Completed, Some("text of a")));
    assert_eq!(work.attempts[0].remote, Some(a));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn the_same_identity_with_another_result_is_a_held_conflict_and_an_exact_replay_is_not() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    let remote = attempt.remote.clone().unwrap();
    let right = header(secret.expose());

    let first = completed(&remote, "first result");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &first).await, CallbackOutcome::Applied);
    let revision = h.service.get(h.account, id).await.unwrap().revision;
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &first).await, CallbackOutcome::Ignored, "the same delivery again");

    let changed = completed(&remote, "a different result");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &changed).await, CallbackOutcome::Conflict, "same identity, another payload");
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.transcript.as_deref(), work.revision), (Some("first result"), revision), "the conflicting payload is never applied");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_lost_answer_is_adopted_only_from_an_exact_listing_match_and_otherwise_stays_unresolved() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let work = h.service.get(h.account, id).await.unwrap();
    let attempt = work.attempts[0].clone();
    let key = work.object.key.clone();
    let sent = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap();
    assert_eq!(attempt.phase, AttemptPhase::Uncertain, "a lost answer is never a rejection");
    assert!(!h.recover(id).await, "the lease has not run out");

    h.advance(200 * SECOND);
    // A listing miss is no proof of nonacceptance: the attempt stays, and no fallback is bought.
    assert!(!h.recover(id).await);
    h.dispatch(id).await;
    assert!(h.deepgram.sent().is_empty(), "no fallback while the first attempt may exist");
    assert_eq!(h.service.get(h.account, id).await.unwrap().attempts[0].phase, AttemptPhase::Uncertain);

    // Right reference and callback but another audio object: not this attempt's job.
    let mut near = record(&h, id, &key, &sent, "near-miss");
    near.audio_url = Some(SensitiveUrl::new("https://objects.test/work/other/clip.mp3?sig=x"));
    *h.soniox.listing.lock().unwrap() = Ok(vec![near]);
    assert!(!h.recover(id).await);

    // Two exact matches are ambiguous: none is chosen.
    *h.soniox.listing.lock().unwrap() = Ok(vec![record(&h, id, &key, &sent, "m1"), record(&h, id, &key, &sent, "m2")]);
    assert!(!h.recover(id).await);
    let still = h.service.get(h.account, id).await.unwrap();
    assert_eq!((still.attempts[0].phase, still.attempts[0].remote.is_none()), (AttemptPhase::Uncertain, true));

    // Exactly one: adopted.
    *h.soniox.listing.lock().unwrap() = Ok(vec![record(&h, id, &key, &sent, "found-it")]);
    assert!(h.recover(id).await);
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.status, work.attempts[0].phase), (Status::Running, AttemptPhase::Accepted));
    assert_eq!(work.attempts[0].remote, Some(h.soniox.remote("found-it")));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_provider_with_no_listing_is_given_up_and_its_late_identity_is_kept_and_erased() {
    let h = harness(|_| {}).await;
    h.deepgram.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    *h.deepgram.listing.lock().unwrap() = Err(TransportError::Unsupported("no listing".into()));
    let id = h.uploaded(Some(Provider::Deepgram)).await;
    h.dispatch(id).await;
    let first = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    assert_eq!((first.provider, first.phase), (Provider::Deepgram, AttemptPhase::Uncertain));
    let secret = h.deepgram.sent().into_iter().find(|r| r.attempt == first.id).unwrap().callback.secret;

    h.advance(200 * SECOND);
    assert!(h.recover(id).await, "no listing can ever settle it: the contract accepts giving it up for this provider");
    h.dispatch(id).await;
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.attempts[0].phase, work.attempts[1].provider, work.status), (AttemptPhase::Superseded, Provider::Soniox, Status::Running));

    // The given-up job did start, and its callback names it: the identity is learned, not dropped.
    let late = h.deepgram.remote("late-dg");
    let outcome = h.service.callback(Provider::Deepgram, id, first.id, &header(secret.expose()), &completed(&late, "late text")).await;
    assert_eq!(outcome, CallbackOutcome::Ignored, "it changes nothing about the work");
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.transcript.clone(), work.attempts[0].phase, work.attempts[0].remote.clone()), (None, AttemptPhase::Superseded, Some(late.clone())));

    h.service.erase_account(h.account).await.unwrap();
    assert!(h.deepgram.deleted.lock().unwrap().contains(&late), "the late identity is in the manifest and deleted");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn an_opt_out_that_commits_after_the_candidates_were_chosen_still_keeps_the_transcript() {
    // It makes an old completed work and reads the global candidate list: the retention sweep of
    // another test would purge it, so it holds the same guard.
    let _workers = exclusive_workers().await;
    let h = harness(|c| c.retention_days = 30).await;
    h.advance(-100 * DAY);
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    h.service.callback(Provider::Soniox, id, attempt.id, &header(secret.expose()), &completed(attempt.remote.as_ref().unwrap(), "keep me")).await;
    h.advance(40 * DAY);

    // The sweep chose this work while auto-delete was on.
    let cutoff = h.now().0 - 31 * DAY;
    let chosen = h.service.store().retention_candidates(cutoff, 100_000).await.unwrap();
    assert!(chosen.iter().any(|(work, _)| *work == id));
    // The opt-out commits before the purge reaches it.
    h.service.set_auto_delete(h.account, false).await.unwrap();
    assert_eq!(h.service.retention_purge(h.account, id).await.unwrap(), Applied::Unchanged);
    assert!(h.service.get(h.account, id).await.unwrap().transcript.is_some());

    h.service.set_auto_delete(h.account, true).await.unwrap();
    assert_eq!(h.service.retention_purge(h.account, id).await.unwrap(), Applied::Changed);
    assert!(h.service.get(h.account, id).await.unwrap().transcript.is_none());
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_provider_job_that_cannot_be_deleted_yet_is_retried_until_it_is_deleted() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let remote = h.service.get(h.account, id).await.unwrap().attempts[0].remote.clone().unwrap();
    h.soniox.delete_answers.lock().unwrap().push_back(DeleteOutcome::Retained { until: None, reason: "still processing".into() });

    let view = h.service.erase_account(h.account).await.unwrap();
    assert!(!view.complete);
    let waiting = view.open.iter().find(|r| r.kind == "remote").expect("the provider copy is open");
    assert_eq!((waiting.state.as_str(), waiting.reason.as_deref()), ("blocked", Some("still processing")), "retained for now is runnable, not parked");

    h.advance(20 * 60 * SECOND);
    h.service.erase_account(h.account).await.unwrap();
    assert_eq!(h.soniox.deleted.lock().unwrap().iter().filter(|r| **r == remote).count(), 2, "asked again once the retry came due");
    let view = h.service.erasure_view(h.account).await.unwrap();
    only_upload_sweeps_open(&view, 1);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn erasure_deletes_every_job_that_matches_a_lost_attempt_and_not_one_under_another_project() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let work = h.service.get(h.account, id).await.unwrap();
    let key = work.object.key.clone();
    let sent = h.soniox.sent().into_iter().find(|r| r.attempt == work.attempts[0].id).unwrap();
    let mut elsewhere = record(&h, id, &key, &sent, "other-project");
    elsewhere.remote.project = "another-project".into();
    *h.soniox.listing.lock().unwrap() = Ok(vec![record(&h, id, &key, &sent, "m1"), record(&h, id, &key, &sent, "m2"), elsewhere]);

    h.service.erase_account(h.account).await.unwrap();
    h.service.erase_account(h.account).await.unwrap();
    let deleted = h.soniox.deleted.lock().unwrap().clone();
    assert!(deleted.contains(&h.soniox.remote("m1")) && deleted.contains(&h.soniox.remote("m2")), "every matching job is deleted: {deleted:?}");
    assert!(deleted.iter().all(|r| r.project != "another-project"), "a job under another project is not this attempt's");

    h.advance(901 * SECOND);
    h.service.erase_account(h.account).await.unwrap();
    let view = h.service.erasure_view(h.account).await.unwrap();
    only_upload_sweeps_open(&view, 1);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn an_acknowledgement_that_arrives_after_the_attempt_was_given_up_still_registers_its_identity() {
    let h = harness(|_| {}).await;
    *h.deepgram.listing.lock().unwrap() = Err(TransportError::Unsupported("no listing".into()));
    let id = h.uploaded(Some(Provider::Deepgram)).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    *h.deepgram.submit_hold.lock().unwrap() = Some((entered.clone(), release.clone()));
    h.deepgram.answer(Ok(Submission::Accepted { remote: h.deepgram.remote("slow-ack") }));

    let submitting = h.dispatch(id);
    let interfering = async {
        // The request is out and its answer is held; the lease runs out and recovery gives the attempt up.
        entered.notified().await;
        h.advance(200 * SECOND);
        assert!(h.recover(id).await, "no listing can settle it, so it is given up for the other provider");
        release.notify_one();
    };
    tokio::join!(submitting, interfering);

    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!(work.attempts[0].phase, AttemptPhase::Superseded, "the late answer does not revive it");
    assert_eq!(work.attempts[0].remote, Some(h.deepgram.remote("slow-ack")), "but its identity is no longer lost");

    h.service.erase_account(h.account).await.unwrap();
    assert!(h.deepgram.deleted.lock().unwrap().contains(&h.deepgram.remote("slow-ack")), "and it is deleted with the account");
}

fn failed(remote: &RemoteIdentity, message: &str) -> Vec<u8> {
    let payload = ProviderPayload::Soniox {
        reported: serde_json::from_value(json!({"id": remote.id, "status": "error", "error_message": message})).unwrap(),
        transcript: None,
    };
    serde_json::to_vec(&ProviderObservation { remote: remote.clone(), origin: ObservationOrigin::Webhook, payload }).unwrap()
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_failed_attempt_keeps_its_digest_so_another_failure_or_a_success_is_a_conflict() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    let remote = attempt.remote.clone().unwrap();
    let right = header(secret.expose());

    let first = failed(&remote, "bad audio");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &first).await, CallbackOutcome::Applied);
    assert_eq!(h.service.get(h.account, id).await.unwrap().status, Status::Failed);
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &first).await, CallbackOutcome::Ignored, "the exact delivery again");
    let revision = h.service.get(h.account, id).await.unwrap().revision;

    // The attempt is Failed, so every lifecycle call would be stale: only the digest can tell.
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &failed(&remote, "another reason")).await, CallbackOutcome::Conflict);
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &completed(&remote, "a success after all")).await, CallbackOutcome::Conflict);
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.status, work.transcript, work.revision), (Status::Failed, None, revision), "neither is applied");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_callback_verified_before_the_erasure_and_applied_after_it_still_registers_its_identity() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    let late = h.soniox.remote("late-id");

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    *h.soniox.hold.lock().unwrap() = Some((late.id.clone(), entered.clone(), release.clone()));
    let late_headers = header(secret.expose());
    let late_body = completed(&late, "too late");
    let callback = h.service.callback(Provider::Soniox, id, attempt.id, &late_headers, &late_body);
    let erasing = async {
        // The callback is verified and waiting; the whole account is erased in between.
        entered.notified().await;
        h.service.erase_account(h.account).await.unwrap();
        release.notify_one();
    };
    let (outcome, ()) = tokio::join!(callback, erasing);
    assert_eq!(outcome, CallbackOutcome::Dropped, "nothing is applied to an erased account");

    h.service.erase_account(h.account).await.unwrap();
    assert!(h.soniox.deleted.lock().unwrap().contains(&late), "the authenticated identity was registered and is deleted, not dropped with the callback");
    h.advance(901 * SECOND);
    h.service.erase_account(h.account).await.unwrap();
    let view = h.service.erasure_view(h.account).await.unwrap();
    only_upload_sweeps_open(&view, 1);
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_webhook_after_a_poll_completion_is_judged_against_the_polled_result() {
    let h = harness(|_| {}).await;
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    let secret = h.soniox.sent().into_iter().find(|r| r.attempt == attempt.id).unwrap().callback.secret;
    let remote = attempt.remote.clone().unwrap();
    let right = header(secret.expose());

    // Recovery polls the provider past the stuck window and takes the finished result.
    let mut polled: ProviderObservation = serde_json::from_slice(&completed(&remote, "the transcript")).unwrap();
    polled.origin = ObservationOrigin::RecoveryPoll;
    *h.soniox.result_answer.lock().unwrap() = Some(polled);
    h.advance(31 * 60 * SECOND);
    assert!(h.recover(id).await);
    assert_eq!(h.service.get(h.account, id).await.unwrap().transcript.as_deref(), Some("the transcript"));
    let revision = h.service.get(h.account, id).await.unwrap().revision;

    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &completed(&remote, "the transcript")).await, CallbackOutcome::Ignored, "the same result by webhook is a replay");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &failed(&remote, "late error")).await, CallbackOutcome::Conflict, "a failure after a success is another result");
    assert_eq!(h.service.callback(Provider::Soniox, id, attempt.id, &right, &completed(&remote, "another transcript")).await, CallbackOutcome::Conflict);
    let work = h.service.get(h.account, id).await.unwrap();
    assert_eq!((work.status, work.transcript.as_deref(), work.revision), (Status::Completed, Some("the transcript"), revision));
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_late_acknowledgement_that_converted_an_unidentified_resource_does_not_make_a_listing_lose_its_matches() {
    let h = harness(|_| {}).await;
    h.soniox.answer(Err(TransportError::Unavailable { status: None, operation: "submit".into() }));
    let id = h.uploaded(None).await;
    h.dispatch(id).await;
    let attempt = h.service.get(h.account, id).await.unwrap().attempts[0].clone();
    h.service.erase_account(h.account).await.unwrap();
    let resources = h.service.store().erasure_resources(h.account).await.unwrap();
    let unidentified = resources.iter().find(|r| r.kind == "unidentified").expect("the lost attempt is in the manifest").id;

    // While a listing is in flight the acknowledgement arrives and turns the resource into a known remote...
    let (a, b, c) = (h.soniox.remote("ack"), h.soniox.remote("listed-1"), h.soniox.remote("listed-2"));
    h.service.store().record_late_remote(h.account, attempt.id, &a).await.unwrap();
    // ...then the listing's matches arrive: all are registered, once, and the known remote is kept.
    h.service.store().identify_resource(h.account, unidentified, &[b.clone(), c.clone(), a.clone()]).await.unwrap();
    h.service.store().set_unidentified(unidentified, "unresolved", Some("a stale verdict")).await.unwrap();

    let resources = h.service.store().erasure_resources(h.account).await.unwrap();
    let mut remotes: Vec<String> = resources
        .iter()
        .filter(|r| r.kind == "remote")
        .map(|r| serde_json::from_value::<RemoteIdentity>(r.target.clone()).unwrap().id)
        .collect();
    remotes.sort();
    let mut expected = vec![a.id.clone(), b.id.clone(), c.id.clone()];
    expected.sort();
    assert_eq!(remotes, expected, "every identity once, none lost");
    let converted = resources.iter().find(|r| r.id == unidentified).unwrap();
    assert_eq!((converted.kind.as_str(), converted.state.as_str()), ("remote", "pending"), "a stale unresolved note cannot park a known remote");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_provider_retention_deadline_never_shortens_the_upload_link_hold_and_a_longer_one_wins() {
    for (retained_for, expected_after) in [(60, 900), (7200, 7200)] {
        let h = harness(|_| {}).await;
        h.uploaded(None).await;
        let t0 = h.now();
        // The generation is deleted by name first; the current-key sweep is the one the provider holds.
        h.objects.delete_answers.lock().unwrap().push_back(DeleteOutcome::Deleted);
        h.objects.delete_answers.lock().unwrap().push_back(DeleteOutcome::Retained { until: Some(Micros(t0.0 + retained_for * SECOND)), reason: "retention".into() });
        h.service.erase_account(h.account).await.unwrap();
        let resources = h.service.store().erasure_resources(h.account).await.unwrap();
        let key_sweep = resources.iter().find(|r| r.kind == "object" && r.target.get("version").is_none_or(|v| v.is_null())).expect("the current-key sweep");
        assert_eq!(key_sweep.state, "blocked");
        assert_eq!(key_sweep.blocked_until, Some(Micros(t0.0 + expected_after * SECOND)), "retained for {retained_for}s");
    }
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn erasures_that_cannot_finish_do_not_starve_the_ones_behind_them() {
    // Isolation: no test advances the shared queue (`erasure_pass` touches every pending erasure with
    // its own doubles), each advances only its own account through `erase_account`, so nothing else
    // moves these two accounts between the readings below.
    let _workers = exclusive_workers().await;
    let h = harness(|_| {}).await;
    let other = harness(|_| {}).await;
    let (first, second) = (h.account, other.account);
    // Both are pending with a barrier; the first was requested earlier and keeps being tried.
    h.service.store().open_barrier(first, Micros(1)).await.unwrap();
    other.service.store().open_barrier(second, Micros(2)).await.unwrap();
    let position = |list: &[Uuid], who: Uuid| list.iter().position(|a| *a == who).unwrap();
    let before = h.service.store().pending_erasures(100_000).await.unwrap();
    assert!(position(&before, first) < position(&before, second), "oldest request first when neither was tried");

    h.service.store().touch_erasure(first, Micros(10)).await.unwrap();
    let after = h.service.store().pending_erasures(100_000).await.unwrap();
    assert!(position(&after, second) < position(&after, first), "after a try the first goes behind everything not yet tried");
}

#[tokio::test]
#[ignore = "needs TASKR_WORK_TEST_DATABASE_URL"]
async fn a_listing_result_that_arrives_after_the_erasure_closed_reopens_it_and_is_deleted() {
    let h = harness(|_| {}).await;
    // An account with no works has no issued upload links to sweep, so its erasure closes at once.
    assert!(h.service.erase_account(h.account).await.unwrap().complete, "closed: nothing was left to confirm");
    assert!(h.service.store().erasure_resources(h.account).await.unwrap().is_empty(), "and its details are gone");

    // A listing that was in flight returns a job for a resource row that no longer exists.
    let late = h.soniox.remote("listed-late");
    h.service.store().identify_resource(h.account, 9_999_999_999, std::slice::from_ref(&late)).await.unwrap();
    let view = h.service.erasure_view(h.account).await.unwrap();
    assert!(!view.complete, "the match is registered and the erasure is open again");
    assert!(h.service.store().pending_erasures(100_000).await.unwrap().contains(&h.account));

    h.service.erase_account(h.account).await.unwrap();
    assert!(h.soniox.deleted.lock().unwrap().contains(&late));
    assert!(h.service.erasure_view(h.account).await.unwrap().complete, "and closed again once confirmed");
}
