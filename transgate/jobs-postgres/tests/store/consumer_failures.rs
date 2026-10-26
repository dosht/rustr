//! Actual Jobs Start/intake/ingest/dispatch over PgJobStore and the restricted runtime.
//! No billing or JobUow double: the only doubles are outbound signing/provider I/O.
//!
//! Each test creates a private database using the existing fixture's owner authority and
//! already-provisioned runtime login. This needs CREATEDB on the disposable test owner.
//! SQL constraints and global dispatch scans never touch the shared fixture database.
//! The database is dropped even if a test body panics. No role changes or outbox claims.

use super::{
    jobs_app_with_transports, new_job, owner, database_url, PgJobStore, JobQueries, JobStore,
    JobEvent, JobId, JobStatus, AccountId, Micros, NOW, RUNTIME_LOGIN, RUNTIME_PASSWORD,
};
use std::{future::Future, sync::{Arc, Mutex}};
use billing_core::model::NewPackage;
use media_ports::{
    AccessMethod, BoxFuture, CallbackBinding, CallbackError, DeleteOutcome, Headers,
    ObjectMetadata, ObjectRef, ObjectStore, ObservationOrigin, ProviderObservation,
    ProviderPayload, PutObject, RemoteIdentity, RemoteRecord, SensitiveUrl, SonioxReported,
    SonioxToken, SonioxTranscript, StoreKind, Submission, TemporaryAccess, Transcriber,
    TranscriptionRequest, TransportError,
};
use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions, types::Json};
use transgate_core::{
    ids::{InboxId, MICROS_PER_DAY},
    jobs::{
        app::{IntakeAck, Jobs, NewDraft, StartRequest},
        domain::{Attempt, AttemptState, DeferredCharge, Provider},
        events::{mails_for, JobMail},
        ingest,
        ports::InboxState,
        routing::Preferences,
    },
    languages::Language,
};
use uuid::Uuid;

async fn isolated<F, Fut>(body: F)
where
    F: FnOnce(PgJobStore, PgPool) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    // Existing OnceCell provisions roles once, before any scratch schema migrations.
    let admin = owner().await;
    let name = format!("transgate_consumer_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&admin).await.unwrap();
    let scratch_owner = PgPoolOptions::new().max_connections(4)
        .connect_with(admin.connect_options().as_ref().clone().database(&name)).await.unwrap();
    billing_postgres::migrate(&scratch_owner).await.unwrap();
    transgate_jobs_postgres::migrate(&scratch_owner).await.unwrap();
    let options = billing_postgres::options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD)
        .unwrap().database(&name);
    let pool = billing_postgres::connect_runtime(options, 8).await.unwrap();
    transgate_jobs_postgres::verify_runtime(&pool).await.unwrap();
    let store = PgJobStore::new(pool);

    let result = tokio::spawn(body(store.clone(), scratch_owner.clone())).await;
    store.pool().close().await;
    scratch_owner.close().await;
    sqlx::query(AssertSqlSafe(format!("DROP DATABASE {name} WITH (FORCE)")))
        .execute(&admin).await.unwrap();
    match result {
        Ok(()) => {}
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => panic!("consumer test task was cancelled: {error}"),
    }
}

/// Sign only the fixture's upload bucket; archive I/O is intentionally unconfigured.
struct UploadSigner;

impl ObjectStore for UploadSigner {
    fn kind(&self) -> StoreKind { StoreKind::Gcs }
    fn bucket(&self) -> &str { "uploads" }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: media_ports::Micros, ttl: u32)
        -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        Box::pin(async move {
            assert_eq!((object.store, object.bucket.as_str()), (StoreKind::Gcs, "uploads"));
            Ok(TemporaryAccess {
                object: object.clone(), method, url: SensitiveUrl::new("https://objects.example.test/audio"),
                expires_at: media_ports::Micros(now.0 + i64::from(ttl) * 1_000_000), headers: vec![],
            })
        })
    }
    fn head<'a>(&'a self, _object: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture does not perform object lookup".into())) })
    }
    fn put(&self, _request: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture has no archive bucket".into())) })
    }
    fn delete<'a>(&'a self, _object: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture does not erase objects".into())) })
    }
}

#[derive(Default)]
struct ProviderScript {
    refused: Mutex<Option<JobId>>,
    sent: Mutex<Vec<TranscriptionRequest>>,
}

impl Transcriber for ProviderScript {
    fn provider(&self) -> media_ports::Provider { media_ports::Provider::Soniox }
    fn project(&self) -> &str { "project" }
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        self.sent.lock().unwrap().push(request.clone());
        let refuse = self.refused.lock().unwrap().is_some_and(|id| request.client_reference == format!("job_{}", id.0));
        Box::pin(async move {
            Ok(if refuse {
                Submission::Rejected { status: Some(400), code: "invalid_audio_fixture".into() }
            } else {
                Submission::Accepted {
                    remote: RemoteIdentity { provider: media_ports::Provider::Soniox, project: "project".into(), id: format!("remote-{}", request.attempt) },
                }
            })
        })
    }
    fn status<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture has no status polling".into())) })
    }
    fn result<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture supplies complete callback tokens".into())) })
    }
    fn delete_remote<'a>(&'a self, _remote: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture does not erase remote work".into())) })
    }
    fn find_by_reference<'a>(&'a self, _reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("fixture does not reconcile listings".into())) })
    }
    fn verify_callback(&self, _binding: &CallbackBinding, _headers: &Headers, _body: &[u8]) -> Result<ProviderObservation, CallbackError> {
        Err(CallbackError::Unauthorized) // These tests enter after the host's authentication boundary.
    }
    fn complete_callback(&self, _observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("callback must already contain its transcript".into())) })
    }
}

fn app(store: &PgJobStore) -> (Jobs, Arc<ProviderScript>) {
    let provider = Arc::new(ProviderScript::default());
    (jobs_app_with_transports(store, NOW, vec![Arc::new(UploadSigner)], vec![provider.clone()]), provider)
}

async fn account(store: &PgJobStore) -> AccountId {
    let id = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    assert!(uow.billing().await.unwrap().create_account(id, NOW).await.unwrap());
    uow.commit().await.unwrap();
    id
}

async fn package(store: &PgJobStore, account: AccountId, minutes: i64, days: i64) -> i64 {
    let mut uow = store.begin().await.unwrap();
    let id = {
        let mut billing = uow.billing().await.unwrap();
        billing.lock_account(account).await.unwrap();
        billing.insert_package(&NewPackage {
            account, payment_id: None, payment_seq: None, name: "paid".into(), minutes,
            start_at: Micros(NOW.0 - MICROS_PER_DAY), end_at: Micros(NOW.0 + days * MICROS_PER_DAY),
            created: NOW, signup_grant: false,
        }).await.unwrap()
    };
    uow.commit().await.unwrap();
    id
}

async fn draft(app: &Jobs, account: AccountId, minutes: Option<i32>, name: &str) -> JobId {
    app.create_draft(account, NewDraft {
        name: name.into(), size_bytes: Some(1024), language: Language::from_code("en"),
        speakers: true, duration_minutes: minutes,
    }).await.unwrap().id
}

fn start(ids: Vec<JobId>) -> StartRequest {
    StartRequest { job_ids: ids, locale: Some("en".into()), preferences: Preferences { preferred: None, fallback_enabled: false } }
}

async fn running_attempt(store: &PgJobStore, account: AccountId, id: JobId) -> Attempt {
    let job = store.owned_job(account, id).await.unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Running);
    store.attempt(job.current_attempt.unwrap()).await.unwrap().unwrap()
}

fn speech(attempt: &Attempt) -> ProviderObservation {
    let remote = attempt.remote_id.clone().expect("accepted provider identity");
    ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Soniox, project: attempt.project.clone(), id: remote.clone() },
        origin: ObservationOrigin::Webhook,
        payload: ProviderPayload::Soniox {
            reported: SonioxReported {
                id: remote.clone(), status: "completed".into(), audio_duration_ms: Some(180_000.0),
                error: None, error_type: None, error_message: None, extra: Default::default(),
            },
            transcript: Some(SonioxTranscript {
                id: remote, tokens: [("First complete sentence.", 0.0, 2000.0, 1), ("Second complete sentence.", 3000.0, 5000.0, 2)]
                    .into_iter().map(|(text, start, end, speaker)| SonioxToken {
                        text: text.into(), start_ms: Some(start), end_ms: Some(end), speaker: Some(serde_json::json!(speaker)),
                        language: None, translation_status: None, is_audio_event: None, extra: Default::default(),
                    }).collect(),
                text: None, metadata: serde_json::Value::Null, extra: Default::default(),
            }),
        },
    }
}

async fn intake(app: &Jobs, attempt: &Attempt, observation: &ProviderObservation) -> InboxId {
    match app.intake(attempt, observation.clone(), ingest::fingerprint(observation)).await.unwrap() {
        IntakeAck::Stored { inbox } => inbox,
        other => panic!("first callback was not durably accepted: {other:?}"),
    }
}

async fn events(store: &PgJobStore, account: AccountId) -> Vec<(i64, JobEvent)> {
    let rows: Vec<(i64, Json<JobEvent>)> = sqlx::query_as(
        "SELECT id, event FROM transgate_jobs.events WHERE account_id = $1 ORDER BY id",
    ).bind(account.0).fetch_all(store.pool()).await.unwrap();
    rows.into_iter().map(|(id, Json(event))| (id, event)).collect()
}

async fn usage(store: &PgJobStore, account: AccountId) -> (i64, i64) {
    sqlx::query_as("SELECT count(*), COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1")
        .bind(account.0).fetch_one(store.pool()).await.unwrap()
}

async fn applied(store: &PgJobStore, inbox: InboxId) {
    let mut uow = store.begin().await.unwrap();
    let entry = uow.lock_inbox(inbox).await.unwrap().unwrap();
    assert_eq!(entry.state, InboxState::Applied);
    assert_eq!((entry.failures, entry.note), (0, None));
    uow.rollback().await.unwrap();
    let last_error: Option<String> = sqlx::query_scalar("SELECT last_error FROM transgate_jobs.inbox WHERE id = $1")
        .bind(inbox.0).fetch_one(store.pool()).await.unwrap();
    assert_eq!(last_error, None);
}

async fn completed_transcript(store: &PgJobStore, account: AccountId, id: JobId, deferred: DeferredCharge) {
    let job = store.owned_job(account, id).await.unwrap().unwrap();
    assert_eq!((job.status, job.duration_minutes, job.deferred_charge), (JobStatus::Completed, Some(3), deferred));
    assert_eq!(job.error, None, "a debit failure is not a transcription/partial-result error");
    let lines = store.lines(id).await.unwrap();
    let actual: Vec<_> = lines.iter().map(|l| (l.line.as_str(), l.start_time.as_str(), l.end_time.as_str(), l.speaker.as_deref())).collect();
    assert_eq!(actual, vec![
        ("First complete sentence.", "00:00:00", "00:00:02", Some("SPEAKER_00")),
        ("Second complete sentence.", "00:00:03", "00:00:05", Some("SPEAKER_01")),
    ], "the complete normalized transcript commits, not a one-line placeholder");
    assert!(lines.iter().all(|l| l.account == account && l.job == id));
    let stored_events = events(store, account).await;
    assert_eq!(stored_events.len(), 1);
    assert_eq!(stored_events[0].1, JobEvent::Completed {
        job: id, account, provider: Some(Provider::Soniox), language: Language::from_code("en"),
        error: None, callback_url: None, at: NOW,
    });
    assert!(mails_for(&stored_events[0].1).is_empty(), "no failed-file or partial-error notification for a refused debit");
}

/// Replay after the fault is removed (or balance replenished) must not retry a refused debit.
async fn replay_is_inert(app: &Jobs, store: &PgJobStore, attempt: &Attempt, observation: &ProviderObservation, inbox: InboxId) {
    let before_job = store.owned_job(attempt.account, attempt.job).await.unwrap().unwrap();
    let before_lines = store.lines(attempt.job).await.unwrap();
    let before_events = events(store, attempt.account).await;
    let before_usage = usage(store, attempt.account).await;
    for _ in 0..2 {
        assert_eq!(app.intake(attempt, observation.clone(), ingest::fingerprint(observation)).await.unwrap(), IntakeAck::Duplicate { inbox });
        app.ingest(inbox).await.unwrap();
    }
    assert_eq!(store.owned_job(attempt.account, attempt.job).await.unwrap().unwrap(), before_job);
    assert_eq!(store.lines(attempt.job).await.unwrap(), before_lines, "line ids and text are unchanged");
    assert_eq!(events(store, attempt.account).await, before_events, "no new completion/failure event");
    assert_eq!(usage(store, attempt.account).await, before_usage, "no replay charge or partial allocation");
    applied(store, inbox).await;
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL with CREATEDB"]
async fn bil_036_jobs_ingest_delivers_once_when_the_deferred_balance_is_insufficient() {
    isolated(|store, _owner| async move {
        let (app, _) = app(&store);
        let account = account(&store).await;
        package(&store, account, 1, 10).await;
        let id = draft(&app, account, None, "unknown-length.mp3").await;
        app.start(account, start(vec![id])).await.unwrap();
        let attempt = running_attempt(&store, account, id).await;
        assert_eq!(store.owned_job(account, id).await.unwrap().unwrap().deferred_charge, DeferredCharge::Pending);
        assert_eq!(usage(&store, account).await, (0, 0));
        let observation = speech(&attempt);
        let inbox = intake(&app, &attempt, &observation).await;
        app.ingest(inbox).await.unwrap();
        completed_transcript(&store, account, id, DeferredCharge::Refused).await;
        assert_eq!(usage(&store, account).await, (0, 0));
        applied(&store, inbox).await;
        // The same callback is not permission to bill later when the owner buys minutes.
        package(&store, account, 10, 20).await;
        replay_is_inert(&app, &store, &attempt, &observation, inbox).await;
    }).await;
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL with CREATEDB"]
async fn bil_036_jobs_ingest_rolls_back_a_partial_sql_debit_but_commits_delivery_once() {
    isolated(|store, owner| async move {
        let (app, _) = app(&store);
        let account = account(&store).await;
        package(&store, account, 1, 10).await;
        let second = package(&store, account, 4, 20).await;
        let id = draft(&app, account, None, "split-debit.mp3").await;
        app.start(account, start(vec![id])).await.unwrap();
        let attempt = running_attempt(&store, account, id).await;
        // Real SQL error on allocation two, after allocation one has been inserted. The
        // constraint is private to this test's database and also scoped to this account.
        sqlx::query(AssertSqlSafe(format!(
            "ALTER TABLE billing.usage ADD CONSTRAINT consumer_second_allocation CHECK (account_id <> '{}'::uuid OR package_id <> {second})",
            account.0,
        ))).execute(&owner).await.unwrap();
        let error = sqlx::query("INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at) VALUES ($1, $2, $3, 2, billing.us_to_ts($4))")
            .bind(account.0).bind(second).bind(id.0).bind(NOW.0).execute(store.pool()).await.unwrap_err();
        let db_error = error.as_database_error().unwrap();
        assert_eq!(db_error.code().as_deref(), Some("23514"));
        assert_eq!(db_error.constraint(), Some("consumer_second_allocation"));
        let before: i64 = sqlx::query_scalar("SELECT last_value FROM billing.usage_id_seq").fetch_one(&owner).await.unwrap();
        let observation = speech(&attempt);
        let inbox = intake(&app, &attempt, &observation).await;
        app.ingest(inbox).await.unwrap();
        let after: i64 = sqlx::query_scalar("SELECT last_value FROM billing.usage_id_seq").fetch_one(&owner).await.unwrap();
        assert_eq!(after - before, 2, "the real debit attempted both allocation INSERTs, including the SQL fault");
        completed_transcript(&store, account, id, DeferredCharge::Refused).await;
        assert_eq!(usage(&store, account).await, (0, 0), "the first allocation was rolled back with the rejected second one");
        applied(&store, inbox).await;
        sqlx::query("ALTER TABLE billing.usage DROP CONSTRAINT consumer_second_allocation").execute(&owner).await.unwrap();
        replay_is_inert(&app, &store, &attempt, &observation, inbox).await;
    }).await;
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL with CREATEDB"]
async fn job_031_jobs_ingest_rolls_back_partial_lines_and_emits_one_coded_failure() {
    isolated(|store, owner| async move {
        let (app, _) = app(&store);
        let account = account(&store).await;
        package(&store, account, 10, 10).await;
        let id = draft(&app, account, None, "line-insert-failure.mp3").await;
        app.start(account, start(vec![id])).await.unwrap();
        let attempt = running_attempt(&store, account, id).await;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER TABLE transgate_jobs.lines ADD CONSTRAINT consumer_second_line CHECK (account_id <> '{}'::uuid OR line <> 'Second complete sentence.')",
            account.0,
        ))).execute(&owner).await.unwrap();
        let observation = speech(&attempt);
        let inbox = intake(&app, &attempt, &observation).await;
        app.ingest(inbox).await.unwrap();
        let job = store.owned_job(account, id).await.unwrap().unwrap();
        assert_eq!((job.status, job.duration_minutes, job.deferred_charge), (JobStatus::Failed, Some(3), DeferredCharge::Charged));
        assert_eq!(usage(&store, account).await, (1, 3), "source order: the deferred debit precedes the failed line insert, with no refund");
        assert!(store.lines(id).await.unwrap().is_empty(), "no partial transcript after the second line's SQL failure");
        let error = job.error.as_ref().unwrap();
        assert_eq!(error["code"], "database_error");
        assert_eq!(error["provider"], "soniox");
        assert_eq!(error["details"]["segmentsCount"], 2);
        assert_eq!(error["details"]["jobId"], id.0);
        assert!(error["details"]["errorMessage"].as_str().unwrap().contains("consumer_second_line"));
        let stored_events = events(&store, account).await;
        assert_eq!(stored_events.len(), 1);
        assert_eq!(stored_events[0].1, JobEvent::Failed { job: id, account, language: Language::from_code("en"), error: job.error.clone(), at: NOW });
        let mails = mails_for(&stored_events[0].1);
        assert!(matches!(mails.as_slice(), [JobMail::UserFailedFile, JobMail::SupportJobFailed { .. }]));
        assert_eq!(store.attempt(attempt.id).await.unwrap().unwrap().state, AttemptState::Finished);
        applied(&store, inbox).await;
        sqlx::query("ALTER TABLE transgate_jobs.lines DROP CONSTRAINT consumer_second_line").execute(&owner).await.unwrap();
        replay_is_inert(&app, &store, &attempt, &observation, inbox).await;
    }).await;
}

async fn assert_later_job_completes(app: &Jobs, store: &PgJobStore, account: AccountId, first: JobId, second: JobId) {
    let failed = store.owned_job(account, first).await.unwrap().unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.error.as_ref().unwrap()["code"], "invalid_request");
    assert!(failed.error.as_ref().unwrap().to_string().contains("invalid_audio_fixture"));
    let initial_events = events(store, account).await;
    assert_eq!(initial_events.len(), 1);
    assert!(matches!(&initial_events[0].1, JobEvent::Failed { job, .. } if *job == first));
    let attempt = running_attempt(store, account, second).await;
    let observation = speech(&attempt);
    let inbox = intake(app, &attempt, &observation).await;
    app.ingest(inbox).await.unwrap();
    assert_eq!(store.owned_job(account, second).await.unwrap().unwrap().status, JobStatus::Completed);
    assert_eq!(store.lines(second).await.unwrap().iter().map(|l| l.line.as_str()).collect::<Vec<_>>(), ["First complete sentence.", "Second complete sentence."]);
    assert!(store.lines(first).await.unwrap().is_empty());
    let stored_events = events(store, account).await;
    assert_eq!(stored_events.len(), 2);
    assert!(matches!(&stored_events[0].1, JobEvent::Failed { job, .. } if *job == first));
    assert!(matches!(&stored_events[1].1, JobEvent::Completed { job, error: None, .. } if *job == second));
    replay_is_inert(app, store, &attempt, &observation, inbox).await;
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL with CREATEDB"]
async fn job_011_012_start_reports_the_first_refusal_and_delivers_the_later_job() {
    isolated(|store, _owner| async move {
        let (app, provider) = app(&store);
        let account = account(&store).await;
        package(&store, account, 10, 10).await;
        let first = draft(&app, account, Some(1), "bad-audio.mp3").await;
        let second = draft(&app, account, Some(1), "good-audio.mp3").await;
        *provider.refused.lock().unwrap() = Some(first);
        // Request order differs; the transactional start loads drafts by ascending id.
        let answer = app.start(account, start(vec![second, first])).await.unwrap();
        assert_eq!(answer.results.iter().map(|r| (r.job_id, r.status)).collect::<Vec<_>>(), [(first, JobStatus::Failed), (second, JobStatus::Running)]);
        assert!(answer.results[0].error.as_ref().unwrap().contains("invalid_audio_fixture"));
        assert_eq!(answer.results[1].error, None);
        assert_eq!(provider.sent.lock().unwrap().iter().map(|r| r.client_reference.clone()).collect::<Vec<_>>(), [format!("job_{}", first.0), format!("job_{}", second.0)]);
        assert_eq!(usage(&store, account).await, (2, 2), "each known duration was charged at start, even the refused submission");
        assert_later_job_completes(&app, &store, account, first, second).await;
        assert_eq!(usage(&store, account).await, (2, 2), "provider's higher length never adds a charge to a known duration");
    }).await;
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL with CREATEDB"]
async fn job_019_dispatch_pass_continues_after_a_definite_refusal_and_never_recharges() {
    isolated(|store, _owner| async move {
        let (app, provider) = app(&store);
        let account = account(&store).await;
        package(&store, account, 10, 10).await;
        // Durable unsubmitted queue rows, as recovered after restart. Pending has priority
        // even though the second not_started row is older. Their null duration defers billing.
        let mut uow = store.begin().await.unwrap();
        uow.billing().await.unwrap().lock_account(account).await.unwrap();
        let first = uow.insert_job(&transgate_core::jobs::domain::NewJob {
            duration_minutes: None, ..new_job(account, JobStatus::Pending)
        }).await.unwrap().id;
        let second = uow.insert_job(&transgate_core::jobs::domain::NewJob {
            duration_minutes: None, created_at: Micros(NOW.0 - MICROS_PER_DAY), ..new_job(account, JobStatus::NotStarted)
        }).await.unwrap().id;
        uow.commit().await.unwrap();
        *provider.refused.lock().unwrap() = Some(first);
        let report = app.dispatch_pass().await.unwrap();
        assert_eq!((report.examined, report.failed, report.submitted, report.errors, report.waiting, report.rate_limited), (2, 1, 1, 0, 0, 0));
        assert_eq!(provider.sent.lock().unwrap().iter().map(|r| r.client_reference.clone()).collect::<Vec<_>>(), [format!("job_{}", first.0), format!("job_{}", second.0)]);
        assert_eq!(usage(&store, account).await, (0, 0), "dispatch alone does not charge a null duration");
        assert_later_job_completes(&app, &store, account, first, second).await;
        assert_eq!(usage(&store, account).await, (1, 3));
        let before_events = events(&store, account).await;
        let report = app.dispatch_pass().await.unwrap();
        assert_eq!(report.examined, 0, "terminal jobs are not dispatched by a later pass");
        assert_eq!(events(&store, account).await, before_events);
        assert_eq!(usage(&store, account).await, (1, 3));
        assert_eq!(provider.sent.lock().unwrap().len(), 2, "neither terminal job was submitted again");
    }).await;
}
