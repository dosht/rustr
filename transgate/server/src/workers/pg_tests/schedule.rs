//! The daily 02:00 UTC transcript cleanup and the Sunday 04:00 UTC history cleanup through the
//! real worker, a real database and a controlled clock. The clock is the test's, so no test waits.

use super::*;
use transgate_core::schedule::Schedule;
use transgate_core::transcript::NewLine;

/// 2026-03-04 (a Wednesday) 00:00 UTC, in seconds, and 2026-03-08 (the Sunday after).
const WEDNESDAY: i64 = 1_772_582_400;
const SUNDAY: i64 = 1_772_928_000;
const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

fn at(seconds: i64) -> Micros {
    Micros(seconds * SECOND)
}

async fn account_with_job(store: &PgJobStore, created: Micros) -> (AccountId, transgate_core::JobId) {
    let account = AccountId(Uuid::new_v4());
    let mut uow = store.begin().await.unwrap();
    uow.billing().await.unwrap().create_account(account, created).await.unwrap();
    let job = uow
        .insert_job(&NewJob {
            account,
            status: JobStatus::Completed,
            name: Some("old.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: Language::from_code("en"),
            translate_to: vec![],
            speakers: false,
            fix_grammer: None,
            duration_minutes: Some(2),
            size_bytes: Some(1),
            callback_url: None,
            whisper_params: None,
            created_at: created,
        })
        .await
        .unwrap();
    let lines: Vec<NewLine> = (0..3)
        .map(|i| NewLine {
            start_time: format!("00:00:0{i}"),
            end_time: format!("00:00:0{i}"),
            line: "words".into(),
            speaker: None,
            corrected_text: None,
            split_on_speaker_change: None,
            language: None,
            translation_status: None,
        })
        .collect();
    uow.insert_lines(&job, &lines).await.unwrap();
    uow.commit().await.unwrap();
    (account, job.id)
}

async fn lines(pool: &PgPool, job: transgate_core::JobId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.lines WHERE job_id = $1").bind(job.0).fetch_one(pool).await.unwrap()
}

async fn runs(pool: &PgPool, job_name: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.retention_runs WHERE job_name = $1 AND executed_by = 'scheduler' AND status = 'success'")
        .bind(job_name)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn workers_at(pool: &PgPool, clock: &Arc<Injected>) -> Arc<Workers> {
    replica(pool, clock.clone(), Arc::new(HeldListing::new(Arc::new(Notify::new()), Arc::new(Notify::new()))))
}

// PRV-029, CRN-005: the transcript cleanup runs once per UTC day at 02:00 with 30 days (a job 31 days old
// loses its lines, a job 10 days old keeps them), not again inside the same slot, and a claim left by a
// process that died is taken over by the next one only after its lease runs out.
#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn prv_029_crn_005_the_daily_0200_sweep_runs_once_per_slot_and_resumes_after_a_restart() {
    let (pool, _database) = isolated().await;
    let store = PgJobStore::new(pool.clone());
    let name = Schedule::TranscriptRetention.name();
    let clock = Arc::new(Injected(AtomicI64::new((WEDNESDAY + 2 * HOUR) * SECOND)));
    let (_, old) = account_with_job(&store, at(WEDNESDAY + 2 * HOUR - 40 * DAY)).await;
    let (_, recent) = account_with_job(&store, at(WEDNESDAY + 2 * HOUR - 10 * DAY)).await;
    let first = workers_at(&pool, &clock);

    // 02:00:00 exactly: the slot is due and runs with 30 days.
    assert!(first.run_schedules(&[Schedule::TranscriptRetention]).await.is_empty());
    assert_eq!((lines(&pool, old).await, lines(&pool, recent).await), (0, 3), "31+ days old is swept, 10 days old is kept");
    assert_eq!(runs(&pool, name).await, 1);
    let days: i32 = sqlx::query_scalar("SELECT retention_days FROM transgate_jobs.retention_runs WHERE job_name = $1 AND status = 'success'")
        .bind(name)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(days, 30);

    // The same slot never runs twice, however many ticks or replicas; the next day's 02:00 does.
    clock.0.store((WEDNESDAY + 2 * HOUR + 60) * SECOND, Ordering::SeqCst);
    assert!(workers_at(&pool, &clock).run_schedules(&[Schedule::TranscriptRetention]).await.is_empty());
    clock.0.store((WEDNESDAY + DAY + 2 * HOUR - 1) * SECOND, Ordering::SeqCst);
    first.run_schedules(&[Schedule::TranscriptRetention]).await;
    assert_eq!(runs(&pool, name).await, 1, "01:59:59 the next day is still the previous slot");
    clock.0.store((WEDNESDAY + DAY + 2 * HOUR) * SECOND, Ordering::SeqCst);
    first.run_schedules(&[Schedule::TranscriptRetention]).await;
    assert_eq!(runs(&pool, name).await, 2, "02:00 the next day is a new slot");

    // A process claimed the next slot and died before finishing. Another process waits out the lease.
    let slot = Schedule::TranscriptRetention.slot(at(WEDNESDAY + 2 * DAY + 2 * HOUR));
    assert_eq!(slot, at(WEDNESDAY + 2 * DAY + 2 * HOUR));
    media_postgres::claim_slot(&pool, name, slot.0, slot.0, 600).await.unwrap().expect("the dead process's claim");
    let (_, due_later) = account_with_job(&store, at(WEDNESDAY + 2 * DAY + 2 * HOUR - 40 * DAY)).await;
    let restarted = workers_at(&pool, &clock);
    clock.0.store(slot.0 + 599 * SECOND, Ordering::SeqCst);
    restarted.run_schedules(&[Schedule::TranscriptRetention]).await;
    assert_eq!((runs(&pool, name).await, lines(&pool, due_later).await), (2, 3), "inside the lease nothing runs");
    clock.0.store(slot.0 + 601 * SECOND, Ordering::SeqCst);
    assert!(restarted.run_schedules(&[Schedule::TranscriptRetention]).await.is_empty());
    assert_eq!((runs(&pool, name).await, lines(&pool, due_later).await), (3, 0), "the expired claim is taken over and the sweep resumes");
    let state = media_postgres::slot_state(&pool, name, slot.0).await.unwrap().expect("the slot");
    assert_eq!(state.outcome.as_deref(), Some("succeeded"));
}

// CRN-007: the Sunday 04:00 UTC cleanup keeps 90 days of the scheduler's own history: rows older than
// `now - 90 - 1 day` go, newer ones stay, and the slot belongs to its Sunday only.
#[tokio::test]
#[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
async fn crn_007_the_sunday_0400_sweep_keeps_ninety_days_of_scheduler_history() {
    let (pool, _database) = isolated().await;
    let name = Schedule::HistoryRetention.name();
    let sunday = SUNDAY + 4 * HOUR;
    let clock = Arc::new(Injected(AtomicI64::new((sunday - 1) * SECOND)));
    // Finished runs of another schedule, of several ages; the sweep deletes by age.
    for (label, age) in [("120 days", 120 * DAY), ("92 days", 92 * DAY), ("89 days", 89 * DAY), ("10 days", 10 * DAY)] {
        let slot = (sunday - age) * SECOND;
        let claim = media_postgres::claim_slot(&pool, "jobs", slot, slot, 60).await.unwrap().unwrap_or_else(|| panic!("{label}"));
        assert!(media_postgres::finish_slot(&pool, &claim, slot + SECOND, "succeeded").await.unwrap());
    }
    let present = |age: i64| {
        let pool = pool.clone();
        async move { media_postgres::slot_state(&pool, "jobs", (sunday - age) * SECOND).await.unwrap().is_some() }
    };
    let workers = workers_at(&pool, &clock);

    // 03:59:59 Sunday still belongs to the previous Sunday's slot, which a fresh database has not run:
    // it runs now with the clock's `now`, so seed-independent checks use only the next slot below.
    workers.run_schedules(&[Schedule::HistoryRetention]).await;
    let first_runs = runs(&pool, Schedule::HistoryRetention.name()).await;
    assert_eq!(first_runs, 1, "a never-run slot is taken at once");
    clock.0.store((sunday + 60) * SECOND, Ordering::SeqCst);
    workers.run_schedules(&[Schedule::HistoryRetention]).await;
    assert_eq!(runs(&pool, name).await, 2, "04:00 Sunday is a new slot");
    assert_eq!((present(120 * DAY).await, present(92 * DAY).await), (false, false), "older than 91 days goes");
    assert_eq!((present(89 * DAY).await, present(10 * DAY).await), (true, true), "inside 90 days stays");
    let days: i32 = sqlx::query_scalar("SELECT retention_days FROM transgate_jobs.retention_runs WHERE job_name = $1 AND status = 'success' LIMIT 1")
        .bind(name)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(days, 90);
}
