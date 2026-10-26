//! transgate-jobs-postgres against a real database, through the restricted
//! billing runtime login the server uses.
//!
//! Run with `TRANSGATE_JOBS_TEST_DATABASE_URL=... cargo test -p transgate-jobs-postgres -- --include-ignored`.
//! The URL names a disposable database with owner authority; a missing variable
//! fails the test, it never skips. Every test uses random accounts.

use std::time::Duration;

use billing_core::ports::StoreError;
use billing_core::types::{AccountId, Micros};
use media_ports::{
    DeepgramMetadata, DeepgramResults, DeepgramTranscript, ObjectRef, ObservationOrigin, ProviderObservation,
    ProviderPayload, RemoteIdentity, SecretString, SonioxReported, SonioxToken, SonioxTranscript, StoreKind,
};
use sqlx::{AssertSqlSafe, PgPool};
use tokio::sync::OnceCell;
use transgate_core::ids::{AttemptId, ErasureId, JobId, LineId, MICROS_PER_DAY, MICROS_PER_MINUTE};
use transgate_core::jobs::domain::{
    Attempt, AttemptRequest, AttemptState, JobSource, JobStatus, NewJob, Provider, RemoteCleanup, StorageProvider,
};
use transgate_core::jobs::events::JobEvent;
use transgate_core::jobs::ports::{
    ArchiveWrite, InboxState, IssuedUpload, JobListQuery, JobQueries, JobStore, LockGuard, Locks, NewInboxEntry, RateLimiter,
};
use transgate_core::languages::Language;
use transgate_core::privacy::erasure::{ErasureItem, ErasureRecord, ErasureState, ItemState};
use transgate_core::privacy::retention::RetentionCandidate;
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::NewLine;
use transgate_jobs_postgres::{PgJobStore, PgLocks, PgRateLimiter};
use uuid::Uuid;

#[path = "store/consumer_failures.rs"]
mod consumer_failures;

const RUNTIME_LOGIN: &str = "transgate_jobs_test_runtime";
const RUNTIME_PASSWORD: &str = "transgate-jobs-test-runtime-password";
const NOW: Micros = Micros(1_768_478_400_000_000); // 2026-01-15T12:00:00Z

fn database_url() -> String {
    std::env::var("TRANSGATE_JOBS_TEST_DATABASE_URL")
        .expect("TRANSGATE_JOBS_TEST_DATABASE_URL must name a disposable PostgreSQL database for these tests")
}

static PROVISIONED: OnceCell<()> = OnceCell::const_new();

async fn owner() -> PgPool {
    let owner = billing_postgres::connect_owner(&database_url()).await.expect("owner connection");
    PROVISIONED
        .get_or_init(|| async {
            billing_postgres::migrate(&owner).await.expect("billing migrations");
            transgate_jobs_postgres::migrate(&owner).await.expect("jobs migrations");
            // A rerun is a no-op beside billing's and media's histories.
            transgate_jobs_postgres::migrate(&owner).await.expect("jobs migrations rerun");
            billing_postgres::provision_runtime_login(&owner, RUNTIME_LOGIN, RUNTIME_PASSWORD).await.expect("runtime login");
        })
        .await;
    owner
}

/// The server's pool: billing's verified runtime login, then this crate's check.
async fn runtime() -> PgPool {
    runtime_with(8).await
}

async fn runtime_with(max_connections: u32) -> PgPool {
    owner().await;
    let options = billing_postgres::options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap();
    let pool = billing_postgres::connect_runtime(options, max_connections).await.expect("restricted runtime pool");
    transgate_jobs_postgres::verify_runtime(&pool).await.expect("restricted transgate runtime");
    pool
}

fn object(key: &str) -> ObjectRef {
    ObjectRef { store: StoreKind::Gcs, bucket: "uploads".into(), key: key.into(), version: None }
}

fn new_job(account: AccountId, status: JobStatus) -> NewJob {
    NewJob {
        account,
        status,
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
        created_at: NOW,
    }
}

fn attempt(job: JobId, account: AccountId, number: i32, state: AttemptState, remote: Option<&str>) -> Attempt {
    Attempt {
        id: AttemptId::new(),
        job,
        account,
        number,
        provider: Provider::Soniox,
        project: "project".into(),
        model: "stt-async-v5".into(),
        state,
        claimed_at: NOW,
        lease_until: Micros(NOW.0 + 2 * MICROS_PER_MINUTE),
        request: AttemptRequest {
            language_code: Some("en".into()),
            translation_target: None,
            diarize: true,
            audio: object("audio"),
            client_reference: format!("job_{}", job.0),
        },
        callback_secret: SecretString::new("per-attempt-secret"),
        remote_id: remote.map(str::to_string),
        accepted_at: remote.map(|_| NOW),
        last_error: None,
        remote_cleanup: RemoteCleanup::NotRequested,
    }
}

fn line(start: &str, text: &str) -> NewLine {
    NewLine {
        start_time: start.into(),
        end_time: start.into(),
        line: text.into(),
        speaker: Some("SPEAKER_00".into()),
        corrected_text: None,
        split_on_speaker_change: None,
        language: None,
        translation_status: None,
    }
}

fn soniox_observation(remote: &str) -> ProviderObservation {
    ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Soniox, project: "project".into(), id: remote.into() },
        origin: ObservationOrigin::Webhook,
        payload: ProviderPayload::Soniox {
            reported: SonioxReported {
                id: remote.into(),
                status: "completed".into(),
                audio_duration_ms: Some(61_000.0),
                error: None,
                error_type: None,
                error_message: None,
                extra: Default::default(),
            },
            transcript: Some(SonioxTranscript {
                id: remote.into(),
                tokens: vec![SonioxToken {
                    text: "private words".into(),
                    start_ms: Some(0.0),
                    end_ms: Some(900.0),
                    speaker: None,
                    language: None,
                    translation_status: None,
                    is_audio_event: None,
                    extra: Default::default(),
                }],
                text: None,
                metadata: serde_json::Value::Null,
                extra: Default::default(),
            }),
        },
    }
}

/// A provisioned billing account and one job in `status`, committed.
async fn account_with_job(store: &PgJobStore, status: JobStatus) -> (AccountId, JobId) {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    assert!(uow.billing().await.unwrap().create_account(account, NOW).await.unwrap());
    let job = uow.insert_job(&new_job(account, status)).await.unwrap();
    uow.commit().await.unwrap();
    (account, job.id)
}

async fn wait_for_lock(owner: &PgPool, pid: i32) -> bool {
    for _ in 0..100 {
        let waiting: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type IN ('Lock', 'Advisory'))")
                .bind(pid)
                .fetch_one(owner)
                .await
                .unwrap();
        if waiting {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

fn sql_state(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> String {
    result.unwrap_err().as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default()
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_runtime_login_is_restricted_and_cannot_remove_a_barrier_or_write_operator_settings() {
    let pool = runtime().await;
    let owner = owner().await;
    assert!(transgate_jobs_postgres::runtime_authority_problems(&owner, transgate_jobs_postgres::RUNTIME_ROLE).await.unwrap().is_empty());
    for (sql, why) in [
        ("DELETE FROM transgate_jobs.erasures", "the barrier survives (D9)"),
        ("DELETE FROM transgate_jobs.retention_runs", "audit rows are kept"),
        ("UPDATE transgate_jobs.settings SET unlimited_history = true", "operator field (C6)"),
        ("UPDATE transgate_jobs.jobs SET account_id = gen_random_uuid()", "a job never changes owner"),
        ("UPDATE transgate_jobs.lines SET job_id = 1", "an edit never moves a line"),
        ("CREATE TABLE transgate_jobs.extra (id int)", "no DDL"),
    ] {
        assert_eq!(sql_state(sqlx::query(sql).execute(&pool).await), "42501", "{why}: {sql}");
    }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_lifecycle_save_is_a_revision_compare_and_swap_and_job_locks_are_owner_scoped() {
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Draft).await;
    let (_, foreign) = account_with_job(&store, JobStatus::Draft).await;

    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let locked = uow.lock_jobs(account, &[foreign, id, id]).await.unwrap();
    assert_eq!(locked.iter().map(|j| j.id).collect::<Vec<_>>(), vec![id], "a foreign id is absent");
    let mut job = locked[0].clone();
    assert_eq!((job.revision, job.language, job.duration_minutes), (1, Language::from_code("en"), Some(3)));
    job.start(NOW).unwrap();
    assert_eq!(uow.save_job(&job, 1).await.unwrap(), 2);
    let stale = uow.save_job(&job, 1).await;
    assert!(matches!(stale, Err(StoreError::Conflict(_))), "{stale:?}");
    uow.commit().await.unwrap();

    let stored = store.owned_job(account, id).await.unwrap().unwrap();
    assert_eq!((stored.status, stored.revision, stored.next_dispatch_at), (JobStatus::NotStarted, 2, Some(NOW)));
    assert!(store.owned_job(AccountId(Uuid::new_v4()), id).await.unwrap().is_none());
    let listed = store
        .list_jobs(&JobListQuery { account, statuses: vec![JobStatus::NotStarted], since: None, before: None, limit: 10 })
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn two_processes_claiming_the_same_provider_capacity_serialize_on_the_capacity_lock() {
    let owner = owner().await;
    let first = PgJobStore::new(runtime().await);
    // One connection, so the backend that reports its pid is the one that waits.
    let second = PgJobStore::new(runtime_with(1).await);
    let project = format!("project-{}", Uuid::new_v4().simple());

    let mut holder = first.begin().await.unwrap();
    holder.lock_capacity(media_ports::Provider::Deepgram, &project).await.unwrap();

    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(second.pool()).await.unwrap();
    let waiter = tokio::spawn(async move {
        let mut uow = second.begin().await.unwrap();
        uow.lock_capacity(media_ports::Provider::Deepgram, &project).await.unwrap();
        uow.rollback().await.unwrap();
    });
    assert!(wait_for_lock(&owner, pid).await, "the second claim waits on the capacity lock");
    assert!(!waiter.is_finished());
    holder.commit().await.unwrap();
    waiter.await.unwrap();

    // In flight: live claims and accepted attempts of jobs that still wait.
    let store = first;
    let (account, id) = account_with_job(&store, JobStatus::NotStarted).await;
    let mut uow = store.begin().await.unwrap();
    let mut live = attempt(id, account, 1, AttemptState::Claimed, None);
    live.provider = Provider::Deepgram;
    live.project = "inflight".into();
    uow.insert_attempt(&live).await.unwrap();
    assert_eq!(uow.inflight(media_ports::Provider::Deepgram, "inflight", NOW).await.unwrap(), 1);
    let expired = Micros(NOW.0 + 3 * MICROS_PER_MINUTE);
    assert_eq!(uow.inflight(media_ports::Provider::Deepgram, "inflight", expired).await.unwrap(), 0, "an expired claim no longer holds capacity");
    uow.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn durable_intake_returns_the_stored_entry_for_a_replay_or_conflict_and_settling_drops_the_body() {
    let store = PgJobStore::new(runtime().await);
    let owner = owner().await;
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let a = attempt(id, account, 1, AttemptState::Accepted, Some("remote-1"));
    let entry = NewInboxEntry {
        attempt: a.id,
        job: id,
        account,
        remote_id: "remote-1".into(),
        origin: ObservationOrigin::Webhook,
        observation: soniox_observation("remote-1"),
        fingerprint: "fp-1".into(),
        received_at: NOW,
    };
    let mut uow = store.begin().await.unwrap();
    uow.insert_attempt(&a).await.unwrap();
    let (stored, new) = uow.insert_inbox(&entry).await.unwrap();
    assert!(new);
    assert_eq!(stored.entry, entry, "the observation survives intake losslessly");
    let (replay, new) = uow.insert_inbox(&entry).await.unwrap();
    assert!(!new && replay.id == stored.id);
    let conflicting = NewInboxEntry { fingerprint: "fp-2".into(), ..entry.clone() };
    let (held, new) = uow.insert_inbox(&conflicting).await.unwrap();
    assert!(!new && held.entry.fingerprint == "fp-1", "a conflicting payload is not stored over the first");
    uow.record_inbox_failure(stored.id, "transient").await.unwrap();
    uow.settle_inbox(stored.id, InboxState::Applied, None, NOW).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let settled = uow.lock_inbox(stored.id).await.unwrap().unwrap();
    uow.rollback().await.unwrap();
    assert_eq!((settled.state, settled.failures, settled.settled_at), (InboxState::Applied, 1, Some(NOW)));
    assert_eq!(settled.entry.fingerprint, "fp-1");
    match settled.entry.observation.payload {
        ProviderPayload::Soniox { reported, transcript } => {
            assert!(transcript.is_none(), "the transcript content is dropped");
            assert_eq!((reported.id.as_str(), reported.audio_duration_ms), ("remote-1", Some(61_000.0)), "D6 duration as received");
        }
        other => panic!("{other:?}"),
    }
    let raw: String = sqlx::query_scalar("SELECT observation::text FROM transgate_jobs.inbox WHERE id = $1")
        .bind(stored.id.0)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert!(!raw.contains("private words"));
    assert!(store.pending_inbox(100).await.unwrap().iter().all(|e| e.id != stored.id));

    // A Deepgram body keeps only its request id and duration.
    let mut uow = store.begin().await.unwrap();
    let d = attempt(id, account, 2, AttemptState::Accepted, Some("req-1"));
    uow.insert_attempt(&d).await.unwrap();
    let deepgram = ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Deepgram, project: "project".into(), id: "req-1".into() },
        origin: ObservationOrigin::Webhook,
        payload: ProviderPayload::Deepgram(DeepgramTranscript {
            metadata: DeepgramMetadata { request_id: "req-1".into(), duration: Some(12.5), extra: Default::default() },
            results: DeepgramResults { channels: vec![], extra: Default::default() },
            extra: Default::default(),
        }),
    };
    let (entry, _) = uow
        .insert_inbox(&NewInboxEntry { attempt: d.id, remote_id: "req-1".into(), observation: deepgram, fingerprint: "fp-d".into(), ..entry })
        .await
        .unwrap();
    uow.settle_inbox(entry.id, InboxState::Ignored, Some("late result"), NOW).await.unwrap();
    let ignored = uow.lock_inbox(entry.id).await.unwrap().unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(ignored.note.as_deref(), Some("late result"));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_rolled_back_savepoint_undoes_only_its_own_writes() {
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let mut job = uow.lock_job(id).await.unwrap().unwrap();
    job.set_duration(7, NOW);
    let revision = uow.save_job(&job, job.revision).await.unwrap();
    uow.insert_lines(&job, &[line("00:00:01", "kept")]).await.unwrap();
    uow.savepoint("deferred_debit").await.unwrap();
    uow.insert_lines(&job, &[line("00:00:02", "rolled back")]).await.unwrap();
    uow.rollback_to_savepoint("deferred_debit").await.unwrap();
    uow.savepoint("lines").await.unwrap();
    uow.release_savepoint("lines").await.unwrap();
    assert!(matches!(uow.savepoint("x; DROP TABLE y").await, Err(StoreError::Conflict(_))));
    uow.commit().await.unwrap();
    let lines = store.lines(id).await.unwrap();
    assert_eq!(lines.iter().map(|l| l.line.as_str()).collect::<Vec<_>>(), vec!["kept"]);
    assert_eq!(store.owned_job(account, id).await.unwrap().unwrap().revision, revision);
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn d7_line_edits_update_only_and_lines_sort_by_stored_text() {
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Completed).await;
    let mut uow = store.begin().await.unwrap();
    let job = uow.lock_job(id).await.unwrap().unwrap();
    uow.insert_lines(&job, &[line("00:00:10", "b"), line("00:00:02", "a"), line("00:00:10", "c")]).await.unwrap();
    uow.commit().await.unwrap();
    let lines = store.lines(id).await.unwrap();
    assert_eq!(lines.iter().map(|l| l.line.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
    assert!(lines.iter().all(|l| l.account == account));

    let mut uow = store.begin().await.unwrap();
    assert!(uow.update_line_text(lines[0].id, "edited").await.unwrap());
    assert!(uow.update_line_speaker(lines[0].id, "SPEAKER_01").await.unwrap());
    assert!(!uow.update_line_text(LineId(i64::MAX), "resurrected").await.unwrap(), "a missing line is never inserted");
    assert_eq!(uow.rename_speaker(id, "SPEAKER_00", "Host").await.unwrap(), 2);
    uow.commit().await.unwrap();
    let lines = store.lines(id).await.unwrap();
    assert_eq!(lines[0].line, "edited");
    assert_eq!(lines.iter().filter_map(|l| l.speaker.as_deref()).collect::<Vec<_>>(), vec!["SPEAKER_01", "Host", "Host"]);
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn worker_passes_skip_live_attempts_and_accounts_under_erasure() {
    let store = PgJobStore::new(runtime().await);
    let (account, free) = account_with_job(&store, JobStatus::NotStarted).await;
    let mut uow = store.begin().await.unwrap();
    let mut job = uow.lock_job(free).await.unwrap().unwrap();
    job.next_dispatch_at = Some(NOW);
    uow.save_job(&job, job.revision).await.unwrap();
    let held = uow.insert_job(&new_job(account, JobStatus::Pending)).await.unwrap();
    let claim = attempt(held.id, account, 1, AttemptState::Claimed, None);
    uow.insert_attempt(&claim).await.unwrap();
    let mut held = uow.lock_job(held.id).await.unwrap().unwrap();
    held.current_attempt = Some(claim.id);
    uow.save_job(&held, held.revision).await.unwrap();
    uow.commit().await.unwrap();

    let due: Vec<JobId> = store.dispatch_candidates(NOW, 1000).await.unwrap().iter().map(|j| j.id).collect();
    assert!(due.contains(&free) && !due.contains(&held.id), "a live claim holds its job");
    let later = Micros(NOW.0 + 3 * MICROS_PER_MINUTE);
    let due: Vec<JobId> = store.dispatch_candidates(later, 1000).await.unwrap().iter().map(|j| j.id).collect();
    assert!(due.contains(&held.id), "an expired claim no longer holds it");
    assert!(store.unresolved_attempts(later, 1000).await.unwrap().iter().any(|a| a.id == claim.id));

    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().open_erasure(account, NOW).await.unwrap();
    let record = erasure_record(account);
    uow.insert_erasure(&record).await.unwrap();
    uow.commit().await.unwrap();
    assert!(store.barrier(account).await.unwrap());
    let due: Vec<JobId> = store.dispatch_candidates(later, 1000).await.unwrap().iter().map(|j| j.id).collect();
    assert!(!due.contains(&free) && !due.contains(&held.id));
    assert!(!store.unresolved_attempts(later, 1000).await.unwrap().iter().any(|a| a.id == claim.id));
    assert!(store.erasures_due(NOW, 1000).await.unwrap().contains(&record.id), "a requested erasure is runnable");
    let mut uow = store.begin().await.unwrap();
    let duplicate = uow.insert_erasure(&erasure_record(account)).await;
    assert!(matches!(duplicate, Err(StoreError::Conflict(_))), "one barrier per account: {duplicate:?}");
}

fn erasure_record(account: AccountId) -> ErasureRecord {
    ErasureRecord {
        id: ErasureId::new(),
        account,
        requested_at: NOW,
        state: ErasureState::Requested,
        local_erased_at: None,
        identity_deleted_at: None,
        closed_at: None,
    }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_manifest_captures_superseded_unresolved_and_deleted_draft_resources_and_the_cascade_keeps_the_barrier() {
    let store = PgJobStore::new(runtime().await);
    let owner = owner().await;
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let mut uow = store.begin().await.unwrap();
    let superseded = attempt(id, account, 1, AttemptState::Superseded, Some("old-remote"));
    let current = attempt(id, account, 2, AttemptState::Accepted, Some("new-remote"));
    let unknown = attempt(id, account, 3, AttemptState::Uncertain, None);
    let rejected = attempt(id, account, 4, AttemptState::Rejected, None);
    for a in [&superseded, &current, &unknown, &rejected] {
        uow.insert_attempt(a).await.unwrap();
    }
    let job = uow.lock_job(id).await.unwrap().unwrap();
    uow.insert_lines(&job, &[line("00:00:01", "secret")]).await.unwrap();
    uow.emit(&JobEvent::Failed { job: id, account, language: None, error: None, at: NOW }).await.unwrap();
    let draft = uow.insert_job(&new_job(account, JobStatus::Draft)).await.unwrap();
    let upload = IssuedUpload { account, job: draft.id, object: object("draft-upload"), expires_at: Micros(NOW.0 + MICROS_PER_DAY) };
    uow.record_issued_upload(&upload).await.unwrap();
    uow.hard_delete_job(draft.id).await.unwrap();
    uow.save_settings(&UserSettings { auto_delete_transcripts: false, ..UserSettings::defaults(account) }).await.unwrap();
    uow.billing().await.unwrap().open_erasure(account, NOW).await.unwrap();
    let mut record = erasure_record(account);
    uow.insert_erasure(&record).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let resources = uow.account_resources(account).await.unwrap();
    assert_eq!(resources.jobs, vec![id], "the hard-deleted draft is gone");
    assert_eq!(resources.media.len(), 1);
    assert_eq!(resources.provider_copies.len(), 2);
    for remote in ["old-remote", "new-remote"] {
        assert!(
            resources.provider_copies.iter().any(|(_, identity)| identity.id == remote),
            "superseded and current remote identities must both be captured: {remote}"
        );
    }
    assert_eq!(resources.unresolved_submissions, vec![(Provider::Soniox, format!("job_{}", id.0))], "only acceptance-unknown attempts");
    assert_eq!(resources.issued_uploads, vec![upload], "the deleted draft's upload capability still needs its re-sweep");

    let subjects: Vec<i64> = resources.jobs.iter().map(|j| j.subject()).collect();
    {
        let mut billing = uow.billing().await.unwrap();
        billing_core::erasure::erase_account_billing(&mut *billing, account, &subjects, NOW).await.unwrap();
    }
    uow.erase_account_rows(account).await.unwrap();
    let items = transgate_core::privacy::erasure::build_items(record.id, &resources, NOW);
    uow.insert_erasure_items(&items).await.unwrap();
    record.state = ErasureState::LocalErased;
    record.local_erased_at = Some(NOW);
    uow.save_erasure(&record).await.unwrap();
    uow.commit().await.unwrap();

    for table in ["jobs", "lines", "attempts", "inbox", "events", "settings", "issued_uploads", "archived_payloads"] {
        let n: i64 = sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM transgate_jobs.{table} WHERE account_id = $1")))
            .bind(account.0)
            .fetch_one(&owner)
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    assert!(store.barrier(account).await.unwrap(), "the barrier survives the cascade (D9)");
    assert!(store.attempt(current.id).await.unwrap().is_none());

    // Closing drops the details of finished items and keeps approved exceptions.
    let mut uow = store.begin().await.unwrap();
    let mut stored = uow.erasure_items(record.id).await.unwrap();
    assert_eq!(stored.len(), items.len());
    for (n, item) in stored.iter_mut().enumerate() {
        item.state = if n == 0 {
            ItemState::ApprovedException { reason: "provider_has_no_delete_api".into(), approved_by: "policy".into(), at: NOW }
        } else {
            ItemState::Confirmed { at: NOW }
        };
        uow.save_erasure_item(item).await.unwrap();
    }
    record.state = ErasureState::ErasedWithExceptions;
    record.closed_at = Some(NOW);
    uow.save_erasure(&record).await.unwrap();
    let left: Vec<ErasureItem> = uow.erasure_items(record.id).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(left.len(), 1);
    assert!(matches!(left[0].state, ItemState::ApprovedException { .. }));
    assert!(!store.erasures_due(NOW, 1000).await.unwrap().contains(&record.id));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn retention_treats_an_absent_settings_row_as_the_source_default_and_keeps_only_explicit_opt_outs() {
    let store = PgJobStore::new(runtime().await);
    // PRV-013: the source column is NOT NULL DEFAULT true, and the settings read
    // answers true for an account without a row, so a newly enrolled account
    // without a row is swept like an explicit opt-in.
    let cutoff = Micros(NOW.0 - 31 * MICROS_PER_DAY);
    let mut uow = store.begin().await.unwrap();
    let opted_out_before = uow.opted_out_count(cutoff).await.unwrap();
    uow.rollback().await.unwrap();
    let mut ids = Vec::new();
    for (setting, created) in [(Some(true), Micros(NOW.0 - 40 * MICROS_PER_DAY)), (Some(false), Micros(NOW.0 - 40 * MICROS_PER_DAY)), (None, cutoff)] {
        let account = AccountId(Uuid::new_v4());
        let mut uow = store.begin().await.unwrap();
        uow.billing().await.unwrap().create_account(account, NOW).await.unwrap();
        let job = uow.insert_job(&NewJob { created_at: created, ..new_job(account, JobStatus::Completed) }).await.unwrap();
        uow.insert_lines(&job, &[line("00:00:01", "x"), line("00:00:02", "y")]).await.unwrap();
        if let Some(auto) = setting {
            uow.save_settings(&UserSettings { auto_delete_transcripts: auto, ..UserSettings::defaults(account) }).await.unwrap();
        }
        uow.commit().await.unwrap();
        assert_eq!(store.settings(account).await.unwrap().auto_delete_transcripts, setting.unwrap_or(true));
        ids.push(job.id);
    }
    let mut uow = store.begin().await.unwrap();
    let candidates: Vec<RetentionCandidate> =
        uow.retention_candidates(cutoff).await.unwrap().into_iter().filter(|c| ids.contains(&c.job)).collect();
    assert_eq!(
        candidates.iter().map(|c| (c.job, c.auto_delete, c.line_count)).collect::<Vec<_>>(),
        vec![(ids[0], true, 2), (ids[1], false, 2), (ids[2], true, 2)],
        "an absent row at the cutoff qualifies and deletes; only an explicit false keeps lines"
    );
    let locked = uow.lock_retention_set(&candidates, cutoff).await.unwrap();
    assert_eq!(locked, candidates);
    let deleting: Vec<JobId> = locked.iter().filter(|c| c.auto_delete).map(|c| c.job).collect();
    assert_eq!(uow.delete_lines_of(&deleting).await.unwrap(), 4);
    assert_eq!(uow.opted_out_count(cutoff).await.unwrap(), opted_out_before + 1, "only the explicit opt-out is counted");
    uow.commit().await.unwrap();
    assert!(store.lines(ids[0]).await.unwrap().is_empty());
    assert_eq!(store.lines(ids[1]).await.unwrap().len(), 2, "the explicit opt-out keeps its transcript");
    assert!(store.lines(ids[2]).await.unwrap().is_empty(), "the source default deletes the absent-row account's lines");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_rate_window_is_shared_by_replicas_starts_at_the_first_request_and_denials_do_not_count() {
    let owner = owner().await;
    let (a, b) = (PgRateLimiter::new(runtime().await), PgRateLimiter::new(runtime().await));
    let key = format!("203.0.113.{}:{}", 7, Uuid::new_v4().simple());
    let first = a.hit(&key, 2, 60_000, NOW).await.unwrap();
    assert!(first.allowed && first.remaining == 1 && first.reset_at == Micros(NOW.0 + 60_000_000));
    let second = b.hit(&key, 2, 60_000, Micros(NOW.0 + 1_000_000)).await.unwrap();
    assert!(second.allowed && second.remaining == 0 && second.reset_at == first.reset_at, "the window started at the first request");
    for _ in 0..3 {
        assert!(!a.hit(&key, 2, 60_000, Micros(NOW.0 + 2_000_000)).await.unwrap().allowed);
    }
    let hits: i32 = sqlx::query_scalar("SELECT hits FROM transgate_jobs.rate_windows WHERE key_hash = $1")
        .bind(Uuid::new_v5(&Uuid::from_u128(0x7472_616e_7367_6174_652d_7261_7465_6b31), key.as_bytes()))
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(hits, 2, "denied requests are not counted");
    let raw: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.rate_windows WHERE key_hash::text LIKE '%203.0.113%'")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(raw, 0);
    let fresh = b.hit(&key, 2, 60_000, Micros(NOW.0 + 60_000_000)).await.unwrap();
    assert!(fresh.allowed && fresh.remaining == 1 && fresh.reset_at == Micros(NOW.0 + 120_000_000), "a new window at expiry");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn an_advisory_lock_is_exclusive_across_processes_until_released() {
    let (a, b) = (PgLocks::new(runtime().await), PgLocks::new(runtime().await));
    let name = format!("cleanup_old_transcripts-{}", Uuid::new_v4().simple());
    let held: Box<dyn LockGuard> = a.try_lock(&name).await.unwrap().expect("first holder");
    assert!(b.try_lock(&name).await.unwrap().is_none());
    held.release().await;
    let again = b.try_lock(&name).await.unwrap().expect("free after release");
    again.release().await;
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn job_events_are_claimed_once_and_only_the_current_claim_acknowledges() {
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Completed).await;
    let mut uow = store.begin().await.unwrap();
    let event = JobEvent::Completed { job: id, account, provider: None, language: None, error: None, callback_url: None, at: NOW };
    uow.emit(&event).await.unwrap();
    uow.commit().await.unwrap();
    let far = Micros(NOW.0 + 10 * MICROS_PER_DAY);
    let claimed: Vec<_> = store.claim_events(1000, 60, far).await.unwrap().into_iter().filter(|e| e.event == event).collect();
    assert_eq!(claimed.len(), 1);
    assert!(store.claim_events(1000, 60, far).await.unwrap().iter().all(|e| e.event != event), "claimed exclusively");
    let later = Micros(far.0 + 61_000_000);
    let reclaimed: Vec<_> = store.claim_events(1000, 60, later).await.unwrap().into_iter().filter(|e| e.event == event).collect();
    assert_eq!(reclaimed.len(), 1, "an expired claim is reclaimed");
    assert!(!store.ack_event(&claimed[0], later).await.unwrap(), "the lost claim cannot acknowledge");
    assert!(store.ack_event(&reclaimed[0], later).await.unwrap());
    assert!(store.claim_events(1000, 60, Micros(later.0 + MICROS_PER_DAY)).await.unwrap().iter().all(|e| e.event != event));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_history_purge_and_its_audit_row_commit_together_and_interrupted_runs_become_failures() {
    let store = PgJobStore::new(runtime().await);
    let pool = store.pool().clone();
    let name = format!("cleanup-test-{}", Uuid::new_v4().simple());
    let cutoff = Micros(NOW.0 - 91 * MICROS_PER_DAY);
    let old = cutoff.0 - MICROS_PER_DAY;
    let claim = media_postgres::claim_slot(&pool, &name, old, old, 60).await.unwrap().unwrap();
    assert!(media_postgres::finish_slot(&pool, &claim, old + 1_000_000, "succeeded").await.unwrap());
    media_postgres::claim_slot(&pool, &name, old - MICROS_PER_DAY, old - MICROS_PER_DAY, 60).await.unwrap().unwrap();
    let scope = [name.as_str()];

    let days = transgate_core::privacy::retention::RetentionDays::new(Some(90)).unwrap();
    let mut run = transgate_core::privacy::retention::RetentionRun::started(days, NOW, "scheduler");
    run.job_name = format!("history-{}", Uuid::new_v4().simple());
    let mut uow = store.begin().await.unwrap();
    uow.insert_retention_run(&run).await.unwrap();
    assert_eq!(uow.history_count(&scope, cutoff).await.unwrap(), 1, "the unfinished claim is not history");
    assert_eq!(uow.delete_history(&scope, cutoff).await.unwrap(), 1);
    uow.rollback().await.unwrap();
    assert!(media_postgres::slot_state(&pool, &name, old).await.unwrap().is_some(), "nothing deleted without the audit commit");

    let mut uow = store.begin().await.unwrap();
    uow.insert_retention_run(&run).await.unwrap();
    let deleted = uow.delete_history(&scope, cutoff).await.unwrap();
    let finished = run.clone().succeeded(deleted, None, 5);
    uow.save_retention_run(&finished).await.unwrap();
    uow.commit().await.unwrap();
    assert!(media_postgres::slot_state(&pool, &name, old).await.unwrap().is_none());
    assert!(media_postgres::slot_state(&pool, &name, old - MICROS_PER_DAY).await.unwrap().is_some(), "a pending claim is kept");
    let (status, rows): (String, i64) = sqlx::query_as("SELECT status, rows_deleted FROM transgate_jobs.retention_runs WHERE id = $1")
        .bind(run.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((status.as_str(), rows), (finished.status.as_str(), 1));

    // A crash left a run `running`: the next run of that job name marks it failed.
    let mut stale = transgate_core::privacy::retention::RetentionRun::started(days, NOW, "scheduler");
    stale.job_name = run.job_name.clone();
    stale.rows_deleted = 7;
    let mut uow = store.begin().await.unwrap();
    uow.insert_retention_run(&stale).await.unwrap();
    uow.commit().await.unwrap();
    let mut uow = store.begin().await.unwrap();
    assert_eq!(uow.fail_interrupted_runs(&run.job_name, "interrupted before completion", NOW).await.unwrap(), 1);
    assert_eq!(uow.fail_interrupted_runs(&run.job_name, "interrupted before completion", NOW).await.unwrap(), 0);
    uow.commit().await.unwrap();
    let (status, rows, message): (String, i64, Option<String>) =
        sqlx::query_as("SELECT status, rows_deleted, error_message FROM transgate_jobs.retention_runs WHERE id = $1")
            .bind(stale.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), rows, message.as_deref()), ("failure", 0, Some("interrupted before completion")));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn an_upload_capability_belongs_to_the_signing_caller_even_for_a_foreign_or_missing_job() {
    let store = PgJobStore::new(runtime().await);
    let (ada, ada_job) = account_with_job(&store, JobStatus::Draft).await;
    let (bob, _) = account_with_job(&store, JobStatus::Draft).await;
    let expires_at = Micros(NOW.0 + MICROS_PER_DAY);
    let foreign = IssuedUpload { account: bob, job: ada_job, object: object(&format!("{}/{}/a.mp3", bob.0, ada_job.0)), expires_at };
    let missing = IssuedUpload { account: bob, job: JobId(i64::MAX - 7), object: object(&format!("{}/missing/b.mp3", bob.0)), expires_at };
    let own = IssuedUpload { account: ada, job: ada_job, object: object(&format!("{}/{}/own.mp3", ada.0, ada_job.0)), expires_at };
    let mut uow = store.begin().await.unwrap();
    uow.record_issued_upload(&own).await.unwrap();
    uow.record_issued_upload(&foreign).await.unwrap();
    uow.record_issued_upload(&missing).await.expect("a job that does not exist still records the signed capability");
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let bobs = uow.account_resources(bob).await.unwrap().issued_uploads;
    let adas = uow.account_resources(ada).await.unwrap().issued_uploads;
    uow.rollback().await.unwrap();
    assert_eq!(bobs, vec![foreign, missing], "Bob's erasure re-sweeps both of Bob's capabilities, Ada's job id or not");
    assert_eq!(adas, vec![own], "Ada's erasure sees only the capability signed for Ada, never Bob's folder");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn f6_a_conflicting_delivery_is_held_once_with_its_body_and_is_never_pending_or_applied() {
    let store = PgJobStore::new(runtime().await);
    let owner = owner().await;
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let a = attempt(id, account, 1, AttemptState::Accepted, Some("remote-c"));
    let first = NewInboxEntry {
        attempt: a.id,
        job: id,
        account,
        remote_id: "remote-c".into(),
        origin: ObservationOrigin::Webhook,
        observation: soniox_observation("remote-c"),
        fingerprint: "fp-first".into(),
        received_at: NOW,
    };
    let mut other = soniox_observation("remote-c");
    if let ProviderPayload::Soniox { reported, .. } = &mut other.payload {
        reported.audio_duration_ms = Some(999_000.0);
    }
    let conflicting = NewInboxEntry { observation: other, fingerprint: "fp-other".into(), received_at: Micros(NOW.0 + 1), ..first.clone() };
    let mut uow = store.begin().await.unwrap();
    uow.insert_attempt(&a).await.unwrap();
    let (stored, _) = uow.insert_inbox(&first).await.unwrap();
    let (again, new) = uow.insert_inbox(&conflicting).await.unwrap();
    assert!(!new && again.id == stored.id && again.entry.fingerprint == "fp-first", "the delivery key still names the first entry");
    uow.hold_inbox_conflict(stored.id, &conflicting).await.unwrap();
    uow.hold_inbox_conflict(stored.id, &conflicting).await.unwrap();
    uow.commit().await.unwrap();

    let held: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, state, fingerprint, note FROM transgate_jobs.inbox WHERE conflict_of = $1",
    )
    .bind(stored.id.0)
    .fetch_all(&owner)
    .await
    .unwrap();
    assert_eq!(held.len(), 1, "idempotent per (entry, fingerprint)");
    assert_eq!((held[0].1.as_str(), held[0].2.as_str()), ("conflict", "fp-other"));
    assert_eq!(held[0].3.as_deref(), Some(format!("conflicts with inbox {}", stored.id.0).as_str()));
    let mut uow = store.begin().await.unwrap();
    let kept = uow.lock_inbox(transgate_core::ids::InboxId(held[0].0)).await.unwrap().unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(kept.entry.observation, conflicting.observation, "the conflicting body is kept as received");
    let pending: Vec<_> = store.pending_inbox(1000).await.unwrap().into_iter().filter(|e| e.entry.attempt == a.id).collect();
    assert_eq!(pending.iter().map(|e| e.id).collect::<Vec<_>>(), vec![stored.id], "only the first entry is pending");

    // Erased with the account like every inbox entry.
    let mut uow = store.begin().await.unwrap();
    uow.erase_account_rows(account).await.unwrap();
    uow.commit().await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.inbox WHERE account_id = $1")
        .bind(account.0)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn f2_an_archive_intent_is_the_accounts_and_survives_the_deletion_of_its_job_row() {
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let payload = transgate_core::jobs::ports::ArchivedPayload {
        account,
        job: id,
        object: object(&format!("{}/{}/raw.json", account.0, id.0)),
        write: ArchiveWrite::InFlight,
    };
    let mut uow = store.begin().await.unwrap();
    uow.lock_job(id).await.unwrap().unwrap();
    uow.record_archived_payload(&payload).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    uow.hard_delete_job(id).await.unwrap();
    uow.commit().await.unwrap();
    assert!(store.owned_job(account, id).await.unwrap().is_none());

    let mut uow = store.begin().await.unwrap();
    let archived = uow.account_resources(account).await.unwrap().archived_payloads;
    let foreign = uow.account_resources(AccountId(Uuid::new_v4())).await.unwrap().archived_payloads;
    uow.rollback().await.unwrap();
    assert_eq!(archived, vec![payload], "account, job, object and write state come back after the job row is gone");
    assert!(foreign.is_empty());
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn f2_settling_an_archive_changes_only_the_accounts_in_flight_intent_for_that_object() {
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let (other, other_job) = account_with_job(&store, JobStatus::Running).await;
    let intent = |account, job: JobId, key: &str, write| transgate_core::jobs::ports::ArchivedPayload { account, job, object: object(key), write };
    let flying = intent(account, id, &format!("{}/{}/flying.json", account.0, id.0), ArchiveWrite::InFlight);
    let landed = intent(account, id, &format!("{}/{}/landed.json", account.0, id.0), ArchiveWrite::Written);
    // Another account's in-flight intent naming the same object key.
    let foreign = intent(other, other_job, &format!("{}/{}/flying.json", account.0, id.0), ArchiveWrite::InFlight);
    let mut uow = store.begin().await.unwrap();
    for p in [&flying, &landed, &foreign] {
        uow.record_archived_payload(p).await.unwrap();
    }
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    uow.settle_archived_payload(account, &flying.object, ArchiveWrite::NotWritten).await.unwrap();
    uow.settle_archived_payload(account, &landed.object, ArchiveWrite::NotWritten).await.unwrap();
    uow.settle_archived_payload(account, &object("never-recorded"), ArchiveWrite::Written).await.expect("no row is not an error");
    // A second settlement of an already settled intent changes nothing.
    uow.settle_archived_payload(account, &flying.object, ArchiveWrite::Written).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let mine = uow.account_resources(account).await.unwrap().archived_payloads;
    let theirs = uow.account_resources(other).await.unwrap().archived_payloads;
    uow.rollback().await.unwrap();
    assert_eq!(
        mine.iter().map(|p| p.write).collect::<Vec<_>>(),
        vec![ArchiveWrite::NotWritten, ArchiveWrite::Written],
        "only the in-flight intent settled, once; the written one kept its state"
    );
    assert_eq!(theirs, vec![foreign], "another account's intent for the same key is untouched");
}

struct FixedClock(Micros);

impl billing_core::types::Clock for FixedClock {
    fn now(&self) -> Micros {
        self.0
    }
}

struct NoSleep;

impl transgate_core::jobs::ports::Sleeper for NoSleep {
    fn sleep(&self, _millis: u64) -> transgate_core::jobs::ports::BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

fn jobs_app(store: &PgJobStore, now: Micros) -> transgate_core::jobs::app::Jobs {
    jobs_app_with_transports(store, now, vec![], vec![])
}

fn jobs_app_with_transports(
    store: &PgJobStore,
    now: Micros,
    objects: Vec<std::sync::Arc<dyn media_ports::ObjectStore>>,
    transcribers: Vec<std::sync::Arc<dyn media_ports::Transcriber>>,
) -> transgate_core::jobs::app::Jobs {
    use transgate_core::jobs::app::{Jobs, JobsConfig, JobsServices};
    use transgate_core::jobs::routing::{ModelConfig, RoutingConfig};
    Jobs::new(
        JobsServices {
            store: std::sync::Arc::new(store.clone()),
            clock: std::sync::Arc::new(FixedClock(now)),
            objects,
            transcribers,
            stager: None,
            sleeper: std::sync::Arc::new(NoSleep),
            rate_limiter: std::sync::Arc::new(PgRateLimiter::new(store.pool().clone())),
            paid_captions: None,
            video_metadata: None,
        },
        JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: 200 },
            models: ModelConfig {
                soniox: "stt-async-v5".into(),
                soniox_v5: "stt-async-v5".into(),
                deepgram: "nova-2".into(),
                deepgram_account_overrides: vec![],
            },
            upload_storage: StorageProvider::Gcs,
            upload_bucket: "uploads".into(),
            archive_store: StoreKind::Gcs,
            archive_bucket: "archive".into(),
            staging_store: StoreKind::Gcs,
            staging_bucket: "staging".into(),
            callback_base_url: "https://transgate.test".into(),
            soniox_auto_delete: false,
            ssrf_protection: true,
            freemium_lists: Default::default(),
        },
    )
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn f4_a_user_delete_and_the_stuck_pass_on_one_job_queue_on_the_account_lock_and_never_deadlock() {
    let owner = owner().await;
    let store = PgJobStore::new(runtime().await);
    let (account, id) = account_with_job(&store, JobStatus::Running).await;
    let mut uow = store.begin().await.unwrap();
    let mut job = uow.lock_job(id).await.unwrap().unwrap();
    job.provider = Some(Provider::Deepgram);
    job.started_at = Some(NOW);
    uow.save_job(&job, job.revision).await.unwrap();
    uow.commit().await.unwrap();
    let now = Micros(NOW.0 + 31 * MICROS_PER_MINUTE);
    let app = std::sync::Arc::new(jobs_app(&store, now));

    // A third transaction holds the account, so both paths are in flight at once.
    // A path that took the job row before the account would then hold the job
    // while the other holds the account: PostgreSQL would abort one with 40P01.
    let mut blocker = store.begin().await.unwrap();
    blocker.billing().await.unwrap().lock_account(account).await.unwrap();
    let waiting_before: i64 = waiting_runtime_backends(&owner).await;
    let delete = tokio::spawn({
        let app = app.clone();
        async move { app.delete_job(account, id).await }
    });
    let stuck = tokio::spawn({
        let app = app.clone();
        async move { app.stuck_pass().await }
    });
    let mut both_waiting = false;
    for _ in 0..100 {
        if waiting_runtime_backends(&owner).await >= waiting_before + 2 {
            both_waiting = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    blocker.commit().await.unwrap();
    let deleted = delete.await.unwrap();
    let report = stuck.await.unwrap();
    assert!(both_waiting, "both paths were waiting while the account was held");
    assert!(deleted.is_ok(), "{deleted:?}");
    let report = report.expect("the stuck pass finished without a deadlock abort");
    assert_eq!(report.errors, 0, "{report:?}");
    let stored = store.owned_job(account, id).await.unwrap().unwrap();
    assert_eq!(stored.status, JobStatus::Deleted, "the delete applied, before or after the reset");
}

async fn waiting_runtime_backends(owner: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE usename = $1 AND wait_event_type = 'Lock'")
        .bind(RUNTIME_LOGIN)
        .fetch_one(owner)
        .await
        .unwrap()
}
