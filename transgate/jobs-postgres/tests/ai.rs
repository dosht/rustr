//! P4 AI persistence against a real database through the restricted billing
//! runtime login: `AiTx` on the same transaction as `JobUow`.
//!
//! Run with `TRANSGATE_JOBS_TEST_DATABASE_URL=... cargo test -p transgate-jobs-postgres --test ai -- --include-ignored`.
//! The URL names a disposable database with owner authority; a missing variable
//! fails the test, it never skips. Every test uses random accounts.

use std::time::Duration;

use billing_core::ports::StoreError;
use billing_core::types::{AccountId, Micros};
use media_ports::{ObjectRef, StoreKind};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use transgate_core::ai::GenerationId;
use transgate_core::ai::ports::{
    ChatMessage, ChatRole, CompletionSummary, ConsentRecord, CounterKey, CounterKind, CounterScope, Draft, Execution,
    FinalContent, Generation, GenerationInput, GenerationKind, GenerationStatus, Period, QuotaCounter,
    QuotaReservation, ReservedBucket, SummaryOutcome, SummaryRequest,
};
use transgate_core::ids::{JobId, MICROS_PER_DAY, MICROS_PER_MINUTE};
use transgate_core::jobs::domain::{JobSource, JobStatus, NewJob, StorageProvider};
use transgate_core::jobs::ports::{JobQueries, JobStore, JobUow};
use transgate_jobs_postgres::PgJobStore;
use uuid::Uuid;

#[path = "ai_support/uncertain_commit.rs"]
mod uncertain_commit;

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
            billing_postgres::provision_runtime_login(&owner, RUNTIME_LOGIN, RUNTIME_PASSWORD).await.expect("runtime login");
        })
        .await;
    owner
}

async fn runtime_with(max_connections: u32) -> PgPool {
    owner().await;
    let options = billing_postgres::options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap();
    let pool = billing_postgres::connect_runtime(options, max_connections).await.expect("restricted runtime pool");
    transgate_jobs_postgres::verify_runtime(&pool).await.expect("restricted transgate runtime");
    pool
}

async fn store() -> PgJobStore {
    PgJobStore::new(runtime_with(8).await)
}

/// A provisioned billing account with one completed job, committed.
async fn account_with_job(store: &PgJobStore) -> (AccountId, JobId) {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().create_account(account, NOW).await.unwrap();
    let job = uow.insert_job(&new_job(account)).await.unwrap();
    uow.commit().await.unwrap();
    (account, job.id)
}

fn new_job(account: AccountId) -> NewJob {
    NewJob {
        account,
        status: JobStatus::Completed,
        name: Some("meeting.mp3".into()),
        source: JobSource::Upload {
            storage: StorageProvider::Gcs,
            object: Some(ObjectRef { store: StoreKind::Gcs, bucket: "uploads".into(), key: format!("{}/m.mp3", account.0), version: None }),
        },
        language: None,
        translate_to: vec![],
        speakers: false,
        fix_grammer: None,
        duration_minutes: Some(2),
        size_bytes: Some(10),
        callback_url: None,
        whisper_params: None,
        created_at: NOW,
    }
}

fn generation(account: AccountId, job: JobId, kind: GenerationKind, epoch: i64, metered: bool) -> Generation {
    let input = match kind {
        GenerationKind::Chat => GenerationInput::Chat {
            question: "What was decided?".into(),
            sanitized_question: "What was decided?".into(),
            history: vec![],
            language: None,
        },
        GenerationKind::Analysis => GenerationInput::Analysis,
        GenerationKind::CompletionSummary => GenerationInput::CompletionSummary { detailed: false },
    };
    let quota = metered.then(|| QuotaReservation {
        buckets: vec![ReservedBucket {
            key: CounterKey { account, scope: CounterScope::User, kind: CounterKind::Questions, period: Period::Daily },
            period_end: Some(Micros(NOW.0 + MICROS_PER_DAY)),
            amount: 1,
        }],
        question_tokens: 5,
    });
    Generation {
        id: GenerationId::new(),
        account,
        job,
        kind,
        request_key: Uuid::new_v4(),
        request_digest: "digest-a".into(),
        source_revision: 0,
        consent_epoch: epoch,
        input,
        status: GenerationStatus::Accepted,
        sequence: 0,
        draft: Draft::Empty,
        staged: None,
        execution: None,
        quota,
        interruption: None,
        remote_outcome_unknown: false,
        remote_id: None,
        tokens_used: None,
        created_at: NOW,
        updated_at: NOW,
        finished_at: None,
    }
}

fn message(g: &Generation, role: ChatRole, content: &str) -> ChatMessage {
    ChatMessage {
        id: Uuid::new_v4(),
        account: g.account,
        job: g.job,
        generation: g.id,
        order: 0,
        role,
        content: content.into(),
        consent_epoch: g.consent_epoch,
        created_at: NOW,
    }
}

fn consent(account: AccountId, has_consented: bool, epoch: i64) -> ConsentRecord {
    ConsentRecord {
        account,
        has_consented,
        version: "1.0".into(),
        provider: "deepseek".into(),
        policy_url: "https://transgate.test/privacy".into(),
        consent_date: has_consented.then_some(NOW),
        revoked_date: None,
        epoch,
        purge_cutoff: None,
        purge_pending: false,
        updated_at: NOW,
    }
}

fn user_questions(account: AccountId) -> CounterKey {
    CounterKey { account, scope: CounterScope::User, kind: CounterKind::Questions, period: Period::Daily }
}

async fn count(pool: &PgPool, sql: &'static str, account: AccountId) -> i64 {
    sqlx::query_scalar(sql).bind(account.0).fetch_one(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_runtime_cannot_rewrite_a_generations_fences_or_an_artifact_after_it_is_written() {
    let store = store().await;
    for sql in [
        "UPDATE transgate_jobs.generations SET consent_epoch = 99",
        "UPDATE transgate_jobs.generations SET source_revision = 99",
        "UPDATE transgate_jobs.generations SET input = '{}'::jsonb",
        "UPDATE transgate_jobs.generations SET account_id = gen_random_uuid()",
        "UPDATE transgate_jobs.chat_messages SET content = 'rewritten'",
        "UPDATE transgate_jobs.completion_summaries SET text = 'rewritten'",
        "UPDATE transgate_jobs.summary_requests SET account_id = gen_random_uuid()",
        "UPDATE transgate_jobs.quota_counters SET account_id = gen_random_uuid()",
        "TRUNCATE transgate_jobs.quota_counters",
    ] {
        let error = sqlx::query(sql).execute(store.pool()).await.unwrap_err();
        let code = error.as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default();
        assert_eq!(code, "42501", "{sql}: {error}");
    }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn two_processes_admitting_metered_chat_for_one_account_admit_exactly_one() {
    let owner = owner().await;
    let first = store().await;
    // One connection, so the backend that reports its pid is the one that waits.
    let second = PgJobStore::new(runtime_with(1).await);
    let (account, job) = account_with_job(&first).await;

    let mut holder = first.begin().await.unwrap();
    holder.billing().await.unwrap().lock_account(account).await.unwrap();
    assert!(holder.ai().active_metered(account).await.unwrap().is_none());
    let admitted = generation(account, job, GenerationKind::Chat, 1, true);
    holder.ai().insert_generation(&admitted).await.unwrap();

    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(second.pool()).await.unwrap();
    let competitor = tokio::spawn(async move {
        let mut uow = second.begin().await.unwrap();
        uow.billing().await.unwrap().lock_account(account).await.unwrap();
        let busy = uow.ai().active_metered(account).await.unwrap();
        if busy.is_none() {
            uow.ai().insert_generation(&generation(account, job, GenerationKind::Chat, 1, true)).await.unwrap();
        }
        uow.commit().await.unwrap();
        busy.map(|g| g.id)
    });
    let mut waited = false;
    for _ in 0..100 {
        waited = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')")
            .bind(pid)
            .fetch_one(&owner)
            .await
            .unwrap();
        if waited {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(waited, "the second admission waits on the account lock");
    holder.commit().await.unwrap();
    assert_eq!(competitor.await.unwrap(), Some(admitted.id), "the second admission sees the first as busy");
    assert_eq!(
        count(&owner, "SELECT count(*) FROM transgate_jobs.generations WHERE account_id = $1 AND quota IS NOT NULL", account).await,
        1
    );

    // The database also refuses a second active metered generation outright.
    let mut uow = first.begin().await.unwrap();
    let doubled = uow.ai().insert_generation(&generation(account, job, GenerationKind::Chat, 1, true)).await;
    assert!(matches!(doubled, Err(StoreError::Conflict(_))), "{doubled:?}");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_request_key_is_unique_per_account_and_returns_the_original_generation() {
    let store = store().await;
    let (account, job) = account_with_job(&store).await;
    let (other, other_job) = account_with_job(&store).await;
    let original = generation(account, job, GenerationKind::Analysis, 1, false);
    let mut uow = store.begin().await.unwrap();
    uow.ai().insert_generation(&original).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let replay = Generation { id: GenerationId::new(), request_digest: "digest-b".into(), ..original.clone() };
    assert!(matches!(uow.ai().insert_generation(&replay).await, Err(StoreError::Conflict(_))));
    uow.rollback().await.unwrap();
    let mut uow = store.begin().await.unwrap();
    let found = uow.ai().generation_by_request(account, original.request_key).await.unwrap();
    assert_eq!(found, Some(original.clone()), "the caller compares this digest with its own");
    let elsewhere = Generation { id: GenerationId::new(), account: other, job: other_job, ..original.clone() };
    uow.ai().insert_generation(&elsewhere).await.expect("another account may use the same key");
    uow.commit().await.unwrap();
}

// Adapter scope only: what reconciliation reads after a committed completion, and that the
// unique keys refuse a second pair. `Ai` handling a lost COMMIT answer is proved by
// `ai_completes_once_when_the_completing_commit_answer_is_lost` below.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_committed_pair_and_settlement_are_readable_for_reconciliation_and_never_written_twice() {
    let store = store().await;
    let owner = owner().await;
    let (account, job) = account_with_job(&store).await;
    let mut g = generation(account, job, GenerationKind::Chat, 1, true);
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    uow.ai().save_counter(&QuotaCounter { key: user_questions(account), count: 1, max_limit: Some(10), reset_at: Some(Micros(NOW.0 + MICROS_PER_DAY)) }).await.unwrap();
    uow.ai().insert_generation(&g).await.unwrap();
    uow.commit().await.unwrap();

    // A worker stages the sanitized final under its execution fence.
    let token = Uuid::new_v4();
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let mut locked = uow.ai().lock_generation(g.id).await.unwrap().unwrap();
    locked.status = GenerationStatus::Running;
    locked.execution = Some(Execution { token, lease_until: Micros(NOW.0 + MICROS_PER_MINUTE) });
    locked.sequence = 3;
    locked.draft = Draft::Text { text: "Hello".into() };
    locked.staged = Some(FinalContent::Text { text: "Hello world".into() });
    uow.ai().save_generation(&locked).await.unwrap();
    uow.commit().await.unwrap();
    g = locked;

    // The completing transaction: pair, settlement and completion together.
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let pair = uow.ai().append_chat_pair(&message(&g, ChatRole::User, "What was decided?"), &message(&g, ChatRole::Assistant, "Hello world")).await.unwrap();
    assert_eq!((pair[0].order, pair[1].order), (1, 2), "source get_next_message_order starts at 1");
    let mut counter = uow.ai().lock_counter(user_questions(account)).await.unwrap().unwrap();
    counter.count += 3;
    uow.ai().save_counter(&counter).await.unwrap();
    let mut done = g.clone();
    done.status = GenerationStatus::Completed;
    done.quota = None;
    done.execution = None;
    done.finished_at = Some(NOW);
    uow.ai().save_generation(&done).await.unwrap();
    uow.commit().await.unwrap();

    // A fresh transaction reads back the committed pair, generation and settlement.
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let found = uow.ai().chat_pair(g.id).await.unwrap().expect("the pair committed");
    assert_eq!(found, pair);
    let reread = uow.ai().lock_generation(g.id).await.unwrap().unwrap();
    assert_eq!((reread.status, reread.quota.is_none()), (GenerationStatus::Completed, true));
    let again = uow.ai().append_chat_pair(&message(&g, ChatRole::User, "What was decided?"), &message(&g, ChatRole::Assistant, "Hello world")).await;
    assert!(matches!(again, Err(StoreError::Conflict(_))), "a generation never gets a second pair: {again:?}");
    uow.rollback().await.unwrap();
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_jobs.chat_messages WHERE account_id = $1", account).await, 2);
    let mut uow = store.begin().await.unwrap();
    assert_eq!(uow.ai().lock_counter(user_questions(account)).await.unwrap().unwrap().count, 4, "settled once");
    // A later pair of another generation continues the job's order.
    let next = generation(account, job, GenerationKind::Chat, 1, false);
    uow.ai().insert_generation(&next).await.unwrap();
    let later = uow.ai().append_chat_pair(&message(&next, ChatRole::User, "And then?"), &message(&next, ChatRole::Assistant, "Nothing.")).await.unwrap();
    assert_eq!((later[0].order, later[1].order), (3, 4));
    assert_eq!(uow.ai().chat_history(account, job).await.unwrap().len(), 4);
    uow.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_staged_final_survives_a_rolled_back_completion_for_local_reconciliation() {
    let store = store().await;
    let (account, job) = account_with_job(&store).await;
    let mut g = generation(account, job, GenerationKind::Chat, 1, true);
    g.staged = Some(FinalContent::Text { text: "final".into() });
    let mut uow = store.begin().await.unwrap();
    uow.ai().insert_generation(&g).await.unwrap();
    uow.commit().await.unwrap();
    let mut uow = store.begin().await.unwrap();
    uow.ai().append_chat_pair(&message(&g, ChatRole::User, "q"), &message(&g, ChatRole::Assistant, "final")).await.unwrap();
    uow.rollback().await.unwrap();
    let mut uow = store.begin().await.unwrap();
    assert!(uow.ai().chat_pair(g.id).await.unwrap().is_none());
    let reread = uow.ai().generation(g.id).await.unwrap().unwrap();
    assert_eq!(reread.staged, g.staged, "settlement can finish from the staged final, no provider call");
    assert!(reread.quota.is_some(), "the reservation is neither released nor spent");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn user_counters_are_one_row_per_key_and_transcript_counters_are_per_job() {
    let store = store().await;
    let (account, job) = account_with_job(&store).await;
    let mut uow = store.begin().await.unwrap();
    let user = user_questions(account);
    let transcript = CounterKey { account, scope: CounterScope::Transcript(job), kind: CounterKind::Questions, period: Period::Transcript };
    let period_end = Some(Micros(NOW.0 + MICROS_PER_DAY));
    uow.ai().save_counter(&QuotaCounter { key: user, count: 1, max_limit: Some(10), reset_at: period_end }).await.unwrap();
    // A second upsert of the same user key (NULL job) replaces, never duplicates.
    uow.ai().save_counter(&QuotaCounter { key: user, count: 2, max_limit: Some(10), reset_at: period_end }).await.unwrap();
    uow.ai().save_counter(&QuotaCounter { key: transcript, count: 7, max_limit: None, reset_at: None }).await.unwrap();
    uow.commit().await.unwrap();
    let mut uow = store.begin().await.unwrap();
    let all = uow.ai().counters(account, Some(job)).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(uow.ai().lock_counter(user).await.unwrap().unwrap().count, 2);
    assert_eq!(uow.ai().lock_counter(transcript).await.unwrap().unwrap().count, 7);
    assert_eq!(uow.ai().counters(account, None).await.unwrap().len(), 1, "without a job only user counters");
    uow.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn a_delete_purge_removes_every_artifact_at_or_below_the_cutoff_including_legacy_and_keeps_newer_ones() {
    let store = store().await;
    let owner = owner().await;
    let (account, job) = account_with_job(&store).await;
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    uow.ai().save_consent(&consent(account, true, 3)).await.unwrap();
    uow.ai().save_counter(&QuotaCounter { key: user_questions(account), count: 2, max_limit: None, reset_at: None }).await.unwrap();
    let mut ids = Vec::new();
    for epoch in [1, 2, 3] {
        let g = generation(account, job, GenerationKind::Chat, epoch, false);
        uow.ai().insert_generation(&g).await.unwrap();
        uow.ai().append_chat_pair(&message(&g, ChatRole::User, "q"), &message(&g, ChatRole::Assistant, "a")).await.unwrap();
        uow.ai().save_completion_summary(&CompletionSummary { account, job, source_revision: epoch, generation: g.id, text: format!("summary {epoch}"), consent_epoch: epoch, created_at: NOW }).await.unwrap();
        ids.push(g.id);
    }
    uow.ai().request_summary(&SummaryRequest { account, job, source_revision: 1, requested_at: NOW, generation: None, outcome: None, decided_at: None }).await.unwrap();
    uow.commit().await.unwrap();
    // A legacy import carries no epoch.
    sqlx::query("UPDATE transgate_jobs.generations SET consent_epoch = NULL WHERE id = $1").bind(ids[0].0).execute(&owner).await.unwrap();
    sqlx::query("UPDATE transgate_jobs.chat_messages SET consent_epoch = NULL WHERE generation_id = $1").bind(ids[0].0).execute(&owner).await.unwrap();

    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let mut revoked = consent(account, false, 4);
    revoked.purge_cutoff = Some(2);
    revoked.purge_pending = true;
    uow.ai().save_consent(&revoked).await.unwrap();
    uow.commit().await.unwrap();
    assert!(store.pending_ai_purges(1000).await.unwrap().contains(&account));

    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    let legacy = uow.ai().generation(ids[0]).await.unwrap().unwrap();
    assert_eq!(legacy.consent_epoch, 0, "a legacy row reads as epoch 0");
    assert_eq!(uow.ai().purge_ai_artifacts(account, 2).await.unwrap(), 2 * 2 + 2 + 2, "two pairs, two summaries, two generations");
    revoked.purge_pending = false;
    uow.ai().save_consent(&revoked).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    assert!(uow.ai().generation(ids[0]).await.unwrap().is_none() && uow.ai().generation(ids[1]).await.unwrap().is_none());
    assert!(uow.ai().generation(ids[2]).await.unwrap().is_some(), "content above the cutoff survives");
    assert_eq!(uow.ai().chat_history(account, job).await.unwrap().iter().map(|m| m.generation).collect::<Vec<_>>(), vec![ids[2], ids[2]]);
    assert_eq!(uow.ai().completion_summary(account, job).await.unwrap().unwrap().source_revision, 3);
    assert_eq!(uow.ai().counters(account, None).await.unwrap()[0].count, 2, "content-free quota survives");
    assert!(uow.ai().summary_request(job).await.unwrap().is_some(), "content-free summary requests survive");
    uow.rollback().await.unwrap();
    assert!(!store.pending_ai_purges(1000).await.unwrap().contains(&account));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_source_revision_advances_per_job_and_a_hard_delete_takes_every_ai_child_and_transcript_quota_but_not_account_quota() {
    let store = store().await;
    let owner = owner().await;
    let (account, job) = account_with_job(&store).await;
    let mut uow = store.begin().await.unwrap();
    assert_eq!(uow.ai().source_revision(job).await.unwrap(), 0);
    assert_eq!(uow.ai().bump_source_revision(job).await.unwrap(), 1);
    assert_eq!(uow.ai().bump_source_revision(job).await.unwrap(), 2);
    assert_eq!(uow.ai().source_revision(job).await.unwrap(), 2);
    let g = generation(account, job, GenerationKind::Chat, 1, false);
    uow.ai().insert_generation(&g).await.unwrap();
    uow.ai().append_chat_pair(&message(&g, ChatRole::User, "q"), &message(&g, ChatRole::Assistant, "a")).await.unwrap();
    uow.ai().save_completion_summary(&CompletionSummary { account, job, source_revision: 2, generation: g.id, text: "s".into(), consent_epoch: 1, created_at: NOW }).await.unwrap();
    uow.ai().request_summary(&SummaryRequest { account, job, source_revision: 2, requested_at: NOW, generation: Some(g.id), outcome: None, decided_at: None }).await.unwrap();
    let transcript = CounterKey { account, scope: CounterScope::Transcript(job), kind: CounterKind::Tokens, period: Period::Transcript };
    uow.ai().save_counter(&QuotaCounter { key: transcript, count: 40, max_limit: None, reset_at: None }).await.unwrap();
    let account_wide = QuotaCounter { key: user_questions(account), count: 3, max_limit: Some(10), reset_at: Some(Micros(NOW.0 + MICROS_PER_DAY)) };
    uow.ai().save_counter(&account_wide).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let missing = uow.ai().bump_source_revision(JobId(i64::MAX - 11)).await;
    assert!(matches!(missing, Err(StoreError::Conflict(_))), "a revision never exists without its job: {missing:?}");
    uow.rollback().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    uow.hard_delete_job(job).await.unwrap();
    uow.commit().await.unwrap();
    for (sql, expected) in [
        ("SELECT count(*) FROM transgate_jobs.generations WHERE account_id = $1", 0i64),
        ("SELECT count(*) FROM transgate_jobs.chat_messages WHERE account_id = $1", 0),
        ("SELECT count(*) FROM transgate_jobs.completion_summaries WHERE account_id = $1", 0),
        ("SELECT count(*) FROM transgate_jobs.summary_requests WHERE account_id = $1", 0),
        ("SELECT count(*) FROM transgate_jobs.quota_counters WHERE account_id = $1 AND scope = 'transcript'", 0),
    ] {
        assert_eq!(count(&owner, sql, account).await, expected, "{sql}");
    }
    let revisions: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.source_revisions WHERE job_id = $1")
        .bind(job.0)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(revisions, 0);
    let mut uow = store.begin().await.unwrap();
    assert_eq!(uow.ai().counters(account, None).await.unwrap(), vec![account_wide], "account-wide quota survives the hard delete");
    assert!(uow.ai().lock_counter(transcript).await.unwrap().is_none(), "the transcript counter went with its job, as source chat_limits did");
    uow.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn account_erasure_removes_every_ai_row_including_quota_and_consent() {
    let store = store().await;
    let owner = owner().await;
    let (account, job) = account_with_job(&store).await;
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    uow.ai().save_consent(&consent(account, true, 1)).await.unwrap();
    uow.ai().bump_source_revision(job).await.unwrap();
    let g = generation(account, job, GenerationKind::Chat, 1, true);
    uow.ai().insert_generation(&g).await.unwrap();
    uow.ai().append_chat_pair(&message(&g, ChatRole::User, "q"), &message(&g, ChatRole::Assistant, "a")).await.unwrap();
    uow.ai().save_completion_summary(&CompletionSummary { account, job, source_revision: 1, generation: g.id, text: "s".into(), consent_epoch: 1, created_at: NOW }).await.unwrap();
    uow.ai().request_summary(&SummaryRequest { account, job, source_revision: 1, requested_at: NOW, generation: None, outcome: None, decided_at: None }).await.unwrap();
    uow.ai().save_counter(&QuotaCounter { key: user_questions(account), count: 1, max_limit: None, reset_at: None }).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    uow.erase_account_rows(account).await.unwrap();
    uow.commit().await.unwrap();
    for table in ["ai_consent", "generations", "chat_messages", "completion_summaries", "summary_requests", "quota_counters"] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM transgate_jobs.{table} WHERE account_id = $1")))
            .bind(account.0)
            .fetch_one(&owner)
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_summary_producer_queue_is_idempotent_and_a_decided_request_leaves_it() {
    let store = store().await;
    let (account, job) = account_with_job(&store).await;
    let request = SummaryRequest { account, job, source_revision: 1, requested_at: NOW, generation: None, outcome: None, decided_at: None };
    let g = generation(account, job, GenerationKind::CompletionSummary, 1, false);
    let mut uow = store.begin().await.unwrap();
    uow.ai().request_summary(&request).await.unwrap();
    uow.ai().request_summary(&SummaryRequest { requested_at: Micros(NOW.0 + 1), ..request.clone() }).await.unwrap();
    uow.ai().insert_generation(&g).await.unwrap();
    let first = CompletionSummary { account, job, source_revision: 1, generation: g.id, text: "first".into(), consent_epoch: 1, created_at: NOW };
    uow.ai().save_completion_summary(&first).await.unwrap();
    uow.ai().save_completion_summary(&CompletionSummary { text: "second".into(), ..first.clone() }).await.unwrap();
    uow.commit().await.unwrap();
    assert!(store.pending_summary_requests(1000).await.unwrap().contains(&(job, 1)));

    let mut uow = store.begin().await.unwrap();
    let mut locked = uow.ai().lock_summary_request(job, 1).await.unwrap().unwrap();
    assert_eq!(locked.requested_at, NOW, "an existing request is left as is");
    locked.generation = Some(g.id);
    locked.outcome = Some(SummaryOutcome::Saved { generation: g.id });
    locked.decided_at = Some(NOW);
    uow.ai().save_summary_request(&locked).await.unwrap();
    let missing = uow.ai().save_summary_request(&SummaryRequest { source_revision: 9, ..locked.clone() }).await;
    assert!(matches!(missing, Err(StoreError::Conflict(_))), "{missing:?}");
    uow.rollback().await.unwrap();
    let mut uow = store.begin().await.unwrap();
    uow.ai().save_summary_request(&locked).await.unwrap();
    assert_eq!(uow.ai().completion_summary(account, job).await.unwrap().unwrap().text, "first", "an existing summary is kept");
    assert_eq!(uow.ai().summary_request(job).await.unwrap(), Some(locked));
    uow.commit().await.unwrap();
    assert!(!store.pending_summary_requests(1000).await.unwrap().contains(&(job, 1)));
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn runnable_generations_are_accepted_or_running_with_an_expired_lease() {
    let store = store().await;
    let (account, job) = account_with_job(&store).await;
    let accepted = generation(account, job, GenerationKind::Analysis, 1, false);
    let live = Generation {
        status: GenerationStatus::Running,
        execution: Some(Execution { token: Uuid::new_v4(), lease_until: Micros(NOW.0 + MICROS_PER_MINUTE) }),
        ..generation(account, job, GenerationKind::Chat, 1, false)
    };
    let expired = Generation {
        status: GenerationStatus::Running,
        execution: Some(Execution { token: Uuid::new_v4(), lease_until: Micros(NOW.0 - 1) }),
        ..generation(account, job, GenerationKind::CompletionSummary, 1, false)
    };
    let finished = Generation { status: GenerationStatus::Completed, finished_at: Some(NOW), ..generation(account, job, GenerationKind::Chat, 1, false) };
    let mut uow = store.begin().await.unwrap();
    for g in [&accepted, &live, &expired, &finished] {
        uow.ai().insert_generation(g).await.unwrap();
    }
    uow.commit().await.unwrap();
    let runnable = store.runnable_generations(NOW, 10_000).await.unwrap();
    assert!(runnable.contains(&accepted.id) && runnable.contains(&expired.id));
    assert!(!runnable.contains(&live.id) && !runnable.contains(&finished.id));
    let mut uow = store.begin().await.unwrap();
    assert_eq!(uow.ai().active_for_job(account, job, GenerationKind::Chat).await.unwrap().map(|g| g.id), Some(live.id));
    assert_eq!(uow.ai().latest_analysis(account, job).await.unwrap().map(|g| g.id), Some(accepted.id));
    let gone = uow.ai().save_generation(&Generation { id: GenerationId::new(), ..accepted.clone() }).await;
    assert!(matches!(gone, Err(StoreError::Conflict(_))), "save never inserts: {gone:?}");
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn retention_advances_every_listed_jobs_revision_once_in_one_statement() {
    let store = store().await;
    let (account, first) = account_with_job(&store).await;
    let mut uow = store.begin().await.unwrap();
    let second = uow.insert_job(&new_job(account)).await.unwrap().id;
    let untouched = uow.insert_job(&new_job(account)).await.unwrap().id;
    uow.ai().bump_source_revision(second).await.unwrap();
    uow.ai().bump_source_revisions(&[first, second, second]).await.unwrap();
    assert_eq!(uow.ai().source_revision(first).await.unwrap(), 1, "a job never bumped starts at 1");
    assert_eq!(uow.ai().source_revision(second).await.unwrap(), 2, "a repeated id advances once");
    assert_eq!(uow.ai().source_revision(untouched).await.unwrap(), 0);
    uow.ai().bump_source_revisions(&[]).await.expect("an empty retention set changes nothing");
    let missing = uow.ai().bump_source_revisions(&[first, JobId(i64::MAX - 13)]).await;
    assert!(matches!(missing, Err(StoreError::Conflict(_))), "{missing:?}");
    uow.rollback().await.unwrap();

    // The completion metadata estimate round-trips, absent or present.
    let mut g = generation(account, first, GenerationKind::Analysis, 1, false);
    g.tokens_used = Some(42);
    let mut uow = store.begin().await.unwrap();
    uow.ai().insert_generation(&g).await.unwrap();
    assert_eq!(uow.ai().generation(g.id).await.unwrap().unwrap().tokens_used, Some(42));
    g.tokens_used = None;
    uow.ai().save_generation(&g).await.unwrap();
    assert_eq!(uow.ai().generation(g.id).await.unwrap().unwrap().tokens_used, None);
    uow.rollback().await.unwrap();
}

/// A clock both apps share, so "two processes" agree on the instant.
struct SharedClock(std::sync::atomic::AtomicI64);

impl billing_core::types::Clock for SharedClock {
    fn now(&self) -> Micros {
        Micros(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

struct NoSleep;

impl transgate_core::jobs::ports::Sleeper for NoSleep {
    fn sleep(&self, _millis: u64) -> transgate_core::jobs::ports::BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Answers every opened completion with one delta and a successful stop. A
/// deliberate test provider: no network.
struct AnsweringLlm(&'static str);

struct AnswerStream(std::collections::VecDeque<llm_core::CompletionEvent>);

impl llm_core::CompletionStream for AnswerStream {
    fn next(&mut self) -> llm_core::BoxFuture<'_, Result<Option<llm_core::CompletionEvent>, llm_core::LlmError>> {
        let event = self.0.pop_front();
        Box::pin(async move { Ok(event) })
    }
    fn cancel(&mut self) {
        self.0.clear();
    }
}

impl llm_core::Llm for AnsweringLlm {
    fn open(&self, _request: llm_core::CompletionRequest) -> llm_core::BoxFuture<'_, Result<Box<dyn llm_core::CompletionStream>, llm_core::LlmError>> {
        let events = vec![
            llm_core::CompletionEvent::Delta(self.0.into()),
            llm_core::CompletionEvent::Finished(llm_core::CompletionFinish {
                reason: llm_core::FinishReason::Stop,
                usage: None,
                remote_id: Some("cmpl-midnight".into()),
            }),
        ];
        Box::pin(async move { Ok(Box::new(AnswerStream(events.into())) as Box<dyn llm_core::CompletionStream>) })
    }
}

/// Counts every opened completion: a provider call the test can assert on.
struct CountingLlm {
    inner: AnsweringLlm,
    opened: std::sync::atomic::AtomicUsize,
}

impl llm_core::Llm for CountingLlm {
    fn open(&self, request: llm_core::CompletionRequest) -> llm_core::BoxFuture<'_, Result<Box<dyn llm_core::CompletionStream>, llm_core::LlmError>> {
        self.opened.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.open(request)
    }
}

fn ai_app(store: &PgJobStore, clock: std::sync::Arc<SharedClock>, answer: &'static str) -> transgate_core::ai::Ai {
    ai_app_on(std::sync::Arc::new(store.clone()), clock, std::sync::Arc::new(AnsweringLlm(answer)))
}

fn ai_app_on(
    store: std::sync::Arc<dyn JobStore>,
    clock: std::sync::Arc<SharedClock>,
    llm: std::sync::Arc<dyn llm_core::Llm>,
) -> transgate_core::ai::Ai {
    use std::sync::Arc;
    use transgate_core::ai::{Ai, AiConfig, AiServices, Bounds, ChatLimits, ModelSettings};
    Ai::new(
        AiServices {
            store,
            clock,
            llm: Some(llm),
            live: Arc::new(transgate_core::ai::live::LiveHub::new(64 * 1024, 1024 * 1024)),
            sleeper: Arc::new(NoSleep),
        },
        AiConfig {
            features_enabled: true,
            chat_enabled: true,
            chat_validation: true,
            limits: ChatLimits {
                questions_per_transcript: Some(50),
                questions_per_user: Some(500),
                tokens_per_transcript: Some(20_000),
                tokens_per_user: Some(50_000),
                reset_period: Period::Daily,
            },
            model: ModelSettings::source(),
            bounds: Bounds::default(),
        },
    )
}

async fn user_tokens(owner: &PgPool, account: AccountId) -> (i64, Option<i64>) {
    sqlx::query_as(
        "SELECT count, transgate_jobs.ts_to_us(reset_at) FROM transgate_jobs.quota_counters
         WHERE account_id = $1 AND scope = 'user' AND kind = 'tokens'",
    )
    .bind(account.0)
    .fetch_one(owner)
    .await
    .unwrap()
}

// F2 on real rows, two app instances on separate pools: an answer admitted a minute
// before UTC midnight settles into its admission day; neither a limits read nor a
// competing admission from the other process rolls that bucket while it is active.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn an_answer_settled_after_midnight_lands_in_its_admission_day_across_two_processes() {
    use transgate_core::ai::quota::{estimate_tokens, next_reset};
    let owner = owner().await;
    let (first, second) = (store().await, store().await);
    let (account, job) = account_with_job(&first).await;
    let mut uow = first.begin().await.unwrap();
    let completed = uow.lock_job(job).await.unwrap().unwrap();
    uow.insert_lines(&completed, &[transgate_core::transcript::NewLine {
        start_time: "00:00:00".into(),
        end_time: "00:00:05".into(),
        line: "We agreed to ship the quarterly report on Friday and review the budget next week.".into(),
        speaker: Some("Ada".into()),
        corrected_text: None,
        split_on_speaker_change: None,
        language: None,
        translation_status: None,
    }]).await.unwrap();
    uow.commit().await.unwrap();

    let midnight = next_reset(Period::Daily, NOW).unwrap();
    let clock = std::sync::Arc::new(SharedClock(std::sync::atomic::AtomicI64::new(midnight.0 - MICROS_PER_MINUTE)));
    let process_a = ai_app(&first, clock.clone(), "Friday.");
    let process_b = ai_app(&second, clock.clone(), "Friday.");
    process_a.record_consent(account, true).await.unwrap();
    let ask = |message: &str| transgate_core::ai::ChatRequest { message: message.into(), history: vec![] };

    let started = process_a.start_chat(account, job, Uuid::new_v4(), ask("When?")).await.unwrap();
    let reserved = user_tokens(&owner, account).await;
    assert_eq!(reserved, (estimate_tokens("When?"), Some(midnight.0)));

    clock.0.store(midnight.0 + MICROS_PER_MINUTE, std::sync::atomic::Ordering::SeqCst);
    let limits = process_b.limits(account, job).await.unwrap();
    assert_eq!(limits.tokens_per_user.map(|u| u.current), Some(0), "the new day reads as zero");
    assert_eq!(user_tokens(&owner, account).await, reserved, "a limits read never writes the row");
    let competing = process_b.start_chat(account, job, Uuid::new_v4(), ask("Who?")).await;
    assert!(matches!(competing, Err(transgate_core::ai::AiError::Busy(_))), "{competing:?}");
    assert_eq!(user_tokens(&owner, account).await, reserved, "no rollover while the reservation is active");

    let done = process_a.run(started.generation.id).await.unwrap();
    assert_eq!(done.status, GenerationStatus::Completed);
    assert_eq!(
        user_tokens(&owner, account).await,
        (estimate_tokens("When?Friday."), Some(midnight.0)),
        "settled once, into the admission day"
    );

    process_b.start_chat(account, job, Uuid::new_v4(), ask("Who?")).await.unwrap();
    assert_eq!(
        user_tokens(&owner, account).await,
        (estimate_tokens("Who?"), next_reset(Period::Daily, Micros(midnight.0 + 1)).map(|t| t.0)),
        "the next admission starts the new day from its own reservation"
    );
}

fn dispatch(account: AccountId, generation: GenerationId) -> transgate_core::ai::ports::LlmDispatch {
    transgate_core::ai::ports::LlmDispatch {
        account,
        generation,
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        disposition: transgate_core::ai::ports::DispatchDisposition::Possible,
        remote_ids: vec![],
        unkept_id_observed: false,
        created_at: NOW,
    }
}

// D3/D9: the payload-free dispatch ledger survives a consent DELETE purge and a hard job
// deletion, is captured by the erasure manifest, and goes only with account erasure.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_dispatch_ledger_survives_purge_and_hard_delete_and_goes_only_with_erasure() {
    let store = store().await;
    let owner = owner().await;
    let (account, job) = account_with_job(&store).await;
    let g = generation(account, job, GenerationKind::Chat, 1, false);
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    uow.ai().insert_generation(&g).await.unwrap();
    uow.ai().record_dispatch(&dispatch(account, g.id)).await.unwrap();
    assert!(uow.ai().settle_dispatch(account, g.id, transgate_core::ai::ports::DispatchDisposition::Possible, Some("req-1")).await.unwrap());
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    uow.ai().purge_ai_artifacts(account, 5).await.unwrap();
    assert!(uow.ai().generation(g.id).await.unwrap().is_none(), "the generation's content is purged");
    uow.hard_delete_job(job).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let kept = uow.account_resources(account).await.unwrap().llm_dispatches;
    assert_eq!(kept, vec![transgate_core::ai::ports::LlmDispatch { remote_ids: vec!["req-1".into()], ..dispatch(account, g.id) }],
        "purge and hard delete kept the identifiers the erasure must still answer for");
    uow.erase_account_rows(account).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_jobs.llm_dispatches WHERE account_id = $1", account).await, 0);
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn dispatch_evidence_is_appended_distinct_and_bounded_and_never_cleared() {
    use transgate_core::ai::ports::{DispatchDisposition::*, MAX_DISPATCH_REMOTE_ID_BYTES, MAX_DISPATCH_REMOTE_IDS};
    let store = store().await;
    let (account, _job) = account_with_job(&store).await;
    let (first, second) = (GenerationId::new(), GenerationId::new());
    let mut uow = store.begin().await.unwrap();
    uow.ai().record_dispatch(&dispatch(account, first)).await.unwrap();
    uow.ai().record_dispatch(&dispatch(account, first)).await.expect("an identical re-record is idempotent");
    let other_model = transgate_core::ai::ports::LlmDispatch { model: "gpt-4o".into(), ..dispatch(account, first) };
    assert!(matches!(uow.ai().record_dispatch(&other_model).await, Err(StoreError::Conflict(_))));

    for observed in ["req-1", "req-1", "cmpl-1"] {
        assert!(uow.ai().settle_dispatch(account, first, Possible, Some(observed)).await.unwrap());
    }
    assert!(uow.ai().settle_dispatch(account, first, NotDispatched, None).await.unwrap());
    let too_long = "x".repeat(MAX_DISPATCH_REMOTE_ID_BYTES + 1);
    assert!(uow.ai().settle_dispatch(account, first, Possible, Some(&too_long)).await.unwrap());
    assert!(uow.ai().settle_dispatch(account, first, Possible, Some("")).await.unwrap());
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == first).unwrap();
    assert_eq!(row.remote_ids, vec!["req-1".to_string(), "cmpl-1".to_string()], "distinct, first seen first, oversize and empty ignored");
    assert_eq!(row.disposition, Possible, "an observed id is never cleared by a later not-dispatched report");
    for n in 0..MAX_DISPATCH_REMOTE_IDS + 3 {
        uow.ai().settle_dispatch(account, first, Possible, Some(&format!("id-{n}"))).await.unwrap();
    }
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == first).unwrap();
    assert_eq!(row.remote_ids.len(), MAX_DISPATCH_REMOTE_IDS, "the first N are kept");
    assert!(row.unkept_id_observed, "ids that could not be kept are remembered as evidence");
    assert_eq!(&row.remote_ids[..2], &["req-1".to_string(), "cmpl-1".to_string()]);

    uow.ai().record_dispatch(&dispatch(account, second)).await.unwrap();
    assert!(uow.ai().settle_dispatch(account, second, NotDispatched, None).await.unwrap());
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == second).unwrap();
    assert_eq!((row.disposition, row.remote_ids.len()), (NotDispatched, 0), "not dispatched while no id was observed");
    assert!(uow.ai().settle_dispatch(account, second, NotDispatched, Some(&too_long)).await.unwrap());
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == second).unwrap();
    assert_eq!((row.disposition, row.remote_ids.len(), row.unkept_id_observed), (Possible, 0, true), "an id too long to keep is evidence, not a truncated id");
    // The evidence is durable: a later report without an id cannot erase it.
    assert!(uow.ai().settle_dispatch(account, second, NotDispatched, None).await.unwrap());
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == second).unwrap();
    assert_eq!((row.disposition, row.remote_ids.len(), row.unkept_id_observed), (Possible, 0, true), "unkept evidence is never forgotten");
    assert!(uow.ai().settle_dispatch(account, second, Possible, Some("late")).await.unwrap());
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == second).unwrap();
    assert_eq!((row.disposition, row.remote_ids), (Possible, vec!["late".to_string()]), "a late id turns it possible");

    let third = GenerationId::new();
    uow.ai().record_dispatch(&dispatch(account, third)).await.unwrap();
    for _ in 0..2 {
        uow.ai().settle_dispatch(account, third, Possible, Some("same")).await.unwrap();
    }
    let row = dispatches(&mut uow, account).await.into_iter().find(|d| d.generation == third).unwrap();
    assert_eq!((row.remote_ids, row.unkept_id_observed), (vec!["same".to_string()], false), "re-observing a stored id is not unkept evidence");

    assert!(!uow.ai().settle_dispatch(AccountId(Uuid::new_v4()), first, Possible, Some("x")).await.unwrap(), "another account matches nothing");
    assert!(!uow.ai().settle_dispatch(account, GenerationId::new(), Possible, Some("x")).await.unwrap(), "a missing row answers false");
    uow.rollback().await.unwrap();
}

async fn dispatches(uow: &mut Box<dyn JobUow>, account: AccountId) -> Vec<transgate_core::ai::ports::LlmDispatch> {
    uow.account_resources(account).await.unwrap().llm_dispatches
}

#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn only_an_llm_dispatch_erasure_item_can_have_its_target_rewritten() {
    use transgate_core::privacy::erasure::{ErasureItem, ErasureRecord, ErasureState, ItemState, ItemTarget};
    let store = store().await;
    let account = AccountId(Uuid::new_v4());
    let record = ErasureRecord {
        id: transgate_core::ids::ErasureId::new(),
        account,
        requested_at: NOW,
        state: ErasureState::LocalErased,
        local_erased_at: Some(NOW),
        identity_deleted_at: None,
        closed_at: None,
    };
    let generation = Uuid::new_v4();
    let llm = ErasureItem {
        id: Uuid::new_v4(),
        erasure: record.id,
        target: ItemTarget::LlmDispatch { generation, provider: "deepseek".into(), remote_ids: vec![], unkept_id_observed: false },
        state: ItemState::Unresolved { reason: "llm_retention_unverified".into(), next_at: NOW },
        attempts: 0,
    };
    let identity = ErasureItem { id: Uuid::new_v4(), target: ItemTarget::Identity, state: ItemState::Pending, ..llm.clone() };
    let mut uow = store.begin().await.unwrap();
    uow.insert_erasure(&record).await.unwrap();
    uow.insert_erasure_items(&[llm.clone(), identity.clone()]).await.unwrap();
    uow.commit().await.unwrap();

    let mut uow = store.begin().await.unwrap();
    let late = ErasureItem {
        target: ItemTarget::LlmDispatch { generation, provider: "deepseek".into(), remote_ids: vec!["cmpl-9".into()], unkept_id_observed: false },
        attempts: 1,
        ..llm.clone()
    };
    uow.save_erasure_item_target(&late).await.unwrap();
    let rewrite_identity = ErasureItem { target: ItemTarget::LlmDispatch { generation, provider: "deepseek".into(), remote_ids: vec![], unkept_id_observed: false }, ..identity.clone() };
    assert!(matches!(uow.save_erasure_item_target(&rewrite_identity).await, Err(StoreError::Conflict(_))), "an identity item is never rewritten");
    let away = ErasureItem { target: ItemTarget::Identity, ..late.clone() };
    assert!(matches!(uow.save_erasure_item_target(&away).await, Err(StoreError::Conflict(_))), "a dispatch item cannot become another kind");
    let missing = ErasureItem { id: Uuid::new_v4(), ..late.clone() };
    assert!(matches!(uow.save_erasure_item_target(&missing).await, Err(StoreError::Conflict(_))));
    let items = uow.erasure_items(record.id).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(items.iter().find(|i| i.id == llm.id), Some(&late));
    assert_eq!(items.iter().find(|i| i.id == identity.id), Some(&identity));
}

/// A completed job with one transcript line long enough for chat context.
async fn chat_ready_job(store: &PgJobStore) -> (AccountId, JobId) {
    let (account, job) = account_with_job(store).await;
    let mut uow = store.begin().await.unwrap();
    let completed = uow.lock_job(job).await.unwrap().unwrap();
    uow.insert_lines(&completed, &[transgate_core::transcript::NewLine {
        start_time: "00:00:00".into(),
        end_time: "00:00:05".into(),
        line: "We agreed to ship the quarterly report on Friday and review the budget next week.".into(),
        speaker: Some("Ada".into()),
        corrected_text: None,
        split_on_speaker_change: None,
        language: None,
        translation_status: None,
    }]).await.unwrap();
    uow.commit().await.unwrap();
    (account, job)
}

async fn user_counts(owner: &PgPool, account: AccountId) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT kind, count FROM transgate_jobs.quota_counters WHERE account_id = $1 AND scope = 'user' ORDER BY kind")
        .bind(account.0)
        .fetch_all(owner)
        .await
        .unwrap()
}

// P4 D2 unknown COMMIT, through the real `Ai` on PostgreSQL. The completing transaction
// (pair, settlement, completion) is either committed or rolled back, and in both cases
// `Ai` is told the COMMIT failed. It reconciles from the stored generation: one pair, one
// settlement of the reservation, a completed generation, one provider call; a fresh process
// running the generation again calls nothing and writes nothing.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn ai_completes_once_when_the_completing_commit_answer_is_lost() {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use transgate_core::ai::quota::estimate_tokens;
    use uncertain_commit::{Arm, LostCommit, UncertainStore};

    let owner = owner().await;
    for mode in [LostCommit::Committed, LostCommit::NotCommitted] {
        let store = store().await;
        let (account, job) = chat_ready_job(&store).await;
        let clock = Arc::new(SharedClock(std::sync::atomic::AtomicI64::new(NOW.0)));
        let arm = Arm::new(mode);
        let llm = Arc::new(CountingLlm { inner: AnsweringLlm("Friday."), opened: Default::default() });
        let uncertain: Arc<dyn JobStore> = Arc::new(UncertainStore { inner: store.clone(), arm: arm.clone() });
        let ai = ai_app_on(uncertain, clock.clone(), llm.clone());
        ai.record_consent(account, true).await.unwrap();
        let ask = transgate_core::ai::ChatRequest { message: "When?".into(), history: vec![] };
        let started = ai.start_chat(account, job, Uuid::new_v4(), ask).await.unwrap();
        let id = started.generation.id;
        *arm.generation.lock().unwrap() = Some(id);
        let reserved = user_counts(&owner, account).await;
        assert_eq!(reserved, vec![("questions".into(), 1), ("tokens".into(), estimate_tokens("When?"))], "{mode:?}: reserved at admission");

        let done = ai.run(id).await.unwrap();
        assert!(arm.fired.load(Ordering::SeqCst), "{mode:?}: the completing COMMIT answer was really lost");
        assert_eq!(done.status, GenerationStatus::Completed, "{mode:?}: reconciled to the stored completion");
        let pair = count(&owner, "SELECT count(*) FROM transgate_jobs.chat_messages WHERE account_id = $1", account).await;
        assert_eq!(pair, 2, "{mode:?}: exactly one pair");
        let settled = vec![("questions".into(), 1), ("tokens".into(), estimate_tokens("When?Friday."))];
        assert_eq!(user_counts(&owner, account).await, settled, "{mode:?}: the reservation settled exactly once");
        let held: Option<serde_json::Value> = sqlx::query_scalar("SELECT quota FROM transgate_jobs.generations WHERE id = $1")
            .bind(id.0)
            .fetch_one(&owner)
            .await
            .unwrap();
        assert_eq!(held, None, "{mode:?}: no reservation is left held");
        assert_eq!(llm.opened.load(Ordering::SeqCst), 1, "{mode:?}: one provider call");

        // A restarted process sees a completed generation: no call, no second pair or settlement.
        let restarted = ai_app_on(Arc::new(store.clone()), clock.clone(), llm.clone());
        assert_eq!(restarted.run(id).await.unwrap().status, GenerationStatus::Completed);
        assert_eq!(llm.opened.load(Ordering::SeqCst), 1, "{mode:?}: no provider replay after restart");
        assert_eq!(count(&owner, "SELECT count(*) FROM transgate_jobs.chat_messages WHERE account_id = $1", account).await, 2);
        assert_eq!(user_counts(&owner, account).await, settled);
    }
}
