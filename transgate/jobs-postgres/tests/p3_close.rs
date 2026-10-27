//! P3 closing tests on a real database through the restricted runtime login. Each test runs in
//! its own freshly created database: retention is global to a database, so sharing one with
//! other tests would sweep their rows.
//!
//! Run with `TRANSGATE_JOBS_TEST_DATABASE_URL=... cargo test -p transgate-jobs-postgres --test p3_close -- --include-ignored`.
//! A missing variable fails the test, it never skips.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use billing_core::types::{AccountId, Clock, Micros};
use sqlx::{AssertSqlSafe, PgPool};
use tokio::sync::OnceCell;
use transgate_core::ids::{JobId, LineId, MICROS_PER_DAY};
use transgate_core::jobs::app::{Jobs, JobsConfig, JobsServices};
use transgate_core::jobs::domain::{JobSource, JobStatus, NewJob, StorageProvider};
use transgate_core::jobs::ports::{BoxFuture, IdentityAdmin, IdentityOutcome, JobQueries, JobStore, Locks, Sleeper};
use transgate_core::jobs::routing::{ModelConfig, RoutingConfig};
use transgate_core::languages::Language;
use transgate_core::privacy::app::{Privacy, PrivacyServices};
use transgate_core::privacy::erasure::ExceptionPolicy;
use transgate_core::privacy::retention::{RetentionDays, RetentionOutcome, RunStatus, SAFETY_THRESHOLD, TRANSCRIPT_LOCK};
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::{LineEdit, NewLine};
use transgate_jobs_postgres::{PgJobStore, PgLocks, PgRateLimiter};
use uuid::Uuid;

const LOGIN: &str = "transgate_jobs_p3close_runtime";
const PASSWORD: &str = "transgate-jobs-p3close-runtime-password";
const NOW: Micros = Micros(1_768_478_400_000_000); // 2026-01-15T12:00:00Z

fn base_url() -> String {
    std::env::var("TRANSGATE_JOBS_TEST_DATABASE_URL")
        .expect("TRANSGATE_JOBS_TEST_DATABASE_URL must name a disposable PostgreSQL database for these tests")
}

static PROVISIONED: OnceCell<()> = OnceCell::const_new();

/// A database of this test's own, dropped afterwards.
struct Database {
    name: String,
}
impl Drop for Database {
    fn drop(&mut self) {
        let (name, url) = (self.name.clone(), base_url());
        let _ = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            runtime.block_on(async move {
                if let Ok(owner) = billing_postgres::connect_owner(&url).await {
                    let _ = sqlx::query(AssertSqlSafe(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))).execute(&owner).await;
                }
            });
        })
        .join();
    }
}

struct FixedClock(AtomicI64);
impl Clock for FixedClock {
    fn now(&self) -> Micros {
        Micros(self.0.load(Ordering::SeqCst))
    }
}
struct NoSleep;
impl Sleeper for NoSleep {
    fn sleep(&self, _: u64) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}
struct NoIdentity;
impl IdentityAdmin for NoIdentity {
    fn delete_user(&self, _: AccountId) -> BoxFuture<'_, Result<IdentityOutcome, String>> {
        Box::pin(async { Ok(IdentityOutcome::Absent) })
    }
}

struct World {
    owner: PgPool,
    store: PgJobStore,
    privacy: Arc<Privacy>,
    locks: Arc<PgLocks>,
    jobs: Arc<Jobs>,
    soniox: Arc<Fetch>,
    _database: Database,
}

/// A Soniox transport whose transcript fetch (the only call these tests reach) is scripted.
#[derive(Default)]
struct Fetch {
    answer: std::sync::Mutex<Option<Result<media_ports::ProviderObservation, media_ports::TransportError>>>,
}
impl Fetch {
    fn answer(&self, answer: Result<media_ports::ProviderObservation, media_ports::TransportError>) {
        *self.answer.lock().unwrap() = Some(answer);
    }
}
impl media_ports::Transcriber for Fetch {
    fn provider(&self) -> media_ports::Provider {
        media_ports::Provider::Soniox
    }
    fn project(&self) -> &str {
        "project"
    }
    fn submit<'a>(&'a self, _: &'a media_ports::TranscriptionRequest) -> BoxFuture<'a, Result<media_ports::Submission, media_ports::TransportError>> {
        Box::pin(async { Err(media_ports::TransportError::Unsupported("not used".into())) })
    }
    fn status<'a>(&'a self, _: &'a media_ports::RemoteIdentity) -> BoxFuture<'a, Result<media_ports::ProviderObservation, media_ports::TransportError>> {
        Box::pin(async { Err(media_ports::TransportError::Unsupported("not used".into())) })
    }
    fn result<'a>(&'a self, _: &'a media_ports::RemoteIdentity) -> BoxFuture<'a, Result<media_ports::ProviderObservation, media_ports::TransportError>> {
        Box::pin(async { Err(media_ports::TransportError::Unsupported("not used".into())) })
    }
    fn delete_remote<'a>(&'a self, _: &'a media_ports::RemoteIdentity) -> BoxFuture<'a, Result<media_ports::DeleteOutcome, media_ports::TransportError>> {
        Box::pin(async { Ok(media_ports::DeleteOutcome::Absent) })
    }
    fn find_by_reference<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<media_ports::RemoteRecord>, media_ports::TransportError>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn verify_callback(&self, _: &media_ports::CallbackBinding, _: &media_ports::Headers, _: &[u8]) -> Result<media_ports::ProviderObservation, media_ports::CallbackError> {
        Err(media_ports::CallbackError::Malformed)
    }
    fn complete_callback(&self, _: media_ports::ProviderObservation) -> BoxFuture<'_, Result<media_ports::ProviderObservation, media_ports::TransportError>> {
        let answer = self.answer.lock().unwrap().clone().unwrap_or(Err(media_ports::TransportError::Unavailable { status: None, operation: "transcript".into() }));
        Box::pin(async move { answer })
    }
}

async fn world() -> World {
    let base = base_url();
    let name = format!("transgate_p3_{}", Uuid::new_v4().simple());
    let admin = billing_postgres::connect_owner(&base).await.expect("owner connection");
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{name}\""))).execute(&admin).await.expect("create an isolated database");
    let database = Database { name: name.clone() };
    let url = format!("{}/{name}", base.rsplit_once('/').expect("a database URL").0);
    let owner = billing_postgres::connect_owner(&url).await.expect("owner connection to the isolated database");
    billing_postgres::migrate(&owner).await.expect("billing migrations");
    transgate_jobs_postgres::migrate(&owner).await.expect("jobs migrations");
    // The login is a cluster role: provision it once, after a migration created the group.
    PROVISIONED
        .get_or_init(|| async { billing_postgres::provision_runtime_login(&owner, LOGIN, PASSWORD).await.expect("runtime login") })
        .await;
    let options = billing_postgres::options_for_login(&url, LOGIN, PASSWORD).unwrap();
    let pool = billing_postgres::connect_runtime(options, 8).await.expect("restricted runtime pool");
    transgate_jobs_postgres::verify_runtime(&pool).await.expect("restricted transgate runtime");
    let store = PgJobStore::new(pool.clone());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(AtomicI64::new(NOW.0)));
    let locks = Arc::new(PgLocks::new(pool.clone()));
    let privacy = Arc::new(Privacy::new(PrivacyServices {
        store: Arc::new(store.clone()),
        clock: clock.clone(),
        locks: locks.clone(),
        objects: vec![],
        inventories: vec![],
        transcribers: vec![],
        identity: Arc::new(NoIdentity),
        billing: Arc::new(billing_postgres::PgBillingStore::new(pool.clone())),
        terminators: vec![],
        exceptions: ExceptionPolicy::default(),
    }));
    let soniox = Arc::new(Fetch::default());
    let jobs = Arc::new(Jobs::new(
        JobsServices {
            store: Arc::new(store.clone()),
            clock,
            objects: vec![],
            transcribers: vec![soniox.clone()],
            stager: None,
            sleeper: Arc::new(NoSleep),
            rate_limiter: Arc::new(PgRateLimiter::new(pool)),
            paid_captions: None,
            video_metadata: None,
        },
        JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: 200 },
            models: ModelConfig { soniox: "m".into(), soniox_v5: "m".into(), deepgram: "m".into(), deepgram_account_overrides: vec![] },
            upload_storage: StorageProvider::Gcs,
            upload_bucket: "uploads".into(),
            archive_store: media_ports::StoreKind::Gcs,
            archive_bucket: "archive".into(),
            staging_store: media_ports::StoreKind::Gcs,
            staging_bucket: "staging".into(),
            callback_base_url: "https://transgate.test".into(),
            soniox_auto_delete: false,
            ssrf_protection: true,
            freemium_lists: Default::default(),
        },
    ));
    World { owner, store, privacy, locks, jobs, soniox, _database: database }
}

fn line(start: &str, text: &str, speaker: Option<&str>) -> NewLine {
    NewLine {
        start_time: start.into(),
        end_time: start.into(),
        line: text.into(),
        speaker: speaker.map(str::to_string),
        corrected_text: None,
        split_on_speaker_change: None,
        language: Language::from_code("en"),
        translation_status: None,
    }
}

struct Owner {
    account: AccountId,
}

/// An account with the given settings row (none leaves the source default).
async fn account(w: &World, settings: Option<(bool, bool)>) -> Owner {
    let account = AccountId(Uuid::new_v4());
    let mut uow = w.store.begin().await.unwrap();
    uow.billing().await.unwrap().create_account(account, NOW).await.unwrap();
    if let Some((auto_delete, unlimited)) = settings {
        uow.save_settings(&UserSettings { auto_delete_transcripts: auto_delete, unlimited_history: unlimited, ..UserSettings::defaults(account) }).await.unwrap();
    }
    uow.commit().await.unwrap();
    Owner { account }
}

/// A job of `status` created `age_days` days before `NOW` plus `extra_micros`, with `lines` lines and
/// a stored completion summary (an AI artifact retention must keep).
async fn job(w: &World, owner: &Owner, status: JobStatus, age_days: i64, extra_micros: i64, lines: usize) -> JobId {
    let created_at = Micros(NOW.0 - age_days * MICROS_PER_DAY + extra_micros);
    let mut uow = w.store.begin().await.unwrap();
    let job = uow
        .insert_job(&NewJob {
            account: owner.account,
            status,
            name: Some(format!("{age_days}-day-old.mp3")),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: Language::from_code("en"),
            translate_to: vec![],
            speakers: false,
            fix_grammer: None,
            duration_minutes: Some(3),
            size_bytes: Some(1),
            callback_url: None,
            whisper_params: None,
            created_at,
        })
        .await
        .unwrap();
    let rows: Vec<NewLine> = (0..lines.min(50)).map(|i| line(&format!("00:00:{i:02}"), "words", None)).collect();
    uow.insert_lines(&job, &rows).await.unwrap();
    uow.commit().await.unwrap();
    sqlx::query("INSERT INTO transgate_jobs.completion_summaries (job_id, source_revision, account_id, generation_id, text, created_at) VALUES ($1, 0, $2, $3, 'a summary', now())")
        .bind(job.id.0)
        .bind(owner.account.0)
        .bind(Uuid::new_v4())
        .execute(&w.owner)
        .await
        .unwrap();
    job.id
}

async fn line_count(w: &World, job: JobId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.lines WHERE job_id = $1").bind(job.0).fetch_one(&w.owner).await.unwrap()
}

async fn kept(w: &World, job: JobId) -> (i64, i64) {
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.jobs WHERE id = $1").bind(job.0).fetch_one(&w.owner).await.unwrap();
    let summaries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.completion_summaries WHERE job_id = $1").bind(job.0).fetch_one(&w.owner).await.unwrap();
    (jobs, summaries)
}

fn thirty() -> RetentionDays {
    RetentionDays::new(Some(30)).unwrap()
}

// PRV-014, PRV-015, PRV-017: the sweep deletes only transcript lines, only of completed jobs created
// at or before `now - 30 days - 1 day`, only for owners who keep auto-delete on, whatever their
// `unlimited_history` says. Jobs, their statuses and metadata and the AI artifacts survive.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn prv_014_015_017_the_sweep_deletes_only_old_completed_lines_of_owners_who_keep_auto_delete_on() {
    let w = world().await;
    let opted_in = account(&w, Some((true, false))).await;
    let opted_out = account(&w, Some((false, false))).await;
    let unlimited = account(&w, Some((true, true))).await;
    let no_row = account(&w, None).await;

    let at_cutoff = job(&w, &opted_in, JobStatus::Completed, 31, 0, 3).await; // created exactly at the cutoff
    let just_inside = job(&w, &opted_in, JobStatus::Completed, 31, 1, 3).await; // one microsecond newer
    let thirty_days = job(&w, &opted_in, JobStatus::Completed, 30, 0, 3).await; // PRV-015: kept
    let very_old = job(&w, &opted_in, JobStatus::Completed, 400, 0, 3).await;
    let failed = job(&w, &opted_in, JobStatus::Failed, 400, 0, 3).await; // PRV-014: only completed jobs
    let running = job(&w, &opted_in, JobStatus::Running, 400, 0, 3).await;
    let draft = job(&w, &opted_in, JobStatus::Draft, 400, 0, 3).await;
    let kept_by_owner = job(&w, &opted_out, JobStatus::Completed, 400, 0, 3).await;
    let unlimited_history = job(&w, &unlimited, JobStatus::Completed, 400, 0, 3).await; // PRV-017
    let absent_row = job(&w, &no_row, JobStatus::Completed, 400, 0, 3).await; // the source default is on

    let outcome = w.privacy.retention_run(thirty(), "test").await.unwrap();
    assert_eq!(outcome, RetentionOutcome::Success { rows_deleted: 4 * 3, users_opted_out: 1 }, "at the cutoff, very old, unlimited history and the absent row");

    for (name, id, swept) in [
        ("created exactly at the cutoff (31 days)", at_cutoff, true),
        ("one microsecond inside the cutoff", just_inside, false),
        ("exactly 30 days old (PRV-015)", thirty_days, false),
        ("400 days old", very_old, true),
        ("a failed job (PRV-014)", failed, false),
        ("a running job", running, false),
        ("a draft", draft, false),
        ("an owner who opted out", kept_by_owner, false),
        ("unlimited_history does not change cleanup (PRV-017)", unlimited_history, true),
        ("an owner without a settings row", absent_row, true),
    ] {
        assert_eq!(line_count(&w, id).await, if swept { 0 } else { 3 }, "{name}");
        assert_eq!(kept(&w, id).await, (1, 1), "{name}: the job row and its AI summary are always preserved");
    }
    let (status, name): (String, Option<String>) = sqlx::query_as("SELECT status, job_name FROM transgate_jobs.jobs WHERE id = $1").bind(failed.0).fetch_one(&w.owner).await.unwrap();
    let name = name.unwrap_or_default();
    assert_eq!((status.as_str(), name.as_str()), ("failed", "400-day-old.mp3"), "metadata and status are untouched");
}

// PRV-021: every run writes one audit row with the job name, table, retention days, rows deleted,
// execution time, status, error text, opted-out count and executor, including a skipped run (the lock
// is held) and a failed run (the error is recorded after the rollback and still raised).
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn prv_021_every_run_writes_one_audit_row_with_all_its_fields() {
    let w = world().await;
    let opted_in = account(&w, Some((true, false))).await;
    let opted_out = account(&w, Some((false, false))).await;
    job(&w, &opted_in, JobStatus::Completed, 40, 0, 4).await;
    job(&w, &opted_out, JobStatus::Completed, 40, 0, 2).await;
    type Row = (String, String, i32, i64, i64, String, Option<String>, Option<i64>, String);
    let audit = || async {
        sqlx::query_as::<_, Row>(
            "SELECT job_name, table_name, retention_days, rows_deleted, execution_time_ms, status, error_message, users_opted_out_count, executed_by \
             FROM transgate_jobs.retention_runs ORDER BY executed_at, id",
        )
        .fetch_all(&w.owner)
        .await
        .unwrap()
    };
    let runs = |rows: &[Row]| rows.iter().map(|r| (r.5.clone(), r.3, r.7, r.8.clone())).collect::<Vec<_>>();

    // Success.
    assert_eq!(w.privacy.retention_run(thirty(), "scheduler").await.unwrap(), RetentionOutcome::Success { rows_deleted: 4, users_opted_out: 1 });
    let rows = audit().await;
    assert_eq!(rows.len(), 1);
    let first = &rows[0];
    assert_eq!((first.0.as_str(), first.2, first.3, first.6.as_deref(), first.7, first.8.as_str()), ("cleanup-old-transcripts", 30, 4, None, Some(1), "scheduler"));
    assert_eq!(first.5, RunStatus::Success.as_str());
    assert!(!first.1.is_empty() && first.4 >= 0, "table {:?}, elapsed {}", first.1, first.4);

    // Skipped: another holder has the lock.
    let guard = w.locks.try_lock(TRANSCRIPT_LOCK).await.unwrap().expect("the lock is free");
    assert_eq!(w.privacy.retention_run(thirty(), "operator").await.unwrap(), RetentionOutcome::Skipped);
    guard.release().await;
    let rows = audit().await;
    assert_eq!(rows.len(), 2);
    let skipped: Vec<_> = runs(&rows).into_iter().filter(|r| r.3 == "operator").collect();
    assert_eq!(skipped, vec![("skipped".to_string(), 0, None, "operator".to_string())]);

    // Failure: the store fails mid-run; the error is raised and recorded with its message and executor.
    sqlx::query("ALTER TABLE transgate_jobs.lines RENAME TO lines_hidden").execute(&w.owner).await.unwrap();
    let failed = w.privacy.retention_run(thirty(), "ops").await;
    sqlx::query("ALTER TABLE transgate_jobs.lines_hidden RENAME TO lines").execute(&w.owner).await.unwrap();
    assert!(failed.is_err(), "the failure is re-raised: {failed:?}");
    let rows = audit().await;
    assert_eq!(rows.len(), 3, "one row per run: {rows:?}");
    let last = rows.iter().find(|r| r.8 == "ops").expect("the failed run's audit row");
    assert_eq!((last.5.as_str(), last.3), ("failure", 0));
    assert!(last.6.as_deref().is_some_and(|m| !m.is_empty()), "the error text is recorded");
}

// PRV-019: more than 100000 rows deletes nothing, writes an `aborted` audit row with the source
// message and does not raise a partial deletion; exactly 100000 rows is deleted.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn prv_019_the_sweep_is_all_or_nothing_at_100000_versus_100001_rows() {
    let w = world().await;
    let opted_in = account(&w, Some((true, false))).await;
    let big = job(&w, &opted_in, JobStatus::Completed, 40, 0, 1).await;
    let fill = |count: i64| {
        let (pool, job, account) = (w.owner.clone(), big.0, opted_in.account.0);
        async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.lines (job_id, account_id, start_time, end_time, line) \
                 SELECT $1, $2, '00:01:00', '00:01:00', 'filler' FROM generate_series(1, $3)",
            )
            .bind(job)
            .bind(account)
            .bind(count)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    // 1 existing line + 100000 filler = 100001.
    fill(SAFETY_THRESHOLD as i64).await;
    assert_eq!(line_count(&w, big).await, 100_001);
    let outcome = w.privacy.retention_run(thirty(), "test").await.unwrap();
    assert_eq!(outcome, RetentionOutcome::Aborted { would_delete: 100_001 });
    assert_eq!(line_count(&w, big).await, 100_001, "an aborted run deletes nothing");
    let (status, message, rows): (String, Option<String>, i64) =
        sqlx::query_as("SELECT status, error_message, rows_deleted FROM transgate_jobs.retention_runs ORDER BY executed_at DESC LIMIT 1").fetch_one(&w.owner).await.unwrap();
    assert_eq!((status.as_str(), rows), ("aborted", 0));
    assert_eq!(message.as_deref(), Some("SAFETY THRESHOLD EXCEEDED: Would delete 100001 rows (limit: 100000)"));

    // One row fewer: exactly the threshold passes.
    sqlx::query("DELETE FROM transgate_jobs.lines WHERE id = (SELECT min(id) FROM transgate_jobs.lines WHERE job_id = $1)").bind(big.0).execute(&w.owner).await.unwrap();
    assert_eq!(line_count(&w, big).await, 100_000);
    assert_eq!(w.privacy.retention_run(thirty(), "test").await.unwrap(), RetentionOutcome::Success { rows_deleted: 100_000, users_opted_out: 0 });
    assert_eq!(line_count(&w, big).await, 0);
}

// PRV-026: transcript lines belong to an existing job. A line for a missing job is refused, a job that
// still has lines cannot be deleted (ON DELETE RESTRICT), and the controlled deletion removes the
// lines first.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn prv_026_lines_need_an_existing_job_and_a_job_with_lines_cannot_be_deleted() {
    let w = world().await;
    let person = account(&w, None).await;
    let id = job(&w, &person, JobStatus::Completed, 1, 0, 2).await;
    let code = |result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>| -> String {
        result.unwrap_err().as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default()
    };

    // An orphan line: refused (foreign key violation) even for the owner role.
    let orphan = sqlx::query("INSERT INTO transgate_jobs.lines (job_id, account_id, start_time, end_time, line) VALUES ($1, $2, '0', '0', 'x')")
        .bind(i64::MAX - 3)
        .bind(person.account.0)
        .execute(&w.owner)
        .await;
    assert_eq!(code(orphan), "23503");
    // A job with lines is RESTRICTed: the delete fails and both rows stay.
    let delete = sqlx::query("DELETE FROM transgate_jobs.jobs WHERE id = $1").bind(id.0).execute(&w.owner).await;
    assert_eq!(code(delete), "23503");
    assert_eq!((line_count(&w, id).await, kept(&w, id).await.0), (2, 1));
    // The controlled deletion (the use case) removes the lines and then the job.
    let mut uow = w.store.begin().await.unwrap();
    uow.hard_delete_job(id).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!((line_count(&w, id).await, kept(&w, id).await.0), (0, 0));
}

// EDT-005, EDT-006: choosing a speaker changes that one line; renaming a speaker changes every line of
// the job with the same speaker text. Two renames racing on one job end atomic: one wins completely,
// never a mix.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn edt_005_006_a_speaker_choice_changes_one_line_and_racing_renames_never_mix() {
    let w = world().await;
    let person = account(&w, None).await;
    let mut uow = w.store.begin().await.unwrap();
    let created = uow
        .insert_job(&NewJob {
            account: person.account,
            status: JobStatus::Completed,
            name: Some("talk.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: Language::from_code("en"),
            translate_to: vec![],
            speakers: true,
            fix_grammer: None,
            duration_minutes: Some(3),
            size_bytes: Some(1),
            callback_url: None,
            whisper_params: None,
            created_at: NOW,
        })
        .await
        .unwrap();
    let rows: Vec<NewLine> = (0..24).map(|i| line(&format!("00:00:{i:02}"), "words", Some(if i % 2 == 0 { "SPEAKER_00" } else { "SPEAKER_01" }))).collect();
    uow.insert_lines(&created, &rows).await.unwrap();
    let id = created.id;
    uow.commit().await.unwrap();
    let speakers = |job: JobId| {
        let pool = w.owner.clone();
        async move { sqlx::query_scalar::<_, Option<String>>("SELECT speaker FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY start_time, id").bind(job.0).fetch_all(&pool).await.unwrap() }
    };
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY start_time, id").bind(id.0).fetch_all(&w.owner).await.unwrap();

    // EDT-005: one line only.
    w.jobs.edit(person.account, LineEdit::Speaker { line: LineId(ids[0]), speaker: "Ada".into() }).await.unwrap();
    let after = speakers(id).await;
    assert_eq!(after.iter().filter(|s| s.as_deref() == Some("Ada")).count(), 1, "exactly one line changed");
    assert_eq!(after[0].as_deref(), Some("Ada"));
    assert_eq!(after.iter().filter(|s| s.as_deref() == Some("SPEAKER_00")).count(), 11, "the other lines of the same speaker keep their text");

    // EDT-006: a rename changes every line with that speaker and no other.
    w.jobs.edit(person.account, LineEdit::RenameSpeaker { job: id, from: "SPEAKER_01".into(), to: "Sam".into() }).await.unwrap();
    let after = speakers(id).await;
    assert_eq!(after.iter().filter(|s| s.as_deref() == Some("Sam")).count(), 12);
    assert_eq!(after.iter().filter(|s| s.as_deref() == Some("SPEAKER_01")).count(), 0);

    // Racing renames of the same speaker: each round one name wins completely.
    for round in 0..8 {
        let (from, a, b) = ("SPEAKER_00", format!("A{round}"), format!("B{round}"));
        let (first, second) = tokio::join!(
            w.jobs.edit(person.account, LineEdit::RenameSpeaker { job: id, from: from.into(), to: a.clone() }),
            w.jobs.edit(person.account, LineEdit::RenameSpeaker { job: id, from: from.into(), to: b.clone() }),
        );
        assert!(first.is_ok() || second.is_ok(), "one rename applies: {first:?} / {second:?}");
        let names: std::collections::BTreeSet<String> = speakers(id)
            .await
            .into_iter()
            .flatten()
            .filter(|s| s != "Sam" && s != "Ada")
            .collect();
        assert_eq!(names.len(), 1, "round {round}: lines of one speaker never end with two names: {names:?}");
        assert!(names.contains(&a) || names.contains(&b));
        // Put the speaker back for the next round.
        let current = names.into_iter().next().unwrap();
        w.jobs.edit(person.account, LineEdit::RenameSpeaker { job: id, from: current, to: from.into() }).await.unwrap();
    }
}

// TRX-023, LNG-013: a stored translation target without an ISO code (a name the table does not know)
// makes the job fail with 'Translation language mapping failed', with no fallback. It is a failed job,
// never a row the store cannot decode: the account's reads and the worker passes keep working.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn trx_023_lng_013_a_stored_target_without_a_code_fails_the_job_and_never_breaks_a_read() {
    let w = world().await;
    let person = account(&w, None).await;
    let healthy = job(&w, &person, JobStatus::Pending, 1, 0, 0).await;
    let broken = job(&w, &person, JobStatus::Pending, 2, 0, 0).await;
    sqlx::query("UPDATE transgate_jobs.jobs SET translate_to = ARRAY['Klingon'], language = 'English' WHERE id = $1").bind(broken.0).execute(&w.owner).await.unwrap();

    // Reads decode both jobs; the unknown name is not a corrupt row.
    let owned = w.store.owned_job(person.account, broken).await.expect("an unknown stored target is not a decode error");
    assert_eq!(owned.map(|j| j.id), Some(broken));
    let listed = w.store.list_jobs(&transgate_core::jobs::ports::JobListQuery { account: person.account, statuses: vec![JobStatus::Pending], since: None, before: None, limit: 20 }).await.unwrap();
    assert_eq!(listed.len(), 2, "the healthy job stays readable beside it");

    // The worker pass fails the job with the source message and still handles the healthy one.
    let report = w.jobs.dispatch_pass().await.expect("a pass over a job with an unmapped target does not error");
    assert!(report.errors == 0, "{report:?}");
    let (status, error): (String, Option<serde_json::Value>) =
        sqlx::query_as("SELECT status, error FROM transgate_jobs.jobs WHERE id = $1").bind(broken.0).fetch_one(&w.owner).await.unwrap();
    assert_eq!(status, "failed");
    let error = error.unwrap();
    let message = error["message"].as_str().unwrap_or_default();
    assert!(message.starts_with("Translation language mapping failed") && message.contains("Klingon"), "{error}");
    assert_eq!(error["code"], "invalid_request");
    let healthy_error: Option<serde_json::Value> = sqlx::query_scalar("SELECT error FROM transgate_jobs.jobs WHERE id = $1").bind(healthy.0).fetch_one(&w.owner).await.unwrap();
    assert!(
        !healthy_error.map(|e| e.to_string()).unwrap_or_default().contains("mapping failed"),
        "only the unmapped job fails for its target; this world has no providers, so the other fails for that"
    );
    // The saved failed job still carries the stored name.
    let stored: Vec<String> = sqlx::query_scalar("SELECT unnest(translate_to) FROM transgate_jobs.jobs WHERE id = $1").bind(broken.0).fetch_all(&w.owner).await.unwrap();
    assert_eq!(stored, vec!["Klingon".to_string()], "saving the failed job keeps the stored target");
}

// ---- ingestion, edits and races ------------------------------------------------------------------

use media_ports::{ObservationOrigin, ProviderObservation, ProviderPayload, RemoteIdentity, SecretString, SonioxReported, SonioxToken, SonioxTranscript};
use transgate_core::ids::AttemptId;
use transgate_core::jobs::app::IntakeAck;
use transgate_core::jobs::domain::{Attempt, AttemptRequest, AttemptState, Provider, RemoteCleanup};
use transgate_core::jobs::ingest;

fn token(text: &str, start: f64, end: f64, speaker: Option<i64>, language: Option<&str>, status: Option<&str>) -> SonioxToken {
    SonioxToken {
        text: text.into(),
        start_ms: Some(start),
        end_ms: Some(end),
        speaker: speaker.map(|s| serde_json::json!(s)),
        language: language.map(str::to_string),
        translation_status: status.map(str::to_string),
        is_audio_event: None,
        extra: Default::default(),
    }
}

fn soniox_observation(remote: &str, tokens: Option<Vec<SonioxToken>>) -> ProviderObservation {
    ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Soniox, project: "project".into(), id: remote.into() },
        origin: ObservationOrigin::Webhook,
        payload: ProviderPayload::Soniox {
            reported: SonioxReported { id: remote.into(), status: "completed".into(), audio_duration_ms: Some(120_000.0), error: None, error_type: None, error_message: None, extra: Default::default() },
            transcript: tokens.map(|tokens| SonioxTranscript { id: remote.into(), tokens, text: None, metadata: serde_json::Value::Null, extra: Default::default() }),
        },
    }
}

fn deepgram_observation(request: &str, no_paragraphs: bool) -> ProviderObservation {
    let paragraphs = if no_paragraphs {
        vec![]
    } else {
        vec![media_ports::DeepgramParagraph {
            sentences: vec![media_ports::DeepgramSentence { text: "Hello.".into(), start: 0.0, end: 1.0, extra: Default::default() }],
            speaker: Some(0),
            extra: Default::default(),
        }]
    };
    ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Deepgram, project: "project".into(), id: request.into() },
        origin: ObservationOrigin::Webhook,
        payload: ProviderPayload::Deepgram(media_ports::DeepgramTranscript {
            metadata: media_ports::DeepgramMetadata { request_id: request.into(), duration: Some(60.0), extra: Default::default() },
            results: media_ports::DeepgramResults {
                channels: vec![media_ports::DeepgramChannel {
                    alternatives: vec![media_ports::DeepgramAlternative {
                        transcript: String::new(),
                        words: vec![],
                        paragraphs: Some(media_ports::DeepgramParagraphs { paragraphs, extra: Default::default() }),
                        extra: Default::default(),
                    }],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
            extra: Default::default(),
        }),
    }
}

/// A running job of `provider` with its one accepted attempt, waiting for a result.
async fn running(w: &World, owner: &Owner, provider: Provider, speakers: bool, translate_to: &[&str], remote: &str) -> (JobId, Attempt) {
    let mut uow = w.store.begin().await.unwrap();
    let created = uow
        .insert_job(&NewJob {
            account: owner.account,
            status: JobStatus::Running,
            name: Some("call.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: Language::from_code("en"),
            translate_to: translate_to.iter().filter_map(|n| Language::from_name(n)).collect(),
            speakers,
            fix_grammer: None,
            duration_minutes: Some(2),
            size_bytes: Some(1),
            callback_url: None,
            whisper_params: None,
            created_at: NOW,
        })
        .await
        .unwrap();
    let attempt = Attempt {
        id: AttemptId::new(),
        job: created.id,
        account: owner.account,
        number: 1,
        provider,
        project: "project".into(),
        model: "m".into(),
        state: AttemptState::Accepted,
        claimed_at: NOW,
        lease_until: Micros(NOW.0 + 120_000_000),
        request: AttemptRequest {
            language_code: Some("en".into()),
            translation_target: None,
            diarize: speakers,
            audio: media_ports::ObjectRef { store: media_ports::StoreKind::Gcs, bucket: "uploads".into(), key: format!("{}/a.mp3", owner.account.0), version: None },
            client_reference: format!("job_{}", created.id.0),
        },
        callback_secret: SecretString::new("secret"),
        remote_id: Some(remote.into()),
        accepted_at: Some(NOW),
        last_error: None,
        remote_cleanup: RemoteCleanup::NotRequested,
    };
    uow.insert_attempt(&attempt).await.unwrap();
    let mut job = uow.lock_job(created.id).await.unwrap().unwrap();
    job.current_attempt = Some(attempt.id);
    job.provider = Some(provider);
    job.started_at = Some(NOW);
    if provider == Provider::Soniox {
        job.soniox_transcription_id = Some(remote.into());
    }
    uow.save_job(&job, job.revision).await.unwrap();
    uow.commit().await.unwrap();
    (created.id, attempt)
}

async fn deliver(w: &World, attempt: &Attempt, observation: &ProviderObservation) -> transgate_core::ids::InboxId {
    match w.jobs.intake(attempt, observation.clone(), ingest::fingerprint(observation)).await.unwrap() {
        IntakeAck::Stored { inbox } => inbox,
        other => panic!("{other:?}"),
    }
}

async fn job_row(w: &World, job: JobId) -> (String, Option<serde_json::Value>) {
    sqlx::query_as("SELECT status, error FROM transgate_jobs.jobs WHERE id = $1").bind(job.0).fetch_one(&w.owner).await.unwrap()
}

// TRX-029, TRX-031: through real ingestion every line keeps its normalized optional fields in the
// database: the token language and translation status (null for a plain transcription), the speaker,
// and `split_on_speaker_change` as a copy of the job's `speakers` setting. Nothing else (word times,
// confidence, paragraphs) is stored on a line.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn trx_029_031_every_optional_normalized_line_field_survives_the_database_round_trip() {
    let w = world().await;
    let person = account(&w, None).await;

    // A translation job with speakers: originals and their translations, each with its language.
    let (job, attempt) = running(&w, &person, Provider::Soniox, true, &["German"], "tx-translated").await;
    let observation = soniox_observation("tx-translated", Some(vec![
        token("Hello there.", 0.0, 1800.0, Some(1), Some("en"), Some("original")),
        token("Hallo zusammen.", 0.0, 1800.0, Some(1), Some("de"), Some("translation")),
    ]));
    let inbox = deliver(&w, &attempt, &observation).await;
    w.jobs.ingest(inbox).await.unwrap();
    assert_eq!(job_row(&w, job).await.0, "completed");
    type LineFields = (String, Option<String>, Option<String>, Option<bool>, Option<String>);
    type LinePlain = (Option<String>, Option<String>, Option<bool>, Option<String>);
    let rows: Vec<LineFields> = sqlx::query_as(
        "SELECT line, language, translation_status, split_on_speaker_change, speaker FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY id",
    )
    .bind(job.0)
    .fetch_all(&w.owner)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let by_status = |status: &str| rows.iter().find(|r| r.2.as_deref() == Some(status)).unwrap_or_else(|| panic!("no {status} line in {rows:?}")).clone();
    let (original, translation) = (by_status("original"), by_status("translation"));
    assert_eq!((original.1.as_deref(), translation.1.as_deref()), (Some("English"), Some("German")), "the token language, as the app's language name");
    assert_eq!((original.3, translation.3), (Some(true), Some(true)), "TRX-031: a copy of jobs.speakers");
    assert!(original.4.is_some(), "the speaker survives");

    // A plain transcription without speakers: no language, no status, the copy is false.
    let (plain, attempt) = running(&w, &person, Provider::Soniox, false, &[], "tx-plain").await;
    let observation = soniox_observation("tx-plain", Some(vec![token("Just words.", 0.0, 1500.0, None, None, None)]));
    let inbox = deliver(&w, &attempt, &observation).await;
    w.jobs.ingest(inbox).await.unwrap();
    let rows: Vec<LinePlain> = sqlx::query_as(
        "SELECT language, translation_status, split_on_speaker_change, speaker FROM transgate_jobs.lines WHERE job_id = $1",
    )
    .bind(plain.0)
    .fetch_all(&w.owner)
    .await
    .unwrap();
    assert_eq!(rows, vec![(None, None, Some(false), None)], "TRX-029 null for plain transcription; TRX-031 the copy of speakers=false");

    // And the store reads back exactly what it holds.
    let read = w.store.lines(job).await.unwrap();
    assert_eq!(read.iter().map(|l| (l.language.map(|l| l.name()), l.split_on_speaker_change)).collect::<Vec<_>>().len(), 2);
}

// TRX-040: a transcript with no speech fails the job (Soniox with code `no_speech_detected`, Deepgram
// with "No transcription segments found") and writes one outbox event, once, however often the result
// is ingested again.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn trx_040_a_no_speech_result_fails_the_job_once_with_one_outbox_event() {
    let w = world().await;
    let person = account(&w, None).await;
    let failed_events = |job: JobId| {
        let pool = w.owner.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM transgate_jobs.events WHERE event->>'type' = 'failed' AND (event->>'job')::bigint = $1").bind(job.0).fetch_one(&pool).await.unwrap()
        }
    };

    let (soniox, attempt) = running(&w, &person, Provider::Soniox, false, &[], "tx-silent").await;
    let observation = soniox_observation("tx-silent", Some(vec![]));
    let inbox = deliver(&w, &attempt, &observation).await;
    w.jobs.ingest(inbox).await.unwrap();
    w.jobs.ingest(inbox).await.unwrap();
    let (status, error) = job_row(&w, soniox).await;
    assert_eq!(status, "failed");
    let error = error.unwrap();
    assert_eq!((error["message"].as_str(), error["code"].as_str()), (Some("No transcription segments found"), Some("no_speech_detected")), "{error}");
    assert_eq!(failed_events(soniox).await, 1, "one outbox event for the failure");

    let (deepgram, attempt) = running(&w, &person, Provider::Deepgram, false, &[], "dg-silent").await;
    let observation = deepgram_observation("dg-silent", true);
    let inbox = deliver(&w, &attempt, &observation).await;
    w.jobs.ingest(inbox).await.unwrap();
    w.jobs.ingest(inbox).await.unwrap();
    let (status, error) = job_row(&w, deepgram).await;
    assert_eq!(status, "failed");
    assert_eq!(error.unwrap()["message"], "No transcription segments found");
    assert_eq!(failed_events(deepgram).await, 1);
    let lines: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.lines WHERE job_id = ANY($1)").bind(vec![soniox.0, deepgram.0]).fetch_one(&w.owner).await.unwrap();
    assert_eq!(lines, 0);
}

// JOB-029: the Soniox webhook is stored before any work. A transcript fetch that is temporarily
// unavailable leaves the entry pending and is retried by the ingest pass until it works; a fetch that is
// definitely refused fails the job once with `callback_ingestion_error`, and ingesting it again changes nothing.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn job_029_a_transient_fetch_error_is_retried_and_a_deterministic_one_fails_the_job_once() {
    use media_ports::TransportError;
    let w = world().await;
    let person = account(&w, None).await;

    // Transient: unavailable, then available.
    let (job, attempt) = running(&w, &person, Provider::Soniox, false, &[], "tx-flaky").await;
    let webhook = soniox_observation("tx-flaky", None);
    let inbox = deliver(&w, &attempt, &webhook).await;
    w.soniox.answer(Err(TransportError::Unavailable { status: Some(503), operation: "transcript".into() }));
    w.jobs.ingest(inbox).await.unwrap();
    let (state, failures): (String, i32) = sqlx::query_as("SELECT state, failures FROM transgate_jobs.inbox WHERE id = $1").bind(inbox.0).fetch_one(&w.owner).await.unwrap();
    assert_eq!((state.as_str(), failures), ("pending", 1), "the entry is kept and the failure counted");
    assert_eq!(job_row(&w, job).await.0, "running", "a transient error does not fail the job");
    w.soniox.answer(Ok(soniox_observation("tx-flaky", Some(vec![token("It works now.", 0.0, 1500.0, None, None, None)]))));
    let report = w.jobs.ingest_pass().await.unwrap();
    assert_eq!(report.errors, 0, "{report:?}");
    assert_eq!(job_row(&w, job).await.0, "completed", "the retry ingested the result");

    // Deterministic: the fetch is refused for good.
    let (job, attempt) = running(&w, &person, Provider::Soniox, false, &[], "tx-gone").await;
    let inbox = deliver(&w, &attempt, &soniox_observation("tx-gone", None)).await;
    w.soniox.answer(Err(TransportError::Rejected { status: Some(404), code: "not_found".into() }));
    w.jobs.ingest(inbox).await.unwrap();
    let (status, error) = job_row(&w, job).await;
    assert_eq!(status, "failed");
    assert_eq!(error.as_ref().unwrap()["code"], "callback_ingestion_error", "{error:?}");
    let events = |job: JobId| {
        let pool = w.owner.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT count(*) FROM transgate_jobs.events WHERE event->>'type' = 'failed' AND (event->>'job')::bigint = $1").bind(job.0).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!(events(job).await, 1);
    w.jobs.ingest(inbox).await.unwrap();
    w.jobs.ingest_pass().await.unwrap();
    assert_eq!(events(job).await, 1, "failed once, never again");
    let state: String = sqlx::query_scalar("SELECT state FROM transgate_jobs.inbox WHERE id = $1").bind(inbox.0).fetch_one(&w.owner).await.unwrap();
    assert_eq!(state, "applied");
}

// EDT-002, EDT-003: a line edit writes only that line's text and never creates a row, so racing the
// retention sweep or an erasure it can neither resurrect a deleted line nor lose a line that stays: after
// every round the swept job has no lines, the kept job holds the edited text with `corrected_text`
// untouched, and an erased account holds nothing.
#[tokio::test]
#[ignore = "needs TRANSGATE_JOBS_TEST_DATABASE_URL"]
async fn edt_002_003_a_line_edit_racing_the_sweep_or_an_erasure_never_resurrects_or_loses_a_line() {
    let w = world().await;
    let person = account(&w, Some((true, false))).await;
    for round in 0..6 {
        let old = job(&w, &person, JobStatus::Completed, 40, round, 3).await;
        let kept = job(&w, &person, JobStatus::Completed, 1, round, 3).await;
        let first_line = |job: JobId| {
            let pool = w.owner.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT id FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY id LIMIT 1").bind(job.0).fetch_one(&pool).await.unwrap() }
        };
        let (old_line, kept_line) = (first_line(old).await, first_line(kept).await);
        let text = format!("edited in round {round}");
        let (sweep, edit_old, edit_kept) = tokio::join!(
            w.privacy.retention_run(thirty(), "test"),
            w.jobs.edit(person.account, LineEdit::Text { line: LineId(old_line), text: text.clone() }),
            w.jobs.edit(person.account, LineEdit::Text { line: LineId(kept_line), text: text.clone() }),
        );
        sweep.unwrap();
        edit_kept.unwrap_or_else(|e| panic!("round {round}: the edit of a line that stays failed: {e:?}"));
        let _ = edit_old; // either applied before the delete or found nothing; both are fine
        assert_eq!(line_count(&w, old).await, 0, "round {round}: a swept line is never resurrected");
        let (line, corrected): (String, Option<String>) =
            sqlx::query_as("SELECT line, corrected_text FROM transgate_jobs.lines WHERE id = $1").bind(kept_line).fetch_one(&w.owner).await.unwrap();
        assert_eq!((line.as_str(), corrected), (text.as_str(), None), "round {round}: the edit is kept and corrected_text is untouched");
        assert_eq!(line_count(&w, kept).await, 3, "round {round}: no line of the kept job is lost or duplicated");
    }

    // Erasure: the edit and the request race; afterwards the account holds no lines and the edit never recreates one.
    let erased = account(&w, None).await;
    let target = job(&w, &erased, JobStatus::Completed, 1, 0, 3).await;
    let line_id: i64 = sqlx::query_scalar("SELECT id FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY id LIMIT 1").bind(target.0).fetch_one(&w.owner).await.unwrap();
    let (request, edit) = tokio::join!(
        w.privacy.request_erasure(erased.account),
        w.jobs.edit(erased.account, LineEdit::Text { line: LineId(line_id), text: "late".into() }),
    );
    request.unwrap();
    let _ = edit;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.lines WHERE account_id = $1").bind(erased.account.0).fetch_one(&w.owner).await.unwrap();
    assert_eq!(left, 0, "an erased account holds no lines");
    let after = w.jobs.edit(erased.account, LineEdit::Text { line: LineId(line_id), text: "after".into() }).await;
    assert!(after.is_err(), "an edit after the erasure opened is refused: {after:?}");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.lines WHERE account_id = $1").bind(erased.account.0).fetch_one(&w.owner).await.unwrap();
    assert_eq!(left, 0, "and creates nothing");
}
