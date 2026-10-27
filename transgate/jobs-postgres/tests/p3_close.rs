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
    _database: Database,
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
    let jobs = Arc::new(Jobs::new(
        JobsServices {
            store: Arc::new(store.clone()),
            clock,
            objects: vec![],
            transcribers: vec![],
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
    World { owner, store, privacy, locks, jobs, _database: database }
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
    let healthy_status: String = sqlx::query_scalar("SELECT status FROM transgate_jobs.jobs WHERE id = $1").bind(healthy.0).fetch_one(&w.owner).await.unwrap();
    assert_ne!(healthy_status, "failed", "only the unmapped job fails: {healthy_status}");
}
