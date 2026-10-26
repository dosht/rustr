//! Advisory locks and schedule slots against a real database, through a
//! restricted runtime login (never the owner).
//!
//! Needs `MEDIA_TEST_DATABASE_URL` naming a disposable PostgreSQL database
//! whose user may create roles. `#[ignore]`d for offline runs; with
//! `--include-ignored` a missing variable fails, it never skips.

use std::str::FromStr;
use std::sync::Arc;

use media_postgres::{
    LockKey, claim_slot, count_finished_slots, delete_finished_slots, finish_slot, grant_runtime, migrate, renew_slot,
    runtime_authority_problems, slot_state, try_lock,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};
use tokio::sync::{Barrier, OnceCell};
use uuid::Uuid;

const LOGIN: &str = "media_test_runtime";
const PASSWORD: &str = "media-test-runtime-password";
const T0: i64 = 1_768_478_400_000_000; // 2026-01-15T12:00:00Z
const SECOND: i64 = 1_000_000;

fn url() -> String {
    std::env::var("MEDIA_TEST_DATABASE_URL").expect("MEDIA_TEST_DATABASE_URL must name a disposable PostgreSQL database")
}

static READY: OnceCell<()> = OnceCell::const_new();

async fn owner() -> PgPool {
    let owner = PgPoolOptions::new().max_connections(2).connect(&url()).await.expect("owner connection");
    READY
        .get_or_init(|| async {
            migrate(&owner).await.expect("media migration");
            let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
                .bind(LOGIN)
                .fetch_one(&owner)
                .await
                .unwrap();
            if !exists {
                sqlx::query(AssertSqlSafe(format!("CREATE ROLE {LOGIN} LOGIN PASSWORD '{PASSWORD}'")))
                    .execute(&owner)
                    .await
                    .unwrap();
            }
            grant_runtime(&owner, LOGIN).await.expect("runtime grants");
        })
        .await;
    owner
}

async fn runtime() -> PgPool {
    owner().await.close().await;
    let options = PgConnectOptions::from_str(&url()).unwrap().username(LOGIN).password(PASSWORD);
    PgPoolOptions::new().max_connections(6).connect_with(options).await.expect("runtime connection")
}

fn schedule() -> String {
    format!("test-{}", Uuid::new_v4().simple())
}

#[tokio::test]
#[ignore = "needs MEDIA_TEST_DATABASE_URL"]
async fn the_runtime_login_can_claim_and_purge_only_finished_runs_but_not_rewrite_identity_or_create() {
    let owner = owner().await;
    assert_eq!(runtime_authority_problems(&owner, LOGIN).await.unwrap(), Vec::<String>::new());
    let pool = runtime().await;
    let name = schedule();
    assert!(claim_slot(&pool, &name, T0, T0, 60).await.unwrap().is_some());
    let deleted = sqlx::query("DELETE FROM media.schedule_slots WHERE schedule = $1").bind(&name).execute(&pool).await.unwrap();
    assert_eq!(deleted.rows_affected(), 0, "a running claim is invisible to the runtime's DELETE");
    assert!(slot_state(&pool, &name, T0).await.unwrap().is_some());
    for forbidden in [
        "TRUNCATE media.schedule_slots",
        "UPDATE media.schedule_slots SET slot_at = now()",
        "UPDATE media.schedule_slots SET schedule = 'other'",
        "CREATE TABLE media.made_by_runtime (id int)",
    ] {
        let error = sqlx::query(forbidden).execute(&pool).await.unwrap_err();
        let code = error.as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default();
        assert_eq!(code, "42501", "{forbidden}: {error}");
    }
    // An over-privileged role is reported.
    let dirty = format!("media_dirty_{}", &Uuid::new_v4().simple().to_string()[..8]);
    for statement in [
        format!("CREATE ROLE {dirty} NOLOGIN"),
        format!("GRANT TRUNCATE ON media.schedule_slots TO {dirty}"),
        format!("GRANT UPDATE (slot_at) ON media.schedule_slots TO {dirty}"),
    ] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    let problems = runtime_authority_problems(&owner, &dirty).await.unwrap();
    for statement in [format!("DROP OWNED BY {dirty}"), format!("DROP ROLE {dirty}")] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    assert!(problems.iter().any(|p| p == "can TRUNCATE media.schedule_slots"), "{problems:?}");
    assert!(problems.iter().any(|p| p == "can UPDATE media.schedule_slots.slot_at"), "{problems:?}");
}

#[tokio::test]
#[ignore = "needs MEDIA_TEST_DATABASE_URL"]
async fn one_of_many_concurrent_claimants_wins_a_slot_and_a_finished_slot_is_never_claimed_again() {
    let pool = runtime().await;
    let name = Arc::new(schedule());
    let barrier = Arc::new(Barrier::new(6));
    let mut handles = Vec::new();
    for _ in 0..6 {
        let (pool, name, barrier) = (pool.clone(), name.clone(), barrier.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            claim_slot(&pool, &name, T0, T0, 60).await.unwrap()
        }));
    }
    let mut winners = Vec::new();
    for handle in handles {
        if let Some(claim) = handle.await.unwrap() {
            winners.push(claim);
        }
    }
    assert_eq!(winners.len(), 1, "exactly one run owns the slot");
    let claim = winners.pop().unwrap();
    assert_eq!(claim.attempt, 1);
    assert!(finish_slot(&pool, &claim, T0 + 5 * SECOND, "succeeded").await.unwrap());
    // Long after any lease, a finished slot stays finished.
    assert!(claim_slot(&pool, &name, T0, T0 + 86_400 * SECOND, 60).await.unwrap().is_none());
    let state = slot_state(&pool, &name, T0).await.unwrap().unwrap();
    assert_eq!((state.attempts, state.outcome.as_deref(), state.finished_at_us), (1, Some("succeeded"), Some(T0 + 5 * SECOND)));
    // The next slot of the same schedule is its own run.
    assert!(claim_slot(&pool, &name, T0 + 86_400 * SECOND, T0 + 86_400 * SECOND, 60).await.unwrap().is_some());
}

#[tokio::test]
#[ignore = "needs MEDIA_TEST_DATABASE_URL"]
async fn an_interrupted_run_is_reclaimed_after_its_lease_and_the_stale_run_can_neither_renew_nor_finish() {
    let pool = runtime().await;
    let name = schedule();
    let first = claim_slot(&pool, &name, T0, T0, 60).await.unwrap().expect("first run");
    // While the lease is active nobody else gets it, and the holder can renew.
    assert!(claim_slot(&pool, &name, T0, T0 + 30 * SECOND, 60).await.unwrap().is_none());
    assert!(renew_slot(&pool, &first, T0 + 30 * SECOND, 60).await.unwrap(), "renewed until T0 + 90 s");
    assert!(claim_slot(&pool, &name, T0, T0 + 80 * SECOND, 60).await.unwrap().is_none());
    // The first run stops without finishing; after its lease a second run takes over.
    let second = claim_slot(&pool, &name, T0, T0 + 91 * SECOND, 60).await.unwrap().expect("reclaimed after the lease");
    assert_eq!(second.attempt, 2);
    assert_ne!(second.token, first.token);
    assert!(!renew_slot(&pool, &first, T0 + 92 * SECOND, 60).await.unwrap(), "the stale run cannot extend");
    assert!(!finish_slot(&pool, &first, T0 + 92 * SECOND, "succeeded").await.unwrap(), "the stale run cannot finish");
    assert!(finish_slot(&pool, &second, T0 + 95 * SECOND, "failed").await.unwrap());
    let state = slot_state(&pool, &name, T0).await.unwrap().unwrap();
    assert_eq!((state.attempts, state.outcome.as_deref()), (2, Some("failed")));
    // An expired holder cannot renew even when nobody reclaimed the slot yet.
    let other = schedule();
    let lone = claim_slot(&pool, &other, T0, T0, 10).await.unwrap().unwrap();
    assert!(!renew_slot(&pool, &lone, T0 + 11 * SECOND, 10).await.unwrap());
    // Arguments are checked.
    assert!(claim_slot(&pool, "", T0, T0, 10).await.is_err());
    assert!(claim_slot(&pool, &other, T0, T0, 0).await.is_err());
    assert!(finish_slot(&pool, &lone, T0, "").await.is_err());
}

#[tokio::test]
#[ignore = "needs MEDIA_TEST_DATABASE_URL"]
async fn an_advisory_lock_has_one_holder_and_is_freed_by_release_or_by_the_holders_connection_ending() {
    let pool = runtime().await;
    let key = LockKey::named(&format!("leader-{}", Uuid::new_v4()));
    assert_eq!(LockKey::named("retention"), LockKey::named("retention"), "derivation is deterministic");
    assert_ne!(LockKey::named("retention"), LockKey::named("dispatch"));
    let mut held = try_lock(&pool, key).await.unwrap().expect("first holder");
    assert!(held.still_held().await);
    assert!(try_lock(&pool, key).await.unwrap().is_none(), "a second session is refused");
    held.release().await.unwrap();
    let held = try_lock(&pool, key).await.unwrap().expect("free after release");
    // A holder that disappears without releasing (a crash) frees the lock with its connection.
    drop(held);
    let mut retaken = None;
    for _ in 0..50 {
        retaken = try_lock(&pool, key).await.unwrap();
        if retaken.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(retaken.is_some(), "the server released the lock when the holder's connection closed");
}

#[tokio::test]
#[ignore = "needs MEDIA_TEST_DATABASE_URL"]
async fn the_history_purge_removes_only_finished_runs_of_the_named_schedules_started_at_or_before_the_cutoff() {
    let pool = runtime().await;
    let (named, also_named, other) = (schedule(), schedule(), schedule());
    let day = 86_400 * SECOND;
    let cutoff = T0;
    let finished = |name: String, slot: i64, started: i64| {
        let pool = pool.clone();
        async move {
            let claim = claim_slot(&pool, &name, slot, started, 60).await.unwrap().unwrap();
            assert!(finish_slot(&pool, &claim, started + SECOND, "succeeded").await.unwrap());
        }
    };
    finished(named.clone(), T0 - 10 * day, T0 - 10 * day).await; // old and finished: purged
    finished(also_named.clone(), T0, cutoff).await; // started exactly at the cutoff: purged
    finished(named.clone(), T0 + day, T0 + day).await; // started after the cutoff: kept
    finished(other.clone(), T0 - 10 * day, T0 - 10 * day).await; // not a named schedule: kept
    // Old but unfinished: a running claim and an interrupted one whose lease expired.
    claim_slot(&pool, &named, T0 - 3 * day, T0 - 3 * day, 10 * 86_400).await.unwrap().unwrap();
    claim_slot(&pool, &named, T0 - 2 * day, T0 - 2 * day, 60).await.unwrap().unwrap();
    let scope = [named.as_str(), also_named.as_str()];

    let mut tx = pool.begin().await.unwrap();
    assert_eq!(count_finished_slots(&mut tx, &scope, cutoff).await.unwrap(), 2);
    assert_eq!(delete_finished_slots(&mut tx, &scope, cutoff).await.unwrap(), 2);
    tx.rollback().await.unwrap();
    assert!(slot_state(&pool, &named, T0 - 10 * day).await.unwrap().is_some(), "a rolled-back purge deletes nothing");

    let mut tx = pool.begin().await.unwrap();
    assert_eq!(delete_finished_slots(&mut tx, &scope, cutoff).await.unwrap(), 2);
    assert_eq!(count_finished_slots(&mut tx, &scope, cutoff).await.unwrap(), 0);
    tx.commit().await.unwrap();
    assert!(slot_state(&pool, &named, T0 - 10 * day).await.unwrap().is_none());
    assert!(slot_state(&pool, &also_named, T0).await.unwrap().is_none());
    for (name, slot) in [(&named, T0 + day), (&other, T0 - 10 * day), (&named, T0 - 3 * day), (&named, T0 - 2 * day)] {
        assert!(slot_state(&pool, name, slot).await.unwrap().is_some(), "{name} at {slot} is kept");
    }
    let mut tx = pool.begin().await.unwrap();
    assert!(count_finished_slots(&mut tx, &[], cutoff).await.is_err(), "the caller must name its schedules");
    tx.rollback().await.unwrap();
}
