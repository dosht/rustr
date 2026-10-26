//! Erasing an account's enrolled sample resources (the directory row and the
//! metered sample work), through the restricted runtime login against a real
//! database: the delete is possible only behind the billing erasure barrier,
//! and a write that was already in flight when the barrier appeared cannot
//! survive the erasure or recreate a row after it.
//!
//! Needs `BILLING_TEST_DATABASE_URL` naming a disposable PostgreSQL database whose
//! user may create roles. `#[ignore]`d offline; with `--include-ignored` a missing
//! variable fails, it never skips.

use std::str::FromStr;
use std::time::Duration;

use billing_core::erasure::open_account_erasure;
use billing_core::ports::{Customer, CustomerDirectory};
use billing_core::types::{AccountId, Micros};
use billing_postgres::{PgBillingStore, connect_owner, migrate, provision_runtime_login};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use taskr_billing::work::PgCustomerDirectory;
use taskr_billing::{erase_enrolled_sample_resources, migrate_host};
use tokio::sync::OnceCell;
use uuid::Uuid;

const LOGIN: &str = "billing_taskr_erase_runtime";
const PASSWORD: &str = "billing-taskr-erase-runtime-password";

fn url() -> String {
    std::env::var("BILLING_TEST_DATABASE_URL").expect("BILLING_TEST_DATABASE_URL must name a disposable PostgreSQL database")
}

static READY: OnceCell<()> = OnceCell::const_new();

async fn runtime() -> PgPool {
    let owner = connect_owner(&url()).await.unwrap();
    READY
        .get_or_init(|| async {
            migrate(&owner).await.unwrap();
            migrate_host(&owner).await.unwrap();
            provision_runtime_login(&owner, LOGIN, PASSWORD).await.unwrap();
        })
        .await;
    let options = PgConnectOptions::from_str(&url()).unwrap().username(LOGIN).password(PASSWORD);
    PgPoolOptions::new().max_connections(6).connect_with(options).await.unwrap()
}

fn customer(account: Uuid) -> Customer {
    Customer { account: AccountId(account), email: format!("{}@example.test", account.simple()), full_name: Some("Someone".into()), created_at: Micros(1_700_000_000_000_000) }
}

async fn add_sample_work(pool: &PgPool, account: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO taskr_billing.sample_work (account_id, work_key, units, operation, status, created_at) VALUES ($1, 'w', 1, 'op', 'pending', now())")
        .bind(account)
        .execute(pool)
        .await
        .map(|_| ())
}

async fn rows(pool: &PgPool, account: Uuid) -> (i64, i64) {
    let customers = sqlx::query_scalar("SELECT count(*) FROM taskr_billing.customers WHERE account_id = $1").bind(account).fetch_one(pool).await.unwrap();
    let work = sqlx::query_scalar("SELECT count(*) FROM taskr_billing.sample_work WHERE account_id = $1").bind(account).fetch_one(pool).await.unwrap();
    (customers, work)
}

async fn open_barrier(pool: &PgPool, account: Uuid) {
    open_account_erasure(&PgBillingStore::new(pool.clone()), AccountId(account), Micros(1_700_000_001_000_000)).await.unwrap();
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_runtime_cannot_delete_enrolled_sample_rows_without_an_erasure_barrier() {
    let pool = runtime().await;
    let account = Uuid::new_v4();
    PgCustomerDirectory::new(pool.clone()).register(&customer(account)).await.unwrap();
    add_sample_work(&pool, account).await.unwrap();
    assert_eq!(rows(&pool, account).await, (1, 1));

    for table in ["customers", "sample_work"] {
        let deleted = sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM taskr_billing.{table} WHERE account_id = $1"))).bind(account).execute(&pool).await.unwrap().rows_affected();
        assert_eq!(deleted, 0, "{table}: row security refuses the delete while the account is not being erased");
    }
    assert_eq!(rows(&pool, account).await, (1, 1));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn an_erasure_removes_the_directory_row_and_sample_work_and_nothing_recreates_them() {
    let pool = runtime().await;
    let (erased, kept) = (Uuid::new_v4(), Uuid::new_v4());
    let directory = PgCustomerDirectory::new(pool.clone());
    for account in [erased, kept] {
        directory.register(&customer(account)).await.unwrap();
        add_sample_work(&pool, account).await.unwrap();
    }

    open_barrier(&pool, erased).await;
    let mut tx = pool.begin().await.unwrap();
    let removed = erase_enrolled_sample_resources(&mut tx, AccountId(erased)).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!((removed.customers, removed.sample_work), (1, 1));
    assert_eq!(rows(&pool, erased).await, (0, 0));
    assert_eq!(rows(&pool, kept).await, (1, 1), "another account is untouched");

    // A stale enrollment or contact refresh, and a late sample operation, are refused by the
    // database at write time.
    assert!(directory.register(&customer(erased)).await.is_err(), "a stale enrollment cannot recreate the directory row");
    assert!(add_sample_work(&pool, erased).await.is_err(), "a late sample operation cannot recreate work");
    assert_eq!(rows(&pool, erased).await, (0, 0));
    // Repeating the erasure is a no-op.
    let mut tx = pool.begin().await.unwrap();
    let again = erase_enrolled_sample_resources(&mut tx, AccountId(erased)).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!((again.customers, again.sample_work), (0, 0));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_write_in_flight_when_the_barrier_appears_is_waited_for_and_then_removed() {
    let pool = runtime().await;
    let account = Uuid::new_v4();

    // The write began before the barrier: its trigger already passed and holds the shared lock.
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO taskr_billing.customers (account_id, email, full_name, created_at) VALUES ($1, $2, 'Someone', now())")
        .bind(account)
        .bind(format!("{}@example.test", account.simple()))
        .execute(&mut *writer)
        .await
        .unwrap();
    open_barrier(&pool, account).await;

    let eraser_pool = pool.clone();
    let mut eraser = tokio::spawn(async move {
        let mut tx = eraser_pool.begin().await.unwrap();
        let removed = erase_enrolled_sample_resources(&mut tx, AccountId(account)).await.unwrap();
        tx.commit().await.unwrap();
        removed
    });
    assert!(tokio::time::timeout(Duration::from_millis(400), &mut eraser).await.is_err(), "the erasure waits behind the write in flight");

    writer.commit().await.unwrap();
    let removed = tokio::time::timeout(Duration::from_secs(10), eraser).await.expect("the erasure proceeds once the write committed").unwrap();
    assert_eq!(removed.customers, 1, "the row the in-flight write created is removed, not left behind");
    assert_eq!(rows(&pool, account).await, (0, 0));
}
