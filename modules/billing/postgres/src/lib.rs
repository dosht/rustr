//! PostgreSQL adapter for billing-core.
//!
//! - Schema `billing`, migration history `billing._sqlx_migrations`: it
//!   coexists with a host's own migrations and reruns cleanly.
//! - Migration and import authority is separate from the runtime role. A
//!   running server connects as a login that is a member of `billing_runtime`
//!   only; `connect_runtime` refuses anything with more authority.
//! - Every transaction is READ COMMITTED and takes the account row lock first.

mod store;
mod tx;

use std::str::FromStr;

use billing_core::ports::StoreError;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool, Row};

pub use store::PgBillingStore;
pub use tx::PgBillingTx;

pub const SCHEMA: &str = "billing";
pub const RUNTIME_ROLE: &str = "billing_runtime";
pub const IMPORT_ROLE: &str = "billing_import";

fn unavailable(e: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(e.to_string())
}

/// A pool with owner authority, for migrations and role provisioning only.
pub async fn connect_owner(url: &str) -> Result<PgPool, StoreError> {
    PgPoolOptions::new().max_connections(4).connect(url).await.map_err(unavailable)
}

/// Applies the billing migrations into schema `billing` with their own history.
pub async fn migrate(owner: &PgPool) -> Result<(), StoreError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.create_schema("billing");
    migrator.dangerous_set_table_name("billing._sqlx_migrations");
    migrator
        .run(owner)
        .await
        .map_err(|e| StoreError::Unavailable(format!("billing migration failed: {e}")))
}

fn valid_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.starts_with("pg_")
}

/// Columns of `billing.payments` the runtime role may write; the grants in the
/// migration and this list must agree, and verification enforces that.
const PAYMENT_INSERT: [&str; 14] = [
    "account_id", "amount", "currency", "payment_gateway", "environment", "status", "token", "created",
    "metadata", "coupon_id", "pricing_plan_id", "customer_id", "subscription_ref", "provider_order_ref",
];
const PAYMENT_UPDATE: [&str; 8] = [
    "status", "token", "customer_id", "subscription_ref", "provider_order_ref", "subscription_canceled_at",
    "observed", "receipt_url",
];

fn column_write_allowed(table: &str, column: &str, privilege: &str) -> bool {
    match (table, privilege) {
        ("accounts" | "packages" | "usage" | "outbox_deliveries", "INSERT") => true,
        ("accounts", "UPDATE") => column == "touched_at",
        ("payments", "INSERT") => PAYMENT_INSERT.contains(&column),
        ("payments", "UPDATE") => PAYMENT_UPDATE.contains(&column),
        ("subscriptions" | "unresolved_notifications" | "outbox", _) => true,
        ("erasure_barriers" | "erasure_terminations" | "erasure_checkout_resolutions", "INSERT") => true,
        ("erasure_barriers", "UPDATE") => column == "billing_erased_at",
        ("erasure_terminations", "UPDATE") => {
            matches!(column, "customer_ref" | "status" | "attempts" | "last_outcome" | "updated_at")
        }
        _ => false,
    }
}

/// Tables the runtime may delete from. The ledger tables unconditionally (a
/// subject's hard delete); the account row and its outbox only behind row
/// security that restricts deletion to an account under erasure.
fn delete_allowed(table: &str, row_security: bool) -> bool {
    match table {
        "usage" | "legacy_usage" => true,
        "accounts" | "outbox" | "outbox_deliveries" => row_security,
        _ => false,
    }
}

/// Everything `role` can do beyond the runtime contract, from the database's
/// own point of view: attributes, other roles reachable (including through
/// SET ROLE and inheritance, predefined roles too), ownership, schema rights
/// and every table and column privilege, column-level grants included.
async fn authority_problems(pool: &PgPool, role: &str) -> Result<Vec<String>, StoreError> {
    let mut problems = Vec::new();
    let attributes = sqlx::query(
        "SELECT rolsuper, rolcreaterole, rolcreatedb, rolbypassrls, rolreplication FROM pg_roles WHERE rolname = $1",
    )
    .bind(role)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    if let Some(row) = attributes {
        for (column, why) in [
            ("rolsuper", "is a superuser"),
            ("rolcreaterole", "can create roles"),
            ("rolcreatedb", "can create databases"),
            ("rolbypassrls", "bypasses row security"),
            ("rolreplication", "can replicate"),
        ] {
            if row.try_get::<bool, _>(column).map_err(unavailable)? {
                problems.push(why.to_string());
            }
        }
    }
    let others: Vec<String> = sqlx::query_scalar(
        "SELECT r.rolname::text FROM pg_roles r
         WHERE r.rolname <> $1 AND r.rolname <> 'billing_runtime'
           AND pg_has_role($1::name, r.oid, 'MEMBER') ORDER BY 1",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    problems.extend(others.into_iter().map(|r| format!("can act as the role {r}")));
    let schema: Option<(bool, bool)> = sqlx::query_as(
        "SELECT has_schema_privilege($1::name, 'billing', 'CREATE'),
                COALESCE((SELECT pg_get_userbyid(nspowner)::text = $1 FROM pg_namespace WHERE nspname = 'billing'), false)",
    )
    .bind(role)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    if let Some((can_create, owns)) = schema {
        if can_create {
            problems.push("can create objects in schema billing".into());
        }
        if owns {
            problems.push("owns schema billing".into());
        }
    }
    let owned: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'billing' AND c.relkind = 'r' AND pg_get_userbyid(c.relowner)::text = $1",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    problems.extend(owned.into_iter().map(|t| format!("owns billing.{t}")));
    // Column-level privileges: any INSERT or UPDATE outside the runtime's allow-list.
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.relname::text, a.attname::text, p.priv
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'billing'
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
         CROSS JOIN (VALUES ('INSERT'), ('UPDATE')) AS p(priv)
         WHERE c.relkind = 'r' AND has_column_privilege($1::name, c.oid, a.attnum, p.priv)",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    for (table, column, privilege) in columns {
        if !column_write_allowed(&table, &column, &privilege) {
            problems.push(format!("can {privilege} billing.{table}.{column}"));
        }
    }
    // Table-level DELETE, TRUNCATE and TRIGGER: only the two ledger tables may be deleted from,
    // plus the erasure targets while row security guards them.
    let table_rights: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT c.relname::text, p.priv, c.relrowsecurity
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'billing'
         CROSS JOIN (VALUES ('DELETE'), ('TRUNCATE'), ('TRIGGER'), ('REFERENCES')) AS p(priv)
         WHERE c.relkind = 'r' AND has_table_privilege($1::name, c.oid, p.priv)",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    for (table, privilege, row_security) in table_rights {
        let allowed = privilege == "DELETE" && delete_allowed(&table, row_security);
        if !allowed {
            problems.push(format!("can {privilege} billing.{table}"));
        }
    }
    Ok(problems)
}

/// The group's own authority. A member can `SET ROLE billing_runtime` and act
/// with the group's attributes and privileges whether or not it inherits them,
/// and the migration leaves an existing group as it found it, so the group is
/// checked like a login, plus that it cannot log in itself.
async fn group_problems(pool: &PgPool) -> Result<Vec<String>, StoreError> {
    let can_login: Option<bool> = sqlx::query_scalar("SELECT rolcanlogin FROM pg_roles WHERE rolname = $1")
        .bind(RUNTIME_ROLE)
        .fetch_optional(pool)
        .await
        .map_err(unavailable)?;
    let Some(can_login) = can_login else {
        return Ok(vec![format!("the group {RUNTIME_ROLE} does not exist")]);
    };
    let mut problems = Vec::new();
    if can_login {
        problems.push("can log in".to_string());
    }
    problems.extend(authority_problems(pool, RUNTIME_ROLE).await?);
    Ok(problems.into_iter().map(|p| format!("the group {RUNTIME_ROLE} {p}")).collect())
}

/// Everything wrong with `login` as a runtime login: its own authority, an
/// admin option that would let it hand the group to other roles, and the
/// authority of the group it acts as.
async fn runtime_login_problems(pool: &PgPool, login: &str) -> Result<Vec<String>, StoreError> {
    let mut problems = authority_problems(pool, login).await?;
    let admin: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_auth_members m
                        JOIN pg_roles g ON g.oid = m.roleid JOIN pg_roles u ON u.oid = m.member
                        WHERE g.rolname = $1 AND u.rolname = $2 AND m.admin_option)",
    )
    .bind(RUNTIME_ROLE)
    .bind(login)
    .fetch_one(pool)
    .await
    .map_err(unavailable)?;
    if admin {
        problems.push(format!("can grant {RUNTIME_ROLE} to other roles"));
    }
    problems.extend(group_problems(pool).await?);
    Ok(problems)
}

/// Creates (or re-keys) a login that can act as `billing_runtime` and nothing
/// more. An existing login is re-keyed only when its current authority is
/// already within the runtime contract: leftover grants, memberships or
/// attributes are refused, never silently kept.
pub async fn provision_runtime_login(owner: &PgPool, login: &str, password: &str) -> Result<(), StoreError> {
    if !valid_identifier(login) || login == RUNTIME_ROLE || login == IMPORT_ROLE {
        return Err(StoreError::Conflict(format!("{login:?} is not a usable login name")));
    }
    if password.is_empty() || password.contains('\0') {
        return Err(StoreError::Conflict("the runtime login needs a non-empty password".into()));
    }
    let password = password.replace('\'', "''");
    let group = group_problems(owner).await?;
    if !group.is_empty() {
        return Err(StoreError::Conflict(format!(
            "no runtime login is provisioned while {}",
            group.join("; ")
        )));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
        .bind(login)
        .fetch_one(owner)
        .await
        .map_err(unavailable)?;
    if exists {
        let problems = runtime_login_problems(owner, login).await?;
        if !problems.is_empty() {
            return Err(StoreError::Conflict(format!(
                "the existing role {login:?} has authority beyond the runtime contract and is not re-keyed: {}",
                problems.join("; ")
            )));
        }
    }
    let attributes = "LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT";
    let statement = if exists {
        format!("ALTER ROLE \"{login}\" WITH {attributes} PASSWORD '{password}'")
    } else {
        format!("CREATE ROLE \"{login}\" WITH {attributes} PASSWORD '{password}'")
    };
    // `login` is restricted to [a-z0-9_] above and the password's quotes are doubled.
    sqlx::query(AssertSqlSafe(statement)).execute(owner).await.map_err(unavailable)?;
    sqlx::query(AssertSqlSafe(format!("GRANT {RUNTIME_ROLE} TO \"{login}\"")))
        .execute(owner)
        .await
        .map_err(unavailable)?;
    let problems = runtime_login_problems(owner, login).await?;
    if !problems.is_empty() {
        return Err(StoreError::Conflict(format!(
            "the role {login:?} still has authority beyond the runtime contract: {}",
            problems.join("; ")
        )));
    }
    Ok(())
}

/// Connection options for the same server as `url` but another login.
pub fn options_for_login(url: &str, login: &str, password: &str) -> Result<PgConnectOptions, StoreError> {
    Ok(PgConnectOptions::from_str(url).map_err(unavailable)?.username(login).password(password))
}

/// What a connected role can do, from the database's own point of view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRole {
    pub user: String,
}

/// Refuses a connection that is not the restricted runtime role: a
/// superuser, an owner of the ledger, an import member, a role that can act
/// as any other role, or anything with a table or column privilege beyond the
/// runtime contract (column-level grants included). A pool that passes this is
/// what servers must use; a test of `SET ROLE` on an owner pool is not proof.
pub async fn verify_runtime_role(pool: &PgPool) -> Result<RuntimeRole, StoreError> {
    let who: String = sqlx::query_scalar("SELECT current_user::text").fetch_one(pool).await.map_err(unavailable)?;
    let member: bool = sqlx::query_scalar("SELECT pg_has_role(current_user, 'billing_runtime', 'MEMBER')")
        .fetch_one(pool)
        .await
        .map_err(unavailable)?;
    let mut problems = runtime_login_problems(pool, &who).await?;
    if !member {
        problems.push("is not a member of billing_runtime".into());
    }
    if problems.is_empty() {
        Ok(RuntimeRole { user: who })
    } else {
        Err(StoreError::Conflict(format!(
            "database role {who:?} is not a restricted billing runtime role: it {}",
            problems.join("; ")
        )))
    }
}

/// A pool for a running server. Fails unless the connecting role is the
/// restricted runtime role.
pub async fn connect_runtime(options: PgConnectOptions, max_connections: u32) -> Result<PgPool, StoreError> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
        .map_err(unavailable)?;
    verify_runtime_role(&pool).await?;
    Ok(pool)
}

/// A package a usage row references whose recorded owner differs from the
/// package's own owner: preserved as imported, counted by package, reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerAnomaly {
    pub package_id: i64,
    pub package_owner: String,
    pub groups: i64,
    pub rows: i64,
    /// The legacy `GROUP BY package, user` grouping returns the package once
    /// per recorded user, so its remaining minutes count once per group.
    pub legacy_remaining_sum: i64,
    /// What this module computes: included minus every row that references it.
    pub normalized_remaining: i64,
}

/// Packages whose usage rows name more than one account (or one that is not
/// the owner). The two remaining-minute figures are the exact source-versus-
/// normalized difference P9 must report instead of hiding.
pub async fn owner_anomalies(pool: &PgPool) -> Result<Vec<OwnerAnomaly>, StoreError> {
    let rows = sqlx::query(
        "WITH usage_all AS (
           SELECT package_id, account_id, minutes FROM billing.usage
           UNION ALL
           SELECT package_id, account_id, minutes FROM billing.legacy_usage
         ), grouped AS (
           SELECT package_id, account_id, sum(minutes)::bigint AS minutes, count(*)::bigint AS rows
           FROM usage_all WHERE package_id IS NOT NULL GROUP BY package_id, account_id
         )
         SELECT p.id AS package_id, p.account_id::text AS owner,
                count(*)::bigint AS groups, sum(g.rows)::bigint AS rows,
                sum(p.minutes_included - g.minutes)::bigint AS legacy_remaining,
                (p.minutes_included - sum(g.minutes))::bigint AS normalized_remaining
         FROM grouped g JOIN billing.packages p ON p.id = g.package_id
         GROUP BY p.id, p.account_id, p.minutes_included
         HAVING count(*) > 1 OR bool_or(g.account_id IS DISTINCT FROM p.account_id)
         ORDER BY p.id",
    )
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    rows.iter()
        .map(|r| {
            Ok(OwnerAnomaly {
                package_id: r.try_get("package_id").map_err(unavailable)?,
                package_owner: r.try_get("owner").map_err(unavailable)?,
                groups: r.try_get("groups").map_err(unavailable)?,
                rows: r.try_get("rows").map_err(unavailable)?,
                legacy_remaining_sum: r.try_get("legacy_remaining").map_err(unavailable)?,
                normalized_remaining: r.try_get("normalized_remaining").map_err(unavailable)?,
            })
        })
        .collect()
}
