//! Signup-anchored provisioning, the restricted runtime role, import
//! preservation and migration coexistence, against a real database.

mod support;

use billing_core::app::BillingError;
use billing_core::calendar::utc;
use billing_core::grants::PaymentStatus;
use billing_core::ports::{BillingTx, StoreError};
use billing_postgres::{
    PgBillingTx, connect_runtime, migrate, options_for_login, owner_anomalies, provision_runtime_login, verify_runtime_role,
};
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::{AssertSqlSafe, PgPool, SqlSafeStr};
use support::*;
use uuid::Uuid;

fn sqlstate(e: &sqlx::Error) -> String {
    e.as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default()
}

async fn packages_of(f: &Fixture, account: billing_core::types::AccountId) -> Vec<billing_core::ledger::PackageBalance> {
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.packages(account).await.unwrap()
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_001_002_first_access_months_after_signup_gets_the_original_expired_window_not_a_new_trial() {
    let f = fixture(vec![]).await;
    let signup = utc(2026, 1, 1, 0, 0, 0, 0);
    let first_access = utc(2026, 4, 1, 9, 30, 0, 0);
    let id = account();
    // The host knows when the user signed up; billing does not guess it from "now".
    f.directory.add(customer(id, &unique_email(), signup));
    f.clock.set(first_access);

    let overview = f.billing.overview(id).await.unwrap();
    assert_eq!(overview.balance, None, "the two month window of 20 minutes ended on March 1");
    let packages = packages_of(&f, id).await;
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].package.minutes_included, 20);
    assert_eq!(packages[0].package.start_at, signup);
    assert_eq!(packages[0].package.end_at, utc(2026, 3, 1, 0, 0, 0, 0));
    assert_eq!(packages[0].package.name.as_deref(), Some("freemium"));

    // Repeated sign-in, duplicate provisioning and expired reads grant nothing more.
    for _ in 0..3 {
        f.billing.provision(customer(id, "other@example.test", first_access)).await.unwrap();
        f.billing.overview(id).await.unwrap();
    }
    assert_eq!(packages_of(&f, id).await.len(), 1);
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'FreemiumGranted'", id).await,
        1
    );
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_001_a_user_with_no_trusted_signup_record_is_refused_not_granted_a_trial() {
    let f = fixture(vec![]).await;
    let stranger = account();
    assert_eq!(f.billing.overview(stranger).await.unwrap_err(), BillingError::NotProvisioned);
    assert_eq!(f.billing.ensure_provisioned(stranger).await.unwrap_err(), BillingError::NotProvisioned);
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.accounts WHERE account_id = $1", stranger).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_001_concurrent_provisioning_races_grant_exactly_one_package() {
    let f = std::sync::Arc::new(fixture(vec![]).await);
    let id = account();
    let signup = utc(2026, 1, 10, 0, 0, 0, 0);
    f.directory.add(customer(id, &unique_email(), signup));
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(6));
    let mut handles = Vec::new();
    for _ in 0..6 {
        let (f, barrier) = (f.clone(), barrier.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            f.billing.provision(customer(id, "x@example.test", signup)).await.unwrap()
        }));
    }
    let mut granted = 0;
    for h in handles {
        if h.await.unwrap().granted {
            granted += 1;
        }
    }
    assert_eq!(granted, 1, "exactly one of the racing provisions created the account");
    assert_eq!(packages_of(&f, id).await.len(), 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_002_the_runtime_login_cannot_import_update_the_ledger_or_gain_authority() {
    let f = fixture(vec![]).await;
    // Held for the whole test: while the group-contamination test has the group
    // briefly over-privileged, these forbidden statements must not run at all.
    let fixture_owner = owner().await;
    with_role_fixture_shared(&fixture_owner, runtime_login_cannot_import_update_or_gain_authority(&f)).await;
}

async fn runtime_login_cannot_import_update_or_gain_authority(f: &Fixture) {
    let pool = &f.pool;
    let a = provisioned(f, &unique_email()).await;
    let b = provisioned(f, &unique_email()).await;
    let package_a = packages_of(f, a).await[0].package.id;

    // The connection really is the restricted role, and it passes verification.
    let who: String = sqlx::query_scalar("SELECT current_user::text").fetch_one(pool).await.unwrap();
    assert_eq!(who, RUNTIME_LOGIN);
    assert_eq!(verify_runtime_role(pool).await.unwrap().user, RUNTIME_LOGIN);
    // An owner connection must not be accepted as a runtime connection.
    let refused = verify_runtime_role(&owner().await).await.unwrap_err();
    assert!(matches!(refused, StoreError::Conflict(_)), "{refused:?}");

    // Default-path negative row: refused by the table, whoever asks.
    let negative = sqlx::query(
        "INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at) VALUES ($1, $2, 1, -5, now())",
    )
    .bind(a.0)
    .bind(package_a)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&negative), "23514", "check violation");

    // The historical-anomaly table and every other import path are closed.
    let legacy = sqlx::query("INSERT INTO billing.legacy_usage (id, minutes) VALUES ($1, -5)")
        .bind(Uuid::new_v4().as_u128() as i64 & 0x0fff_ffff_ffff_ffff)
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(sqlstate(&legacy), "42501", "permission denied");
    for forbidden in [
        "UPDATE billing.usage SET minutes = 0",
        "UPDATE billing.packages SET minutes_included = 999999",
        "UPDATE billing.accounts SET created_at = now()",
        "UPDATE billing.legacy_usage SET minutes = 0",
        "UPDATE billing.payments SET amount = 0.01",
        "UPDATE billing.payments SET created = now()",
        "DELETE FROM billing.payments",
        "DELETE FROM billing.packages",
        "TRUNCATE billing.usage",
        "INSERT INTO billing.coupons (id) VALUES ('forbidden')",
        "CREATE TABLE billing.runtime_made_this (id int)",
        "SET ROLE billing_import",
    ] {
        let error = sqlx::query(forbidden).execute(pool).await.unwrap_err();
        assert_eq!(sqlstate(&error), "42501", "{forbidden}: {error}");
    }

    // A new write cannot reference another account's package.
    let cross = sqlx::query(
        "INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at) VALUES ($1, $2, 1, 5, now())",
    )
    .bind(b.0)
    .bind(package_a)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&cross), "23503", "composite ownership key");
    let cross_package = sqlx::query(
        "INSERT INTO billing.packages (account_id, payment_id, name, minutes_included, created, start_at, end_at)
         VALUES ($1, 999999999, 'x', 1, now(), now(), now())",
    )
    .bind(a.0)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&cross_package), "23503", "a package cannot point at a payment that is not its account's");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_002_imported_owner_anomalies_are_preserved_counted_by_package_and_reported() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let a = account();
    let b = account();
    let ids = (Uuid::new_v4().as_u128() as i64) & 0x0fff_ffff_ffff_f000;

    // Written as the import role, the only authority that may preserve this.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    for id in [a, b] {
        sqlx::query("INSERT INTO billing.accounts (account_id, created_at) VALUES ($1, now())")
            .bind(id.0)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let package: i64 = sqlx::query_scalar(
        "INSERT INTO billing.packages (account_id, name, minutes_included, created, start_at, end_at)
         VALUES ($1, 'PREMIUM', 100, now(), now() - interval '1 day', now() + interval '30 days') RETURNING id",
    )
    .bind(a.0)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    for (n, account, minutes) in [(1, a, 20), (2, b, 10), (3, a, -5)] {
        sqlx::query("INSERT INTO billing.legacy_usage (id, account_id, package_id, minutes, used_at) VALUES ($1, $2, $3, $4, now())")
            .bind(ids + n)
            .bind(account.0)
            .bind(package)
            .bind(minutes)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();

    // The runtime counts every row that references the package, whoever it names.
    let packages = packages_of(&f, a).await;
    assert_eq!(packages[0].consumed, 20 + 10 - 5);
    assert_eq!(packages[0].remaining(), 75);

    let report = owner_anomalies(&owner).await.unwrap();
    let mine = report.iter().find(|r| r.package_id == package).expect("the anomaly is reported");
    assert_eq!(mine.groups, 2);
    assert_eq!(mine.rows, 3);
    assert_eq!(mine.legacy_remaining_sum, (100 - 15) + (100 - 10), "the legacy grouping counts the package twice");
    assert_eq!(mine.normalized_remaining, 75);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_078_historical_free_text_gateways_and_canceled_status_are_preserved_on_read() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let a = provisioned(&f, &unique_email()).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO billing.payments (account_id, amount, currency, payment_gateway, status, created)
         VALUES ($1, 393.35999999999996, 'TRY', 'Iyzico IBAN', 'canceled', now()) RETURNING id",
    )
    .bind(a.0)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut rt = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut rt).await.unwrap();
    let payment = billing.payment(id, false).await.unwrap().unwrap();
    assert_eq!(payment.status, PaymentStatus::Canceled);
    assert_eq!(payment.gateway, "Iyzico IBAN");
    assert_eq!(payment.amount.to_string(), "393.35999999999996", "historical money is read back digit for digit");
}

/// A host migrator and the billing migrator keep separate histories, even with the
/// same version number. The synthetic host here keeps its history in its own test
/// schema, never in the shared `public._sqlx_migrations` that the real taskr
/// migrator owns; coexistence with that real history is proved by the taskr-billing
/// consumer test.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn migration_histories_coexist_and_rerun_cleanly() {
    let owner: PgPool = owner().await;
    let mut host = Migrator::with_migrations(vec![Migration::new(
        1,
        "coexistence marker".into(),
        MigrationType::Simple,
        "CREATE TABLE IF NOT EXISTS billing_test_host.coexistence_marker (id int)".into_sql_str(),
        false,
    )]);
    host.dangerous_set_table_name("billing_test_host._host_history");
    host.run(&owner).await.expect("synthetic host migrator");
    host.run(&owner).await.expect("synthetic host migrator reruns");
    migrate(&owner).await.expect("billing migrator reruns beside it");
    migrate(&owner).await.expect("and again");

    let billing_versions: i64 = sqlx::query_scalar("SELECT count(*) FROM billing._sqlx_migrations WHERE version = 1 AND success")
        .fetch_one(&owner)
        .await
        .unwrap();
    let host_versions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_test_host._host_history WHERE version = 1 AND description = 'coexistence marker'",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(billing_versions, 1, "billing's version 1 lives in billing._sqlx_migrations");
    assert_eq!(host_versions, 1, "the host's version 1 lives in its own history");
    let billing_rows_in_host_history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM billing_test_host._host_history WHERE description ILIKE '%billing%'")
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(billing_rows_in_host_history, 0, "billing never writes to another history");
}

/// A login that already exists with extra authority: a column-level grant on the
/// ledger, a column-level INSERT on the import table, or a membership that can
/// reach another role. Re-keying it must be refused, and connecting as it through
/// `connect_runtime` must be refused, so a server never runs on it.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_002_a_contaminated_existing_login_is_refused_by_provisioning_and_by_the_runtime_connection() {
    let owner = owner().await;
    let tag = Uuid::new_v4().simple().to_string();
    let cases = [
        ("ledger_column", "GRANT UPDATE (minutes) ON billing.usage TO", "billing.usage.minutes"),
        ("import_column", "GRANT INSERT (id) ON billing.legacy_usage TO", "billing.legacy_usage.id"),
        ("payment_amount", "GRANT UPDATE (amount) ON billing.payments TO", "billing.payments.amount"),
        ("table_delete", "GRANT DELETE ON billing.packages TO", "DELETE billing.packages"),
        ("predefined_role", "GRANT pg_read_all_data TO", "pg_read_all_data"),
        ("import_role", "GRANT billing_import TO", "billing_import"),
    ];
    for (name, grant, expected) in cases {
        let role = format!("billing_dirty_{name}_{}", &tag[..8]);
        for statement in [
            format!("CREATE ROLE {role} LOGIN PASSWORD 'pw' IN ROLE billing_runtime"),
            format!("{grant} {role}"),
        ] {
            sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
        }
        let (provisioned, connected) = with_role_fixture_shared(&owner, async {
            let provisioned = provision_runtime_login(&owner, &role, "another-password").await;
            let options = options_for_login(&database_url(), &role, "pw").unwrap();
            (provisioned, connect_runtime(options, 1).await)
        })
        .await;
        for statement in [format!("DROP OWNED BY {role}"), format!("DROP ROLE {role}")] {
            sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
        }
        let refused = provisioned.expect_err(name);
        assert!(matches!(&refused, StoreError::Conflict(m) if m.contains(expected)), "{name}: {refused:?}");
        let refused = connected.expect_err(name);
        assert!(matches!(&refused, StoreError::Conflict(m) if m.contains(expected)), "{name}: {refused:?}");
    }
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_002_a_clean_login_is_created_rekeyed_and_accepted_by_the_runtime_connection() {
    let owner = owner().await;
    let role = format!("billing_clean_{}", &Uuid::new_v4().simple().to_string()[..8]);
    let pool = with_role_fixture_shared(&owner, async {
        provision_runtime_login(&owner, &role, "first-password").await.unwrap();
        provision_runtime_login(&owner, &role, "second-password").await.unwrap();
        connect_runtime(options_for_login(&database_url(), &role, "second-password").unwrap(), 1).await
    })
    .await;
    if let Ok(pool) = &pool {
        pool.close().await;
    }
    for statement in [format!("DROP OWNED BY {role}"), format!("DROP ROLE {role}")] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    pool.expect("the re-keyed login passes runtime verification");
}

async fn role_exists(owner: &PgPool, role: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
        .bind(role)
        .fetch_one(owner)
        .await
        .unwrap()
}

/// The allow-listed group itself is contaminated: an attribute, a membership, a
/// column privilege or schema authority that any member reaches with `SET ROLE`.
/// Provisioning refuses to create a login and the runtime connection refuses the
/// already-provisioned test login. Role state is cluster-wide, so each contamination
/// is committed only while this test holds the role fixture lock exclusively, and
/// undone before anything is asserted.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_002_a_contaminated_billing_runtime_group_is_refused_by_provisioning_and_by_the_runtime_connection() {
    let owner = owner().await;
    let tag = Uuid::new_v4().simple().to_string();
    let probe = format!("billing_group_probe_{}", &tag[..8]);
    let set_only = format!("billing_set_only_{}", &tag[..8]);
    let cases = [
        ("ALTER ROLE billing_runtime CREATEROLE", "ALTER ROLE billing_runtime NOCREATEROLE", "the group billing_runtime can create roles"),
        ("ALTER ROLE billing_runtime LOGIN", "ALTER ROLE billing_runtime NOLOGIN", "the group billing_runtime can log in"),
        (
            "GRANT pg_write_all_data TO billing_runtime",
            "REVOKE pg_write_all_data FROM billing_runtime",
            "the group billing_runtime can act as the role pg_write_all_data",
        ),
        (
            "GRANT UPDATE (minutes) ON billing.usage TO billing_runtime",
            "REVOKE UPDATE (minutes) ON billing.usage FROM billing_runtime",
            "the group billing_runtime can UPDATE billing.usage.minutes",
        ),
        (
            "GRANT CREATE ON SCHEMA billing TO billing_runtime",
            "REVOKE CREATE ON SCHEMA billing FROM billing_runtime",
            "the group billing_runtime can create objects in schema billing",
        ),
    ];

    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock($1)").bind(ROLE_FIXTURE_LOCK).execute(&mut *lock).await.unwrap();
    let mut outcomes = Vec::new();
    for (contaminate, restore, expected) in cases {
        sqlx::query(contaminate).execute(&owner).await.unwrap();
        let connected = connect_runtime(options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap(), 1)
            .await
            .map(|_| ());
        let provisioned = provision_runtime_login(&owner, &probe, "probe-password").await;
        let created = role_exists(&owner, &probe).await;
        sqlx::query(restore).execute(&owner).await.unwrap();
        outcomes.push((expected, connected, provisioned, created));
    }

    // A member that does not inherit the group's privileges still reaches them with
    // SET ROLE, which is why the group is checked and not only the login.
    for statement in [
        format!("CREATE ROLE {set_only} LOGIN PASSWORD 'pw'"),
        format!("GRANT billing_runtime TO {set_only} WITH INHERIT FALSE, SET TRUE"),
        "GRANT UPDATE (minutes) ON billing.usage TO billing_runtime".to_string(),
    ] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    let options = options_for_login(&database_url(), &set_only, "pw").unwrap();
    let set_only_connected = connect_runtime(options.clone(), 1).await.map(|_| ());
    let mut direct = <sqlx::PgConnection as sqlx::Connection>::connect_with(&options).await.unwrap();
    let without_set_role = sqlx::query("UPDATE billing.usage SET minutes = minutes WHERE false").execute(&mut direct).await;
    sqlx::query("SET ROLE billing_runtime").execute(&mut direct).await.unwrap();
    let with_set_role = sqlx::query("UPDATE billing.usage SET minutes = minutes WHERE false").execute(&mut direct).await;
    drop(direct);
    sqlx::query("REVOKE UPDATE (minutes) ON billing.usage FROM billing_runtime").execute(&owner).await.unwrap();
    for statement in [format!("DROP OWNED BY {set_only}"), format!("DROP ROLE {set_only}")] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    let clean = connect_runtime(options_for_login(&database_url(), RUNTIME_LOGIN, RUNTIME_PASSWORD).unwrap(), 1)
        .await
        .map(|_| ());
    sqlx::query("SELECT pg_advisory_unlock($1)").bind(ROLE_FIXTURE_LOCK).execute(&mut *lock).await.unwrap();
    drop(lock);

    for (expected, connected, provisioned, created) in outcomes {
        let refused = connected.expect_err(expected);
        assert!(matches!(&refused, StoreError::Conflict(m) if m.contains(expected)), "connect: {expected}: {refused:?}");
        let refused = provisioned.expect_err(expected);
        assert!(matches!(&refused, StoreError::Conflict(m) if m.contains(expected)), "provision: {expected}: {refused:?}");
        assert!(!created, "{expected}: no login is created while the group is contaminated");
    }
    assert_eq!(sqlstate(&without_set_role.unwrap_err()), "42501", "the non-inheriting member has no direct write");
    assert!(with_set_role.is_ok(), "but SET ROLE billing_runtime reaches the contaminated group's write");
    let refused = set_only_connected.expect_err("a non-inheriting member of a contaminated group");
    assert!(
        matches!(&refused, StoreError::Conflict(m) if m.contains("the group billing_runtime can UPDATE billing.usage.minutes")),
        "{refused:?}"
    );
    clean.expect("the restored group passes again");
}

/// A login holding the group WITH ADMIN OPTION could hand runtime authority to
/// any other role, so it is not a restricted runtime login.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_002_a_login_that_can_grant_the_runtime_group_onward_is_refused() {
    let owner = owner().await;
    let role = format!("billing_admin_opt_{}", &Uuid::new_v4().simple().to_string()[..8]);
    for statement in [
        format!("CREATE ROLE {role} LOGIN PASSWORD 'pw'"),
        format!("GRANT billing_runtime TO {role} WITH ADMIN OPTION"),
    ] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    let (provisioned, connected) = with_role_fixture_shared(&owner, async {
        let provisioned = provision_runtime_login(&owner, &role, "another-password").await;
        let options = options_for_login(&database_url(), &role, "pw").unwrap();
        (provisioned, connect_runtime(options, 1).await.map(|_| ()))
    })
    .await;
    for statement in [format!("DROP OWNED BY {role}"), format!("DROP ROLE {role}")] {
        sqlx::query(AssertSqlSafe(statement)).execute(&owner).await.unwrap();
    }
    for outcome in [provisioned, connected] {
        let refused = outcome.expect_err("an admin-option member");
        assert!(matches!(&refused, StoreError::Conflict(m) if m.contains("can grant billing_runtime to other roles")), "{refused:?}");
    }
}
