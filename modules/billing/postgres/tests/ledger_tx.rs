//! Ledger behavior against a real database: serialization, atomic host
//! transactions, rollback and isolation. Names start with the rule id they prove.
//!
//! Run with `BILLING_TEST_DATABASE_URL=... cargo test -p billing-postgres -- --include-ignored`.

mod support;

use std::sync::Arc;
use std::time::Duration;

use billing_core::app::{ChargeError, charge_job_starts, charge_strict, release_subject_charges, start_answer};
use billing_core::presentation::StartAnswerMeta;
use billing_core::calendar::{add_months, utc};
use billing_core::charge::{JobStart, StartCharge, StartRefusal, is_billable_transition};
use billing_core::ledger::{Allocation, balance, open_at};
use billing_core::model::{NewPackage, NewUsage};
use billing_core::ports::{BillingStore, BillingTx, StoreError};
use billing_core::types::{AccountId, Micros};
use billing_postgres::{PgBillingTx, owner_anomalies};
use uuid::Uuid;
use sqlx::{PgPool, Row};
use support::*;
use tokio::sync::{Barrier, oneshot};

const NOW: Micros = Micros(1_768_478_400_000_000); // 2026-01-15T12:00:00Z

/// A paid 100 minute package valid around NOW, written through the runtime role.
async fn give_package(pool: &PgPool, account: AccountId, name: &str, minutes: i64, ends_months_after_now: i64) -> i64 {
    let mut tx = pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.lock_account(account).await.unwrap();
    let id = billing
        .insert_package(&NewPackage {
            account,
            payment_id: None,
            payment_seq: None,
            name: name.into(),
            minutes,
            start_at: Micros(NOW.0 - 86_400_000_000),
            end_at: add_months(NOW, ends_months_after_now).unwrap(),
            created: NOW,
            signup_grant: false,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn new_account(f: &Fixture) -> AccountId {
    let account = provisioned(f, &unique_email()).await;
    f.clock.set(NOW);
    account
}

async fn open_balance(pool: &PgPool, account: AccountId, at: Micros) -> Option<i64> {
    let mut tx = pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let all = billing.packages(account).await.unwrap();
    balance(&open_at(&all, at))
}

async fn usage_rows(pool: &PgPool, account: AccountId) -> i64 {
    count(pool, "SELECT count(*) FROM billing.usage WHERE account_id = $1", account).await
}

async fn usage_minutes(pool: &PgPool, account: AccountId) -> i64 {
    count(pool, "SELECT COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1", account).await
}

/// The minimal host: it locks the account first, then its own job rows in
/// ascending id, decides eligibility from its own tables, charges through the
/// billing seam and moves job status in the same transaction.
async fn host_start_in(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    account: AccountId,
    ids: &[i64],
    at: Micros,
) -> Result<Vec<(i64, StartCharge)>, ChargeError> {
    let unavailable = |e: sqlx::Error| ChargeError::Store(StoreError::Unavailable(e.to_string()));
    {
        let mut billing = PgBillingTx::attach(tx).await?;
        billing.lock_account(account).await?;
    }
    let rows = sqlx::query(
        "SELECT id, duration, status FROM billing_test_host.jobs
         WHERE account_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE",
    )
    .bind(account.0)
    .bind(ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(unavailable)?;
    let mut jobs = Vec::new();
    let mut moving = Vec::new();
    for row in &rows {
        let id: i64 = row.get("id");
        let status: String = row.get("status");
        if !is_billable_transition(&status, "not_started") {
            continue;
        }
        let duration: Option<i32> = row.get("duration");
        let completed: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM billing_test_host.jobs WHERE account_id = $1 AND status = 'completed' AND id <> $2",
        )
        .bind(account.0)
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(unavailable)?;
        jobs.push(JobStart { subject: id, duration: duration.map(i64::from), other_completed_jobs: completed as u64 });
        moving.push((id, status));
    }
    let plan = {
        let mut billing = PgBillingTx::attach(tx).await?;
        charge_job_starts(&mut billing, account, &jobs, at).await?
    };
    for (id, from) in moving {
        let moved = sqlx::query("UPDATE billing_test_host.jobs SET status = 'not_started' WHERE id = $1 AND status = $2")
            .bind(id)
            .bind(from)
            .execute(&mut **tx)
            .await
            .map_err(unavailable)?;
        assert_eq!(moved.rows_affected(), 1, "the host compare-and-swap protects one start");
    }
    Ok(plan)
}

async fn host_commit_start(
    pool: &PgPool,
    account: AccountId,
    ids: &[i64],
    at: Micros,
) -> Result<Vec<(i64, StartCharge)>, ChargeError> {
    let mut tx = pool.begin().await.map_err(|e| ChargeError::Store(StoreError::Unavailable(e.to_string())))?;
    let plan = host_start_in(&mut tx, account, ids, at).await?;
    tx.commit().await.map_err(|e| ChargeError::Store(StoreError::Unavailable(e.to_string())))?;
    Ok(plan)
}

async fn add_job(pool: &PgPool, account: AccountId, id: i64, status: &str, duration: Option<i32>) {
    sqlx::query("INSERT INTO billing_test_host.jobs (id, account_id, status, duration) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(account.0)
        .bind(status)
        .bind(duration)
        .execute(pool)
        .await
        .unwrap();
}

async fn job_status(pool: &PgPool, id: i64) -> String {
    sqlx::query_scalar("SELECT status FROM billing_test_host.jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_001_002_003_a_charge_writes_one_usage_row_per_package_and_remaining_is_derived() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    let first = give_package(&f.pool, account, "PREMIUM", 30, 1).await;
    let second = give_package(&f.pool, account, "PREMIUM", 50, 3).await;
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let taken = charge_strict(&mut billing, account, 4242, 70, NOW).await.unwrap();
    tx.commit().await.unwrap();
    // The free package (ends in two months) sits between the two purchases by expiry.
    assert_eq!(taken.len(), 3);
    assert_eq!(taken.first().map(|a| a.package_id), Some(first));
    assert_eq!(taken.last().map(|a| a.package_id), Some(second));
    assert_eq!(taken.iter().map(|a| a.minutes).sum::<i64>(), 70);
    assert_eq!(usage_rows(&f.pool, account).await, 3, "a deduction drawing on three packages writes three rows");
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let all = billing.packages(account).await.unwrap();
    let mut remaining: Vec<i64> = all.iter().map(|p| p.remaining()).collect();
    remaining.sort();
    assert_eq!(remaining, vec![0, 0, 30], "remaining is included minus the usage rows that reference each package");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_004_to_008_stored_windows_read_back_exactly_in_a_non_utc_session() {
    let f = fixture(vec![]).await;
    let account = account();
    let signup = utc(2026, 1, 31, 23, 59, 59, 999_999);
    f.clock.set(signup);
    f.directory.add(customer(account, &unique_email(), signup));
    f.billing.provision(customer(account, &unique_email(), signup)).await.unwrap();

    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL TIME ZONE 'Asia/Istanbul'").execute(&mut *tx).await.unwrap();
    let zone: String = sqlx::query_scalar("SHOW TIME ZONE").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(zone, "Asia/Istanbul", "the session really is not UTC");
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let all = billing.packages(account).await.unwrap();
    assert_eq!(all.len(), 1);
    let package = &all[0].package;
    assert_eq!(package.start_at, signup, "microseconds round-trip");
    // January 31 plus two calendar months, clamped nowhere: March 31.
    assert_eq!(package.end_at, utc(2026, 3, 31, 23, 59, 59, 999_999));
    // BIL-005: both ends inclusive, to the microsecond.
    assert_eq!(balance(&open_at(&all, package.start_at)), Some(20));
    assert_eq!(balance(&open_at(&all, package.end_at)), Some(20));
    assert_eq!(balance(&open_at(&all, Micros(package.end_at.0 + 1))), None, "BIL-006 expired adds nothing, BIL-008 no row");
    assert_eq!(balance(&open_at(&all, Micros(package.start_at.0 - 1))), None);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_009_024_a_first_job_overdraft_is_persisted_as_one_full_row_and_zero_minutes_is_a_real_write() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    let base = job_base();
    add_job(&f.pool, account, base + 1, "draft", Some(36)).await;
    add_job(&f.pool, account, base + 2, "draft", Some(0)).await;
    let plan = host_commit_start(&f.pool, account, &[base + 1], NOW).await.unwrap();
    assert!(matches!(plan[0].1, StartCharge::Bypass(_)));
    assert_eq!(usage_rows(&f.pool, account).await, 1, "one ledger row for the full duration");
    assert_eq!(usage_minutes(&f.pool, account).await, 36);
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(-16), "BIL-009: the balance may go below zero");
    // A zero-minute bypass is preserved as a zero row.
    let other = new_account(&f).await;
    add_job(&f.pool, other, base + 3, "draft", Some(0)).await;
    host_commit_start(&f.pool, other, &[base + 3], NOW).await.unwrap();
    assert_eq!(usage_rows(&f.pool, other).await, 1);
    assert_eq!(usage_minutes(&f.pool, other).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_020_021_022_strict_refusal_writes_nothing_and_exact_fit_leaves_exactly_zero() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await; // balance 100 with the free 20
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let refused = charge_strict(&mut billing, account, 1, 101, NOW).await.unwrap_err();
    assert_eq!(refused, ChargeError::Refused(StartRefusal::NotEnoughBalance { required: 101, available: 100 }));
    tx.rollback().await.unwrap();
    assert_eq!(usage_rows(&f.pool, account).await, 0);

    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    charge_strict(&mut billing, account, 2, 100, NOW).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(0), "BIL-021: exactly zero");

    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let nothing = charge_strict(&mut billing, account, 3, 0, NOW).await.unwrap_err();
    assert_eq!(nothing, ChargeError::Refused(StartRefusal::NothingDeducted));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d1_the_persisted_balance_matches_the_outcome_after_an_overdraft() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    let base = job_base();
    // Free package overdrawn to -16 by the first-job bypass, then a paid one of 100.
    add_job(&f.pool, account, base + 1, "draft", Some(36)).await;
    host_commit_start(&f.pool, account, &[base + 1], NOW).await.unwrap();
    let paid = give_package(&f.pool, account, "PREMIUM", 100, 6).await;
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(84));
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let taken = charge_strict(&mut billing, account, 9, 10, NOW).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!((taken[0].package_id, taken[0].minutes), (paid, 10), "exactly the request, not 26");
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(74));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_001_two_concurrent_paid_debits_serialize_on_the_account_lock_and_never_overdraw() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await; // balance 100
    let owner = owner().await;

    let (charged_tx, charged_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let pool_a = f.pool.clone();
    let first = tokio::spawn(async move {
        let mut tx = pool_a.begin().await.unwrap();
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(account).await.unwrap();
        charge_strict(&mut billing, account, 11, 60, NOW).await.unwrap();
        charged_tx.send(()).unwrap();
        release_rx.await.unwrap(); // hold the account lock while the other debit arrives
        tx.commit().await.unwrap();
    });
    charged_rx.await.unwrap();

    let pool_b = f.pool.clone();
    let second = tokio::spawn(async move {
        let mut tx = pool_b.begin().await.unwrap();
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        let result = charge_strict(&mut billing, account, 12, 60, NOW).await;
        tx.rollback().await.unwrap();
        result
    });
    // The second debit must really be waiting on the account row, not merely late.
    let mut waiting = 0i64;
    for _ in 0..100 {
        waiting = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE wait_event_type = 'Lock' AND query ILIKE '%billing.accounts%FOR UPDATE%'",
        )
        .fetch_one(&owner)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(waiting > 0, "the second debit waits on the account lock held by the first");
    release_tx.send(()).unwrap();
    first.await.unwrap();
    let refused = second.await.unwrap().unwrap_err();
    assert_eq!(refused, ChargeError::Refused(StartRefusal::NotEnoughBalance { required: 60, available: 40 }));
    assert_eq!(usage_minutes(&f.pool, account).await, 60);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_001_repeated_races_never_spend_more_than_the_balance() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await; // balance 100
    for round in 0..6 {
        let barrier = Arc::new(Barrier::new(4));
        let mut handles = Vec::new();
        for n in 0..4 {
            let pool = f.pool.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                let mut tx = pool.begin().await.unwrap();
                let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
                let result = charge_strict(&mut billing, account, round * 10 + n, 30, NOW).await;
                match result {
                    Ok(_) => tx.commit().await.unwrap(),
                    Err(_) => tx.rollback().await.unwrap(),
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }
    let spent = usage_minutes(&f.pool, account).await;
    assert!(spent <= 100, "spent {spent} of 100");
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(100 - spent));
    assert_eq!(spent, 90, "three 30 minute debits fit, every later one is refused");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_004_006_two_first_jobs_started_before_either_completes_both_bypass_in_turn() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    let base = job_base();
    add_job(&f.pool, account, base + 1, "draft", Some(36)).await;
    add_job(&f.pool, account, base + 2, "draft", Some(36)).await;
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for id in [base + 1, base + 2] {
        let (pool, barrier) = (f.pool.clone(), barrier.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            host_commit_start(&pool, account, &[id], NOW).await
        }));
    }
    for h in handles {
        let plan = h.await.unwrap().unwrap();
        assert!(matches!(plan[0].1, StartCharge::Bypass(_)), "captured eligibility: no job has completed yet");
    }
    assert_eq!(usage_rows(&f.pool, account).await, 2);
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(20 - 72));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_007_010_after_a_completed_job_the_next_start_is_strict_and_refused_when_overdrawn() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    let base = job_base();
    add_job(&f.pool, account, base + 1, "draft", Some(36)).await;
    host_commit_start(&f.pool, account, &[base + 1], NOW).await.unwrap();
    sqlx::query("UPDATE billing_test_host.jobs SET status = 'completed' WHERE id = $1")
        .bind(base + 1)
        .execute(&f.pool)
        .await
        .unwrap();
    add_job(&f.pool, account, base + 2, "draft", Some(1)).await;
    let refused = host_commit_start(&f.pool, account, &[base + 2], NOW).await.unwrap_err();
    assert_eq!(refused, ChargeError::Refused(StartRefusal::NotEnoughBalance { required: 1, available: -16 }));
    assert_eq!(job_status(&f.pool, base + 2).await, "draft");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_025_026_027_032_jobs_are_billed_once_at_start_and_a_repeat_start_charges_nothing() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await; // balance 100
    let base = job_base();
    for (n, minutes) in [(1, 20), (2, 30), (3, 40)] {
        add_job(&f.pool, account, base + n, "draft", Some(minutes)).await;
    }
    host_commit_start(&f.pool, account, &[base + 1, base + 2, base + 3], NOW).await.unwrap();
    assert_eq!(usage_minutes(&f.pool, account).await, 90);
    assert_eq!(usage_rows(&f.pool, account).await, 3);
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(10));
    // Repeated delivery of the same start: the host compare-and-swap sees no
    // draft rows, so nothing is charged again.
    let again = host_commit_start(&f.pool, account, &[base + 1, base + 2, base + 3], NOW).await.unwrap();
    assert!(again.is_empty());
    assert_eq!(usage_minutes(&f.pool, account).await, 90);
    // An insert that is already not_started is never a billable transition.
    add_job(&f.pool, account, base + 4, "not_started", Some(5)).await;
    host_commit_start(&f.pool, account, &[base + 4], NOW).await.unwrap();
    assert_eq!(usage_minutes(&f.pool, account).await, 90);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_030_031_a_refused_batch_leaves_every_job_status_and_the_ledger_untouched() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await; // balance 100
    let base = job_base();
    add_job(&f.pool, account, base + 1, "draft", Some(20)).await;
    add_job(&f.pool, account, base + 2, "ready", Some(30)).await;
    add_job(&f.pool, account, base + 3, "uploaded", Some(60)).await; // the last does not fit
    let refused = host_commit_start(&f.pool, account, &[base + 3, base + 1, base + 2], NOW).await.unwrap_err();
    assert!(matches!(refused, ChargeError::Refused(StartRefusal::NotEnoughBalance { .. })));
    assert_eq!(job_status(&f.pool, base + 1).await, "draft");
    assert_eq!(job_status(&f.pool, base + 2).await, "ready");
    assert_eq!(job_status(&f.pool, base + 3).await, "uploaded");
    assert_eq!(usage_rows(&f.pool, account).await, 0);
    // An account with no open package at all is refused the same way.
    let empty = new_account(&f).await;
    add_job(&f.pool, empty, base + 9, "draft", Some(5)).await;
    let later = Micros(NOW.0 + 400 * 86_400_000_000); // the free package has expired
    let refused = host_commit_start(&f.pool, empty, &[base + 9], later).await.unwrap_err();
    assert_eq!(refused, ChargeError::Refused(StartRefusal::NoActivePackage));
    assert_eq!(job_status(&f.pool, base + 9).await, "draft");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_028_033_034_036_a_deferred_charge_that_fails_still_commits_delivery_and_repeats_charge_once() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await; // balance 100
    let base = job_base();
    let job = base + 1;
    add_job(&f.pool, account, job, "draft", None).await;
    // Start with no known duration: nothing charged, nothing checked.
    let plan = host_commit_start(&f.pool, account, &[job], NOW).await.unwrap();
    assert_eq!(plan[0].1, StartCharge::Deferred);
    assert_eq!(usage_rows(&f.pool, account).await, 0);

    // The provider callback, run twice (repeat delivery). The host guards with a CAS on duration.
    let callback = |minutes: i64| {
        let pool = f.pool.clone();
        async move {
            let mut tx = pool.begin().await.unwrap();
            {
                let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
                billing.lock_account(account).await.unwrap();
            }
            let set = sqlx::query(
                "UPDATE billing_test_host.jobs SET duration = $2, status = 'completed' WHERE id = $1 AND duration IS NULL",
            )
            .bind(job)
            .bind(minutes as i32)
            .execute(&mut *tx)
            .await
            .unwrap();
            let charged = if set.rows_affected() == 1 {
                let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
                Some(charge_strict(&mut billing, account, job, minutes, NOW).await)
            } else {
                None
            };
            tx.commit().await.unwrap(); // delivery facts commit whatever the charge said
            charged
        }
    };
    assert!(callback(10).await.unwrap().is_ok());
    assert!(callback(10).await.is_none(), "the repeated callback charges nothing");
    assert_eq!(usage_minutes(&f.pool, account).await, 10);

    // A second job whose deferred charge cannot be covered is still delivered.
    let broke = new_account(&f).await;
    let late = base + 2;
    add_job(&f.pool, broke, late, "not_started", None).await;
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(broke).await.unwrap();
    }
    sqlx::query("UPDATE billing_test_host.jobs SET duration = 500, status = 'completed' WHERE id = $1")
        .bind(late)
        .execute(&mut *tx)
        .await
        .unwrap();
    let refused = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        charge_strict(&mut billing, broke, late, 500, NOW).await
    };
    assert!(matches!(refused, Err(ChargeError::Refused(StartRefusal::NotEnoughBalance { .. }))));
    tx.commit().await.unwrap();
    assert_eq!(job_status(&f.pool, late).await, "completed", "delivery is not withheld");
    assert_eq!(usage_rows(&f.pool, broke).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_042_a_hard_delete_returns_the_minutes_and_failed_jobs_keep_their_charge() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await;
    let base = job_base();
    add_job(&f.pool, account, base + 1, "draft", Some(30)).await;
    add_job(&f.pool, account, base + 2, "draft", Some(20)).await;
    host_commit_start(&f.pool, account, &[base + 1, base + 2], NOW).await.unwrap();
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(50));
    // A failed job keeps its ledger rows: there is no refund path.
    sqlx::query("UPDATE billing_test_host.jobs SET status = 'failed' WHERE id = $1")
        .bind(base + 1)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(50));
    // Deleting the job row for good, in one transaction with the release, gives the minutes back.
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(account).await.unwrap();
    }
    sqlx::query("DELETE FROM billing_test_host.jobs WHERE id = $1").bind(base + 2).execute(&mut *tx).await.unwrap();
    let released = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        release_subject_charges(&mut billing, account, base + 2).await.unwrap()
    };
    tx.commit().await.unwrap();
    assert_eq!(released, 1);
    assert_eq!(open_balance(&f.pool, account, NOW).await, Some(70));
}

/// The minimal host's start, then the answer metadata read in the same
/// transaction after the debit, as the source read it after the jobs started.
async fn start_and_answer(pool: &PgPool, account: AccountId, ids: &[i64]) -> StartAnswerMeta {
    let mut tx = pool.begin().await.unwrap();
    host_start_in(&mut tx, account, ids, NOW).await.unwrap();
    let answer = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        start_answer(&mut billing, account, NOW).await.unwrap()
    };
    tx.commit().await.unwrap();
    answer
}

/// BIL-074, with FRE-013: `newBalance` is the balance after the debit (0 when no
/// package is open) and `isFreemium` is app definition 1, exactly one package row
/// ever and it is named `freemium`, whatever its dates (source: `transcription.ts`
/// reads `Balances.getBalance() || 0` and `Balances.freemium()` after the start).
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_074_started_answers_carry_the_balance_after_the_debit_and_the_single_free_package_flag() {
    let f = fixture(vec![]).await;
    let base = job_base();

    // Freemium only: the first job overdraws the 20 free minutes.
    let free = new_account(&f).await;
    add_job(&f.pool, free, base + 1, "draft", Some(36)).await;
    assert_eq!(start_and_answer(&f.pool, free, &[base + 1]).await, StartAnswerMeta { new_balance: -16, is_freemium: true });

    // A paid account: 20 free plus 100 paid, a job of 37.
    let paid = new_account(&f).await;
    give_package(&f.pool, paid, "PREMIUM", 100, 3).await;
    add_job(&f.pool, paid, base + 2, "draft", Some(37)).await;
    assert_eq!(start_and_answer(&f.pool, paid, &[base + 2]).await, StartAnswerMeta { new_balance: 120 - 37, is_freemium: false });

    // The definitions differ here: the paid package has expired, the free one is still
    // open. The database rule treats the start as freemium-only (the first job
    // bypasses), but the answer's flag is app definition 1: two rows ever, so false.
    let lapsed = new_account(&f).await;
    {
        let mut tx = f.pool.begin().await.unwrap();
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(lapsed).await.unwrap();
        billing
            .insert_package(&NewPackage {
                account: lapsed,
                payment_id: None,
                payment_seq: None,
                name: "PREMIUM".into(),
                minutes: 100,
                start_at: add_months(NOW, -2).unwrap(),
                end_at: add_months(NOW, -1).unwrap(),
                created: add_months(NOW, -2).unwrap(),
                signup_grant: false,
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    add_job(&f.pool, lapsed, base + 3, "draft", Some(5)).await;
    assert_eq!(start_and_answer(&f.pool, lapsed, &[base + 3]).await, StartAnswerMeta { new_balance: 15, is_freemium: false });
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_019_accounts_are_isolated_and_a_cross_account_usage_reference_is_rejected() {
    let f = fixture(vec![]).await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let package_a = give_package(&f.pool, a, "PREMIUM", 50, 3).await;
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    charge_strict(&mut billing, b, 1, 15, NOW).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(usage_minutes(&f.pool, a).await, 0, "a deduction for B never changes A");
    assert_eq!(open_balance(&f.pool, a, NOW).await, Some(70));

    // B tries to write usage against A's package: the composite key refuses it.
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.lock_account(b).await.unwrap();
    let rejected = billing
        .insert_usage(&[NewUsage { account: b, package_id: package_a, subject: Some(1), minutes: 5, used_at: NOW }])
        .await;
    assert!(matches!(rejected, Err(StoreError::Conflict(_))), "got {rejected:?}");
    assert_eq!(usage_minutes(&f.pool, a).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_seam_refuses_a_transaction_that_is_not_read_committed() {
    let f = fixture(vec![]).await;
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ").execute(&mut *tx).await.unwrap();
    match PgBillingTx::attach(&mut tx).await {
        Err(StoreError::Conflict(message)) => assert!(message.contains("READ COMMITTED"), "{message}"),
        Err(other) => panic!("wrong error {other:?}"),
        Ok(_) => panic!("a repeatable read transaction must be refused"),
    }
    // A transaction billing opened itself is READ COMMITTED and refuses an unknown account.
    let store = billing_postgres::PgBillingStore::new(f.pool.clone());
    let mut unit = store.begin().await.unwrap();
    let missing = account();
    assert_eq!(unit.tx().lock_account(missing).await, Err(StoreError::AccountMissing(missing)));
    unit.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_001_the_host_seam_refuses_an_autocommit_connection_so_no_charge_can_commit_alone() {
    let f = fixture(vec![]).await;
    let mut connection = f.pool.acquire().await.unwrap();
    match PgBillingTx::attach(&mut connection).await {
        Err(StoreError::Conflict(message)) => assert!(message.contains("explicit transaction"), "{message}"),
        Err(other) => panic!("wrong error {other:?}"),
        Ok(_) => panic!("an autocommit connection must be refused"),
    }
    // The same connection inside BEGIN is accepted, and a late failure rolls the whole charge back.
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await;
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        charge_strict(&mut billing, account, 77, 50, NOW).await.unwrap();
    }
    let late_failure = sqlx::query("INSERT INTO billing_test_host.jobs (id, account_id, status) VALUES (NULL, $1, 'draft')")
        .bind(account.0)
        .execute(&mut *tx)
        .await;
    assert!(late_failure.is_err());
    drop(tx); // rolled back
    assert_eq!(usage_rows(&f.pool, account).await, 0, "the debit did not commit by itself");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_036_a_sql_failure_inside_a_deferred_debit_is_isolated_by_a_savepoint_and_delivery_still_commits() {
    let f = fixture(vec![]).await;
    let account = new_account(&f).await;
    give_package(&f.pool, account, "PREMIUM", 80, 3).await;
    let job = job_base() + 1;
    add_job(&f.pool, account, job, "not_started", None).await;
    let package_id = {
        let mut tx = f.pool.begin().await.unwrap();
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.packages(account).await.unwrap()[0].package.id
    };
    let rows = |second_minutes: i64| {
        vec![
            NewUsage { account, package_id, subject: Some(job), minutes: 5, used_at: NOW },
            NewUsage { account, package_id, subject: Some(job), minutes: second_minutes, used_at: NOW },
        ]
    };

    // Without isolation the failed second insert poisons the whole transaction: nothing after it can run.
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(account).await.unwrap();
    }
    let failed = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.insert_usage(&rows(-1)).await
    };
    assert!(matches!(failed, Err(StoreError::Conflict(_))), "{failed:?}");
    let poisoned = sqlx::query("UPDATE billing_test_host.jobs SET status = 'completed' WHERE id = $1").bind(job).execute(&mut *tx).await;
    assert!(poisoned.is_err(), "the outer transaction is aborted without a savepoint");
    drop(tx);

    // With a savepoint around the whole debit, the delivery facts commit and no debit row survives.
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(account).await.unwrap();
    }
    sqlx::query("UPDATE billing_test_host.jobs SET status = 'completed', duration = 5 WHERE id = $1")
        .bind(job)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT deferred_debit").execute(&mut *tx).await.unwrap();
    let failed = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.insert_usage(&rows(-1)).await
    };
    assert!(matches!(failed, Err(StoreError::Conflict(_))));
    sqlx::query("ROLLBACK TO SAVEPOINT deferred_debit").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(job_status(&f.pool, job).await, "completed", "the transcript is delivered");
    assert_eq!(usage_rows(&f.pool, account).await, 0, "the first row of the failed debit was rolled back with it");
}

/// Imported ledger rows `(row account, package, subject, minutes)`, written as the
/// import role: the only authority that may preserve historical anomalies.
async fn import_usage(rows: &[(AccountId, Option<i64>, i64, i32)]) {
    let owner = owner().await;
    let base = (Uuid::new_v4().as_u128() as i64) & 0x0fff_ffff_ffff_f000;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    for (n, (row_account, package_id, subject, minutes)) in rows.iter().enumerate() {
        sqlx::query(
            "INSERT INTO billing.legacy_usage (id, account_id, package_id, subject_ref, minutes, used_at)
             VALUES ($1, $2, $3, $4, $5, now())",
        )
        .bind(base + n as i64 + 1)
        .bind(row_account.0)
        .bind(*package_id)
        .bind(*subject)
        .bind(*minutes)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

async fn legacy_rows_of_subject(subject: i64) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM billing.legacy_usage WHERE subject_ref = $1")
        .bind(subject)
        .fetch_one(&owner().await)
        .await
        .unwrap()
}

/// The host's hard delete: billing locks every affected owner first, then the host
/// deletes its own job row, all in one transaction.
async fn host_hard_delete_in(tx: &mut sqlx::Transaction<'static, sqlx::Postgres>, account: AccountId, subject: i64) -> u64 {
    let released = {
        let mut billing = PgBillingTx::attach(tx).await.unwrap();
        release_subject_charges(&mut billing, account, subject).await.unwrap()
    };
    sqlx::query("DELETE FROM billing_test_host.jobs WHERE id = $1").bind(subject).execute(&mut **tx).await.unwrap();
    released
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_042_a_hard_delete_cascades_every_imported_row_of_the_subject_across_accounts_and_signs() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let (a, b, c) = (new_account(&f).await, new_account(&f).await, new_account(&f).await);
    let package_a = give_package(&f.pool, a, "PREMIUM", 80, 3).await;
    let package_b = give_package(&f.pool, b, "PREMIUM", 50, 3).await;
    let package_c = give_package(&f.pool, c, "PREMIUM", 40, 3).await;
    let subject = job_base() + 5;
    let unrelated = subject + 1;
    add_job(&f.pool, a, subject, "completed", Some(10)).await;
    import_usage(&[
        (a, Some(package_a), subject, 30),  // the subject's own account and package
        (a, Some(package_b), subject, 10),  // positive, drawn from another account's package
        (b, Some(package_c), subject, -15), // negative, naming a second account on a third one's package
        (a, None, subject, 5),              // no package at all: changes no balance
        (b, Some(package_b), unrelated, 4), // another subject: must stay
    ])
    .await;
    // New usage for the same subject, drawn from the account's earliest-ending package.
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        charge_strict(&mut billing, a, subject, 10, NOW).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(open_balance(&f.pool, a, NOW).await, Some(100 - 30 - 10));
    assert_eq!(open_balance(&f.pool, b, NOW).await, Some(70 - 10 - 4));
    assert_eq!(open_balance(&f.pool, c, NOW).await, Some(60 + 15));

    // Before the delete the anomalies are reportable.
    let report = owner_anomalies(&owner).await.unwrap();
    assert!(report.iter().any(|r| r.package_id == package_b), "a row naming A on B's package is reported");
    assert!(report.iter().any(|r| r.package_id == package_c), "a row naming B on C's package is reported");

    // Every package owner the release changes, in the order billing locks them.
    let mut expected = vec![a, b, c];
    expected.sort();
    let owners = {
        let mut tx = f.pool.begin().await.unwrap();
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.imported_release_owners(a, subject).await.unwrap()
    };
    assert_eq!(owners, expected);

    let mut tx = f.pool.begin().await.unwrap();
    let released = host_hard_delete_in(&mut tx, a, subject).await;
    tx.commit().await.unwrap();
    assert_eq!(released, 1 + 4, "the new usage row and all four imported rows of the subject");
    assert_eq!(open_balance(&f.pool, a, NOW).await, Some(100));
    assert_eq!(open_balance(&f.pool, b, NOW).await, Some(70 - 4), "B gets back what the subject drew from it");
    assert_eq!(open_balance(&f.pool, c, NOW).await, Some(60), "removing a negative row lowers C's balance, as the cascade did");
    assert_eq!(legacy_rows_of_subject(subject).await, 0);
    assert_eq!(legacy_rows_of_subject(unrelated).await, 1);
    let report = owner_anomalies(&owner).await.unwrap();
    assert!(!report.iter().any(|r| r.package_id == package_b || r.package_id == package_c));
}

/// Releases `subject` for `a` and holds that transaction open, then runs a strict
/// debit for `b` on another connection. Proves the debit waits on `b`'s account
/// row, commits the release, and returns what the debit decided afterwards.
async fn debit_during_release(f: &Fixture, a: AccountId, b: AccountId, subject: i64, minutes: i64) -> Result<Vec<Allocation>, ChargeError> {
    let owner = owner().await;
    let (released_tx, released_rx) = oneshot::channel();
    let (commit_tx, commit_rx) = oneshot::channel::<()>();
    let pool = f.pool.clone();
    let release = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let released = host_hard_delete_in(&mut tx, a, subject).await;
        released_tx.send(released).unwrap();
        commit_rx.await.unwrap();
        tx.commit().await.unwrap();
    });
    assert_eq!(released_rx.await.unwrap(), 1, "the imported row is released");

    let (pid_tx, pid_rx) = oneshot::channel();
    let pool = f.pool.clone();
    let debit = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *tx).await.unwrap();
        pid_tx.send(pid).unwrap();
        let result = {
            let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
            charge_strict(&mut billing, b, subject + 100, minutes, NOW).await
        };
        match &result {
            Ok(_) => tx.commit().await.unwrap(),
            Err(_) => tx.rollback().await.unwrap(),
        }
        result
    });
    let pid = pid_rx.await.unwrap();
    let mut waiting = false;
    for _ in 0..100 {
        waiting = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')")
            .bind(pid)
            .fetch_one(&owner)
            .await
            .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    commit_tx.send(()).unwrap();
    release.await.unwrap();
    let result = debit.await.unwrap();
    assert!(waiting, "the affected owner's debit waits on its account row, locked by the release");
    result
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_042_def_001_a_debit_of_an_affected_owner_waits_for_an_imported_refund_and_spends_it() {
    let f = fixture(vec![]).await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let package_b = give_package(&f.pool, b, "PREMIUM", 80, 3).await; // B holds 100
    let subject = job_base() + 7;
    add_job(&f.pool, a, subject, "completed", Some(30)).await;
    import_usage(&[(a, Some(package_b), subject, 30)]).await; // A's job drew 30 from B's package
    assert_eq!(open_balance(&f.pool, b, NOW).await, Some(70), "a debit of 90 would be refused before the release");

    let taken = debit_during_release(&f, a, b, subject, 90).await.expect("after the release B holds 100 again");
    assert_eq!(taken.iter().map(|t| t.minutes).sum::<i64>(), 90);
    assert_eq!(open_balance(&f.pool, b, NOW).await, Some(10));
    assert_eq!(legacy_rows_of_subject(subject).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_042_def_001_a_debit_of_an_affected_owner_waits_for_a_negative_imported_row_to_go_and_cannot_overspend() {
    let f = fixture(vec![]).await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let package_b = give_package(&f.pool, b, "PREMIUM", 80, 3).await; // B holds 100
    let subject = job_base() + 9;
    add_job(&f.pool, a, subject, "completed", Some(20)).await;
    import_usage(&[(a, Some(package_b), subject, -20)]).await; // an imported negative row credits B
    assert_eq!(open_balance(&f.pool, b, NOW).await, Some(120), "a debit of 110 would pass before the release");

    let refused = debit_during_release(&f, a, b, subject, 110).await.unwrap_err();
    assert_eq!(
        refused,
        ChargeError::Refused(StartRefusal::NotEnoughBalance { required: 110, available: 100 }),
        "the debit decided on the balance after the release, not the one it would have read unlocked"
    );
    assert_eq!(open_balance(&f.pool, b, NOW).await, Some(100));
}
