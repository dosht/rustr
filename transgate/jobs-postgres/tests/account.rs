//! P5 D7 job feedback and support outbox against a real database through the
//! restricted billing runtime login.
//!
//! Run with `TRANSGATE_JOBS_TEST_DATABASE_URL=... cargo test -p transgate-jobs-postgres --test account -- --include-ignored`.

use billing_core::ports::StoreError;
use billing_core::types::{AccountId, Micros};
use media_ports::{ObjectRef, StoreKind};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use transgate_core::account::ports::SupportOutbox;
use transgate_core::account::{FeedbackRating, JobFeedback, SupportId, SupportRequest};
use transgate_core::ids::JobId;
use transgate_core::jobs::domain::{JobSource, JobStatus, NewJob, StorageProvider};
use transgate_core::jobs::ports::JobStore;
use transgate_jobs_postgres::PgJobStore;
use uuid::Uuid;

const RUNTIME_LOGIN: &str = "transgate_jobs_test_runtime";
const RUNTIME_PASSWORD: &str = "transgate-jobs-test-runtime-password";
const NOW: Micros = Micros(1_768_478_400_000_000);

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

async fn store() -> PgJobStore {
    owner().await;
    let options = billing_postgres::options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap();
    let pool = billing_postgres::connect_runtime(options, 8).await.expect("restricted runtime pool");
    transgate_jobs_postgres::verify_runtime(&pool).await.expect("restricted transgate runtime");
    PgJobStore::new(pool)
}

async fn account_with_job(store: &PgJobStore) -> (AccountId, JobId) {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().create_account(account, NOW).await.unwrap();
    let job = uow
        .insert_job(&NewJob {
            account,
            status: JobStatus::Completed,
            name: Some("call.mp3".into()),
            source: JobSource::Upload {
                storage: StorageProvider::Gcs,
                object: Some(ObjectRef { store: StoreKind::Gcs, bucket: "uploads".into(), key: format!("{}/c.mp3", account.0), version: None }),
            },
            language: None,
            translate_to: vec![],
            speakers: false,
            fix_grammer: None,
            duration_minutes: Some(1),
            size_bytes: Some(1),
            callback_url: None,
            whisper_params: None,
            created_at: NOW,
        })
        .await
        .unwrap();
    uow.commit().await.unwrap();
    (account, job.id)
}

fn feedback(account: AccountId, job: JobId, rating: FeedbackRating, comment: &str) -> JobFeedback {
    JobFeedback { id: Uuid::new_v4(), job, account, rating, comment: comment.into(), created_at: NOW }
}

fn support(account: AccountId, job: Option<JobId>, message: &str, at: i64) -> SupportRequest {
    SupportRequest { id: SupportId(Uuid::new_v4()), account, job, message: message.into(), created_at: Micros(NOW.0 + at) }
}

async fn count(pool: &PgPool, sql: &'static str, account: AccountId) -> i64 {
    sqlx::query_scalar(sql).bind(account.0).fetch_one(pool).await.unwrap()
}

// WEB-031/032, data-model job_feedback: any number of ratings per job, only 0, 5 or 10,
// never edited by the runtime, and gone with a hard job delete or account erasure.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn feedback_is_appended_per_job_and_goes_with_its_job_and_its_account() {
    let store = store().await;
    let owner = owner().await;
    let (account, job) = account_with_job(&store).await;
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().lock_account(account).await.unwrap();
    for (rating, comment) in [(FeedbackRating::Outstanding, "great"), (FeedbackRating::Disappointed, ""), (FeedbackRating::Satisfied, "ok")] {
        uow.account().insert_feedback(&feedback(account, job, rating, comment)).await.unwrap();
    }
    uow.account().insert_support(&support(account, Some(job), "about this job", 0)).await.unwrap();
    uow.commit().await.unwrap();
    let ratings: Vec<i16> = sqlx::query_scalar("SELECT rating FROM transgate_jobs.job_feedback WHERE job_id = $1 ORDER BY rating")
        .bind(job.0)
        .fetch_all(&owner)
        .await
        .unwrap();
    assert_eq!(ratings, vec![0, 5, 10], "several ratings per job, stored as the source values");

    let mut uow = store.begin().await.unwrap();
    let missing = uow.account().insert_feedback(&feedback(account, JobId(i64::MAX - 17), FeedbackRating::Satisfied, "")).await;
    assert!(matches!(missing, Err(StoreError::Conflict(_))), "feedback never outlives its job: {missing:?}");
    uow.rollback().await.unwrap();
    for sql in [
        "UPDATE transgate_jobs.job_feedback SET comment = 'edited'",
        "UPDATE transgate_jobs.support_requests SET message = 'edited'",
        "INSERT INTO transgate_jobs.job_feedback (id, job_id, account_id, rating, comment, created_at) VALUES (gen_random_uuid(), 1, gen_random_uuid(), 3, '', now())",
    ] {
        let error = sqlx::query(sql).execute(store.pool()).await.unwrap_err();
        let code = error.as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default();
        assert!(code == "42501" || code == "23514" || code == "23503", "{sql}: {error}");
    }

    let mut uow = store.begin().await.unwrap();
    uow.hard_delete_job(job).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_jobs.job_feedback WHERE account_id = $1", account).await, 0, "PRV-005 cascade");
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_jobs.support_requests WHERE account_id = $1", account).await, 1, "support keeps its job only as context");

    let mut uow = store.begin().await.unwrap();
    uow.erase_account_rows(account).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count(&owner, "SELECT count(*) FROM transgate_jobs.support_requests WHERE account_id = $1", account).await, 0, "erasure removes undelivered support");
}

// API-F38, EML-006: undelivered requests are claimed oldest first under a lease, never by
// two workers at once, reclaimed after the lease, released for a retry time and deleted on
// ack. A claim whose lease was superseded can neither ack nor release (attempts fence).
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn the_support_outbox_leases_oldest_first_and_only_the_current_claim_settles() {
    use transgate_core::account::ports::ClaimedSupport;
    let store = store().await;
    let (account, _) = account_with_job(&store).await;
    let far = Micros(NOW.0 + 400 * 86_400_000_000);
    let first = support(account, None, "first", 1);
    let second = support(account, None, "second", 2);
    let mut uow = store.begin().await.unwrap();
    uow.account().insert_support(&second).await.unwrap();
    uow.account().insert_support(&first).await.unwrap();
    uow.commit().await.unwrap();

    let mine = |claimed: Vec<ClaimedSupport>| -> Vec<ClaimedSupport> { claimed.into_iter().filter(|c| c.request.account == account).collect() };
    let claimed = mine(store.claim_support(1000, 60, far).await.unwrap());
    assert_eq!(claimed.iter().map(|c| (c.request.message.as_str(), c.attempts)).collect::<Vec<_>>(), vec![("first", 1), ("second", 1)]);
    assert!(mine(store.claim_support(1000, 60, far).await.unwrap()).is_empty(), "a leased request is not claimed twice");
    let (first_claim, second_claim) = (claimed[0].clone(), claimed[1].clone());

    // The first worker stalls past its lease; another worker reclaims the request.
    let after_lease = Micros(far.0 + 61_000_000);
    let reclaimed = mine(store.claim_support(1000, 60, after_lease).await.unwrap());
    assert_eq!(reclaimed.iter().map(|c| (c.request.message.as_str(), c.attempts)).collect::<Vec<_>>(), vec![("first", 2), ("second", 2)]);
    assert!(!store.ack_support(&first_claim).await.unwrap(), "the stale claim cannot delete the reclaimed request");
    assert!(!store.release_support(&second_claim, "late", after_lease).await.unwrap(), "nor release it");

    let current_first = reclaimed[0].clone();
    assert_eq!(current_first.request, first.clone(), "the request round-trips");
    assert!(store.release_support(&current_first, "smtp timeout", Micros(after_lease.0 + 600_000_000)).await.unwrap());
    assert!(store.ack_support(&reclaimed[1]).await.unwrap());
    assert!(mine(store.claim_support(1000, 60, Micros(after_lease.0 + 120_000_000)).await.unwrap()).is_empty(), "released until its retry time; acked is gone");
    let retried = mine(store.claim_support(1000, 60, Micros(after_lease.0 + 600_000_000)).await.unwrap());
    assert_eq!(retried.iter().map(|c| c.attempts).collect::<Vec<_>>(), vec![3]);
    assert!(store.ack_support(&retried[0]).await.unwrap());
    let later = Micros(after_lease.0 + 86_400_000_000);
    assert!(mine(store.claim_support(1000, 60, later).await.unwrap()).is_empty());
}
