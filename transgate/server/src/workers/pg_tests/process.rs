//! The jobs schedule end to end with the real worker, a real database and scripted providers: every
//! three minutes one replica runs the pass (queue dispatch, 30-minute stuck recovery for Deepgram and
//! Soniox), a stranded pending job is dispatched once by a fresh process, and nothing is charged again.
//! The clock is the test's. Providers are scripted at the port; no provider is called.

use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;

use super::*;
use media_ports::{AccessMethod, ObjectMetadata, ObjectStore, ProviderPayload, PutObject, SensitiveUrl, SonioxReported, TemporaryAccess};
use transgate_core::schedule::Schedule;


/// A provider that accepts each submission (or answers from a script) and counts its calls.
struct Remote {
    provider: media_ports::Provider,
    answers: Mutex<VecDeque<Result<Submission, TransportError>>>,
    submits: AtomicUsize,
    polls: AtomicUsize,
    state: &'static str,
}
impl Remote {
    fn new(provider: media_ports::Provider, state: &'static str) -> Arc<Remote> {
        Arc::new(Remote { provider, answers: Mutex::new(VecDeque::new()), submits: AtomicUsize::new(0), polls: AtomicUsize::new(0), state })
    }
    fn submits(&self) -> usize {
        self.submits.load(Ordering::SeqCst)
    }
    fn polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }
}
impl Transcriber for Remote {
    fn provider(&self) -> media_ports::Provider {
        self.provider
    }
    fn project(&self) -> &str {
        "project"
    }
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        let n = self.submits.fetch_add(1, Ordering::SeqCst);
        let next = self.answers.lock().unwrap().pop_front().unwrap_or_else(|| {
            Ok(Submission::Accepted { remote: RemoteIdentity { provider: self.provider, project: "project".into(), id: format!("{}-{n}-{}", request.client_reference, self.provider.as_str_for_test()) } })
        });
        Box::pin(async move { next })
    }
    fn status<'a>(&'a self, remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let observation = ProviderObservation {
            remote: remote.clone(),
            origin: media_ports::ObservationOrigin::RecoveryPoll,
            payload: ProviderPayload::Soniox {
                reported: SonioxReported { id: remote.id.clone(), status: self.state.into(), audio_duration_ms: None, error: None, error_type: None, error_message: None, extra: Default::default() },
                transcript: None,
            },
        };
        Box::pin(async move { Ok(observation) })
    }
    fn result<'a>(&'a self, _: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        unused()
    }
    fn delete_remote<'a>(&'a self, _: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        unused()
    }
    fn find_by_reference<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn verify_callback(&self, _: &CallbackBinding, _: &Headers, _: &[u8]) -> Result<ProviderObservation, CallbackError> {
        Err(CallbackError::Unauthorized)
    }
    fn complete_callback(&self, observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        Box::pin(async move { Ok(observation) })
    }
}

trait ProviderName {
    fn as_str_for_test(&self) -> &'static str;
}
impl ProviderName for media_ports::Provider {
    fn as_str_for_test(&self) -> &'static str {
        match self {
            media_ports::Provider::Soniox => "soniox",
            media_ports::Provider::Deepgram => "deepgram",
        }
    }
}

/// An upload bucket that signs any object.
struct Bucket;
impl ObjectStore for Bucket {
    fn kind(&self) -> StoreKind {
        StoreKind::Gcs
    }
    fn bucket(&self) -> &str {
        "uploads"
    }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: media_ports::Micros, ttl: u32) -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        Box::pin(async move {
            Ok(TemporaryAccess {
                object: object.clone(),
                method,
                url: SensitiveUrl::new(format!("https://storage.example.test/{}", object.key)),
                expires_at: media_ports::Micros(now.0 + i64::from(ttl) * 1_000_000),
                headers: vec![],
            })
        })
    }
    fn head<'a>(&'a self, _: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        Box::pin(async { Ok(None) })
    }
    fn put(&self, _: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        unused()
    }
    fn delete<'a>(&'a self, _: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async { Ok(DeleteOutcome::Absent) })
    }
}

fn at(seconds: i64) -> Micros {
    Micros(seconds * SECOND)
}

fn replica_of(pool: &PgPool, clock: &Arc<Injected>, soniox: &Arc<Remote>, deepgram: &Arc<Remote>) -> Arc<Workers> {
    replica_with(pool, clock.clone(), vec![soniox.clone(), deepgram.clone()], vec![Arc::new(Bucket)])
}

async fn account(store: &PgJobStore, now: Micros) -> AccountId {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    assert!(uow.billing().await.unwrap().create_account(account, now).await.unwrap());
    uow.commit().await.unwrap();
    account
}

async fn job(store: &PgJobStore, account: AccountId, status: JobStatus, minutes: i32, created: Micros) -> transgate_core::JobId {
    let mut uow = store.begin().await.unwrap();
    let job = uow
        .insert_job(&NewJob {
            account,
            status,
            name: Some("talk.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: Some(object(&format!("{}/talk.mp3", account.0))) },
            language: Language::from_code("en"),
            translate_to: vec![],
            speakers: false,
            fix_grammer: None,
            duration_minutes: Some(minutes),
            size_bytes: Some(1024),
            callback_url: None,
            whisper_params: None,
            created_at: created,
        })
        .await
        .unwrap();
    uow.commit().await.unwrap();
    job.id
}

/// A running job of `provider` that was accepted `started` ago and has not answered since.
async fn running(store: &PgJobStore, account: AccountId, provider: Provider, remote: &str, started: Micros) -> transgate_core::JobId {
    let id = job(store, account, JobStatus::Running, 5, started).await;
    let attempt = Attempt {
        id: AttemptId::new(),
        job: id,
        account,
        number: 1,
        provider,
        project: "project".into(),
        model: "m".into(),
        state: AttemptState::Accepted,
        claimed_at: started,
        lease_until: Micros(started.0 + 2 * MICROS_PER_MINUTE),
        request: AttemptRequest { language_code: Some("en".into()), translation_target: None, diarize: false, audio: object("audio"), client_reference: format!("job_{}", id.0) },
        callback_secret: SecretString::new("secret"),
        remote_id: Some(remote.into()),
        accepted_at: Some(started),
        last_error: None,
        remote_cleanup: RemoteCleanup::NotRequested,
    };
    let mut uow = store.begin().await.unwrap();
    uow.insert_attempt(&attempt).await.unwrap();
    let mut row = uow.lock_job(id).await.unwrap().unwrap();
    row.current_attempt = Some(attempt.id);
    row.provider = Some(provider);
    row.started_at = Some(started);
    row.attempts_made = 1;
    if provider == Provider::Soniox {
        row.soniox_transcription_id = Some(remote.into());
    }
    uow.save_job(&row, row.revision).await.unwrap();
    uow.commit().await.unwrap();
    id
}

async fn row(pool: &PgPool, id: transgate_core::JobId) -> (String, Option<String>, i32, i32) {
    sqlx::query_as("SELECT status, provider, retry_count, recovery_attempts FROM transgate_jobs.jobs WHERE id = $1").bind(id.0).fetch_one(pool).await.unwrap()
}

async fn slot_runs(pool: &PgPool, slot: Micros) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM media.schedule_slots WHERE schedule = 'jobs' AND slot_at = media.us_to_ts($1) AND outcome = 'succeeded'")
        .bind(slot.0)
        .fetch_one(pool)
        .await
        .unwrap()
}

// JOB-026: a Soniox job that was rate limited and left `pending` is picked up again. A fresh process
// (a restart: no memory of the job) dispatches it on its first jobs pass exactly once, the provider is
// asked once, and the account's usage is not charged a second time.
#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn job_026_a_stranded_pending_soniox_job_is_dispatched_once_after_a_restart_and_never_charged_again() {
    let (pool, _database) = isolated().await;
    let store = PgJobStore::new(pool.clone());
    let slot_time = 1_950_000_000 / 180 * 180;
    let clock = Arc::new(Injected(AtomicI64::new((slot_time + 10) * SECOND)));
    let (soniox, deepgram) = (Remote::new(media_ports::Provider::Soniox, "processing"), Remote::new(media_ports::Provider::Deepgram, "processing"));
    let owner = account(&store, clock.now()).await;
    let stranded = job(&store, owner, JobStatus::Pending, 3, at(slot_time - 20 * 60)).await;
    // It was charged when it started: one usage row of its three minutes against the account's package.
    let package: i64 = sqlx::query_scalar(
        "INSERT INTO billing.packages (account_id, name, minutes_included, created, start_at, end_at) VALUES ($1, 'Pro', 100, now(), now() - interval '1 day', now() + interval '30 days') RETURNING id",
    )
    .bind(owner.0)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at) VALUES ($1, $2, $3, 3, now())")
        .bind(owner.0).bind(package).bind(stranded.0).execute(&pool).await.unwrap();
    let usage = || {
        let pool = pool.clone();
        async move { sqlx::query_as::<_, (i64, i64)>("SELECT count(*), COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1").bind(owner.0).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!(usage().await, (1, 3));

    // The first process's memory is gone; a new one runs the pass.
    let restarted = replica_of(&pool, &clock, &soniox, &deepgram);
    assert!(restarted.run_schedules(&[Schedule::Jobs]).await.is_empty());
    assert_eq!((soniox.submits(), deepgram.submits()), (1, 0), "dispatched once, to Soniox");
    let (status, provider, ..) = row(&pool, stranded).await;
    assert_eq!((status.as_str(), provider.as_deref()), ("running", Some("soniox")));
    assert_eq!(usage().await, (1, 3), "no second charge");

    // Another restart in the same slot and a later slot: the job is running, so nothing is submitted again.
    let again = replica_of(&pool, &clock, &soniox, &deepgram);
    again.run_schedules(&[Schedule::Jobs]).await;
    clock.0.store((slot_time + 180 + 10) * SECOND, Ordering::SeqCst);
    again.run_schedules(&[Schedule::Jobs]).await;
    assert_eq!(soniox.submits(), 1, "a running job is not dispatched again");
    assert_eq!(usage().await, (1, 3));
}

// CRN-001, CRN-002, CRN-003: the jobs schedule runs once per three-minute slot however many replicas
// tick (CRN-001), a restarted replica takes the next slot, and one pass dispatches the Deepgram queue and
// recovers jobs stuck for more than 30 minutes: a stuck Deepgram job is reset to pending (CRN-002) and a
// stuck Soniox job is polled for its status (CRN-003).
#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn crn_001_002_003_the_three_minute_jobs_slot_runs_once_across_replicas_recovers_stuck_jobs_and_resumes_after_a_restart() {
    let (pool, _database) = isolated().await;
    let store = PgJobStore::new(pool.clone());
    let slot_time = 1_950_000_000 / 180 * 180;
    let clock = Arc::new(Injected(AtomicI64::new((slot_time + 10) * SECOND)));
    let (soniox, deepgram) = (Remote::new(media_ports::Provider::Soniox, "processing"), Remote::new(media_ports::Provider::Deepgram, "processing"));
    let owner = account(&store, clock.now()).await;
    let started = at(slot_time + 10 - 31 * 60);
    let queued = job(&store, owner, JobStatus::Pending, 300, at(slot_time - 10 * 60)).await; // over the Soniox limit: Deepgram
    let stuck_deepgram = running(&store, owner, Provider::Deepgram, "dg-stuck", started).await;
    let stuck_soniox = running(&store, owner, Provider::Soniox, "tx-stuck", started).await;
    let recent = running(&store, owner, Provider::Soniox, "tx-recent", at(slot_time + 10 - 29 * 60)).await;
    let (a, b) = (replica_of(&pool, &clock, &soniox, &deepgram), replica_of(&pool, &clock, &soniox, &deepgram));

    // Two replicas tick the same slot at once: one pass runs.
    let (first, second) = tokio::join!(a.run_schedules(&[Schedule::Jobs]), b.run_schedules(&[Schedule::Jobs]));
    assert!(first.is_empty() && second.is_empty(), "{first:?} {second:?}");
    assert_eq!(slot_runs(&pool, Schedule::Jobs.slot(clock.now())).await, 1, "one claimed and finished slot");
    assert_eq!(deepgram.submits(), 1, "the queued Deepgram job was dispatched once (CRN-002)");
    assert_eq!(row(&pool, queued).await.0, "running");
    let (status, _, retries, _) = row(&pool, stuck_deepgram).await;
    assert_eq!((status.as_str(), retries), ("pending", 1), "a Deepgram job running for over 30 minutes is reset to pending (CRN-002)");
    assert_eq!(soniox.polls(), 1, "only the Soniox job stuck for over 30 minutes is polled (CRN-003); the 29 minute one is left alone");
    let (status, _, _, recovery) = row(&pool, stuck_soniox).await;
    assert_eq!((status.as_str(), recovery), ("running", 0), "still processing: the claim is given back");
    assert_eq!(row(&pool, recent).await.0, "running");

    // The same slot again, even from a third replica: nothing moves.
    let third = replica_of(&pool, &clock, &soniox, &deepgram);
    third.run_schedules(&[Schedule::Jobs]).await;
    assert_eq!((deepgram.submits(), soniox.polls()), (1, 1));

    // Three minutes later a restarted replica takes the next slot: the reset job is submitted again.
    clock.0.store((slot_time + 180 + 10) * SECOND, Ordering::SeqCst);
    let restarted = replica_of(&pool, &clock, &soniox, &deepgram);
    assert!(restarted.run_schedules(&[Schedule::Jobs]).await.is_empty());
    assert_eq!(slot_runs(&pool, Schedule::Jobs.slot(clock.now())).await, 1);
    let dump: Vec<(i64, String, String, Option<String>)> = sqlx::query_as("SELECT job_id, state, provider, remote_id FROM transgate_jobs.attempts ORDER BY job_id, number").fetch_all(&pool).await.unwrap();
    let jobs: Vec<(i64, String, Option<String>, Option<i64>)> = sqlx::query_as("SELECT id, status, provider, transgate_jobs.ts_to_us(next_dispatch_at) FROM transgate_jobs.jobs ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(deepgram.submits(), 2, "the next slot dispatched the job the last one reset: attempts {dump:?} jobs {jobs:?} reset job {}", stuck_deepgram.0);
    assert_eq!(row(&pool, stuck_deepgram).await.0, "running");
    assert_eq!(soniox.polls(), 3, "the stuck Soniox job again, and now the one that was 29 minutes old (over 30 by this slot)");
}
