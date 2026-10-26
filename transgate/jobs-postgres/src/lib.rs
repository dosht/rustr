//! PostgreSQL adapter for the transgate-core job, retention and erasure ports.
//!
//! - Schema `transgate_jobs`, migration history `transgate_jobs._sqlx_migrations`,
//!   beside billing's and media's own histories.
//! - One pool: the verified billing runtime login (`billing_postgres::connect_runtime`).
//!   The runtime rights of this schema are granted to the `billing_runtime` group,
//!   so a job transaction and the billing transaction it joins are one transaction
//!   on one connection, and no second runtime role exists.
//! - Every unit of work is READ COMMITTED. Lock order (frozen seam 5): the provider
//!   capacity lock (submission claims only), the host erasure barrier row, the
//!   billing account rows, job rows ascending, then revision compare-and-swap.

mod account;
mod ai;
mod locks;
mod rate;
mod rows;
mod store;
mod uow;

use billing_core::ports::StoreError;
use sqlx::PgPool;

pub use locks::PgLocks;
pub use rate::PgRateLimiter;
pub use store::{ClaimedEvent, PgJobStore};
pub use uow::PgJobUow;

pub const SCHEMA: &str = "transgate_jobs";
/// The group that holds this schema's runtime rights: billing's runtime group.
pub const RUNTIME_ROLE: &str = billing_postgres::RUNTIME_ROLE;

pub(crate) fn unavailable(e: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(e.to_string())
}

pub(crate) fn db(e: sqlx::Error) -> StoreError {
    if let Some(d) = e.as_database_error() && let Some(code) = d.code() {
        // Integrity and data errors are a refusal of this write, not an outage.
        if code.starts_with("23") || code.starts_with("22") {
            return StoreError::Conflict(d.message().to_string());
        }
    }
    StoreError::Unavailable(e.to_string())
}

fn media(e: media_postgres::MediaStoreError) -> StoreError {
    StoreError::Unavailable(e.to_string())
}

/// Applies the jobs migrations, then media's schedule and lock schema, and
/// grants the media runtime rights to `billing_runtime`. Owner authority only;
/// run after `billing_postgres::migrate` (the group must exist).
pub async fn migrate(owner: &PgPool) -> Result<(), StoreError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.create_schema(SCHEMA);
    migrator.dangerous_set_table_name("transgate_jobs._sqlx_migrations");
    migrator
        .run(owner)
        .await
        .map_err(|e| StoreError::Unavailable(format!("transgate jobs migration failed: {e}")))?;
    media_postgres::migrate(owner).await.map_err(media)?;
    media_postgres::grant_runtime(owner, RUNTIME_ROLE).await.map_err(media)
}

/// Columns the runtime may write, per table and privilege. The grants in the
/// migration and this list must agree; verification enforces it.
fn column_write_allowed(table: &str, column: &str, privilege: &str) -> bool {
    match (table, privilege) {
        ("jobs" | "attempts" | "lines" | "inbox" | "events" | "retention_runs" | "issued_uploads"
        | "archived_payloads" | "erasures" | "erasure_items" | "rate_windows", "INSERT") => true,
        ("settings", "INSERT") => matches!(column, "account_id" | "auto_delete_transcripts" | "deleted_monthly"),
        ("settings", "UPDATE") => matches!(column, "auto_delete_transcripts" | "deleted_monthly"),
        ("jobs", "UPDATE") => !matches!(column, "id" | "account_id" | "created_at"),
        ("attempts", "UPDATE") => {
            matches!(column, "state" | "lease_until" | "remote_id" | "accepted_at" | "last_error" | "remote_cleanup")
        }
        ("lines", "UPDATE") => matches!(column, "line" | "speaker"),
        ("inbox", "UPDATE") => matches!(column, "observation" | "state" | "settled_at" | "note" | "failures" | "last_error"),
        ("events", "UPDATE") => matches!(column, "attempts" | "claim_token" | "claimed_until" | "published_at" | "last_error"),
        ("retention_runs", "UPDATE") => {
            matches!(column, "rows_deleted" | "execution_time_ms" | "status" | "error_message" | "users_opted_out_count")
        }
        ("erasures", "UPDATE") => matches!(column, "state" | "local_erased_at" | "identity_deleted_at" | "closed_at"),
        ("erasure_items", "UPDATE") => matches!(column, "state" | "attempts" | "target"),
        ("archived_payloads", "UPDATE") => column == "write",
        ("ai_consent" | "source_revisions" | "generations" | "chat_messages" | "completion_summaries"
        | "summary_requests" | "quota_counters" | "llm_dispatches" | "job_feedback" | "support_requests", "INSERT") => true,
        ("support_requests", "UPDATE") => matches!(column, "attempts" | "claimed_until" | "last_error"),
        ("llm_dispatches", "UPDATE") => matches!(column, "disposition" | "remote_ids" | "unkept_id_observed"),
        ("ai_consent", "UPDATE") => column != "account_id",
        ("source_revisions", "UPDATE") => column == "revision",
        ("generations", "UPDATE") => matches!(
            column,
            "status" | "sequence" | "draft" | "staged" | "execution_token" | "lease_until" | "quota" | "interruption"
                | "remote_outcome_unknown" | "remote_id" | "tokens_used" | "updated_at" | "finished_at"
        ),
        ("summary_requests", "UPDATE") => matches!(column, "generation_id" | "outcome" | "decided_at"),
        ("quota_counters", "UPDATE") => matches!(column, "count" | "max_limit" | "reset_at"),
        ("rate_windows", "UPDATE") => matches!(column, "window_start_us" | "reset_at_us" | "hits"),
        _ => false,
    }
}

/// The barrier (`erasures`), the audit rows and the rate windows are never deleted.
fn delete_allowed(table: &str) -> bool {
    matches!(
        table,
        "jobs" | "attempts" | "lines" | "inbox" | "events" | "settings" | "issued_uploads" | "archived_payloads"
            | "erasure_items" | "ai_consent" | "generations" | "chat_messages" | "completion_summaries"
            | "summary_requests" | "quota_counters" | "llm_dispatches" | "job_feedback" | "support_requests"
    )
}

/// What `role` can do in schema `transgate_jobs` beyond the runtime contract:
/// schema creation or ownership, table ownership, a column write outside the
/// allow-list, and any DELETE outside it, TRUNCATE, TRIGGER or REFERENCES.
pub async fn runtime_authority_problems(pool: &PgPool, role: &str) -> Result<Vec<String>, StoreError> {
    let mut problems = Vec::new();
    let schema: Option<(bool, bool)> = sqlx::query_as(
        "SELECT has_schema_privilege($1::name, 'transgate_jobs', 'CREATE'),
                COALESCE((SELECT pg_get_userbyid(nspowner)::text = $1 FROM pg_namespace WHERE nspname = 'transgate_jobs'), false)",
    )
    .bind(role)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    if let Some((can_create, owns)) = schema {
        if can_create {
            problems.push("can create objects in schema transgate_jobs".into());
        }
        if owns {
            problems.push("owns schema transgate_jobs".into());
        }
    }
    let owned: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'transgate_jobs' AND c.relkind = 'r' AND pg_get_userbyid(c.relowner)::text = $1",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    problems.extend(owned.into_iter().map(|t| format!("owns transgate_jobs.{t}")));
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.relname::text, a.attname::text, p.priv
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'transgate_jobs'
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
         CROSS JOIN (VALUES ('INSERT'), ('UPDATE')) AS p(priv)
         WHERE c.relkind = 'r' AND c.relname <> '_sqlx_migrations'
           AND has_column_privilege($1::name, c.oid, a.attnum, p.priv)",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    for (table, column, privilege) in columns {
        if !column_write_allowed(&table, &column, &privilege) {
            problems.push(format!("can {privilege} transgate_jobs.{table}.{column}"));
        }
    }
    let table_rights: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.relname::text, p.priv
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'transgate_jobs'
         CROSS JOIN (VALUES ('DELETE'), ('TRUNCATE'), ('TRIGGER'), ('REFERENCES')) AS p(priv)
         WHERE c.relkind = 'r' AND has_table_privilege($1::name, c.oid, p.priv)",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    for (table, privilege) in table_rights {
        if !(privilege == "DELETE" && delete_allowed(&table)) {
            problems.push(format!("can {privilege} transgate_jobs.{table}"));
        }
    }
    Ok(problems)
}

/// Refuses a pool that is not the restricted runtime: billing's own check
/// (`verify_runtime_role`), then this schema's and media's contract for both
/// the connected login and the `billing_runtime` group it acts as.
pub async fn verify_runtime(pool: &PgPool) -> Result<(), StoreError> {
    let who = billing_postgres::verify_runtime_role(pool).await?.user;
    let mut problems = Vec::new();
    for role in [who.as_str(), RUNTIME_ROLE] {
        for p in runtime_authority_problems(pool, role).await? {
            problems.push(format!("{role} {p}"));
        }
        for p in media_postgres::runtime_authority_problems(pool, role).await.map_err(media)? {
            problems.push(format!("{role} {p}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(StoreError::Conflict(format!(
            "database role {who:?} is not a restricted transgate runtime role: {}",
            problems.join("; ")
        )))
    }
}
