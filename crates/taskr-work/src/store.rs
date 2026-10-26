//! taskr-work's persistence, in its own database and schema. Every account
//! operation runs under one transaction-scoped advisory lock per account, taken
//! before anything else, so enrollment, edits, worker writes and erasure
//! serialize and the erasure barrier is checked under that same lock.
//!
//! No network happens in a transaction.

use billing_core::erasure::{ErasureError, erase_account_billing};
use billing_core::types::{AccountId, Micros as BillingMicros};
use billing_postgres::PgBillingTx;
use media_ports::{ObjectRef, Provider, RemoteIdentity, SecretString};
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::work::{Attempt, AttemptPhase, Micros, Resources, Status, Work};

mod summary;

pub const SCHEMA: &str = "taskr_work";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Unavailable(String),
    /// The account never enrolled.
    NotEnrolled,
    /// The erasure barrier exists: nothing may be created, signed, edited or applied for this account.
    Erased,
    /// A compare-and-swap lost: someone else saved first.
    Conflict,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Unavailable(why) => write!(f, "the work store is unavailable: {why}"),
            StoreError::NotEnrolled => write!(f, "the account is not enrolled in audio work"),
            StoreError::Erased => write!(f, "the account is unavailable due to account erasure"),
            StoreError::Conflict => write!(f, "the work changed concurrently"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Why the combined erasure transaction did not commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EraseDataError {
    Store(StoreError),
    /// Billing refused to cascade yet (a subscription may still renew, the barrier is not open
    /// or the affected owners changed): nothing was erased, and retrying later is correct.
    Blocked(String),
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> StoreError {
        StoreError::Unavailable(e.to_string())
    }
}

/// Owner authority: creates the schema and its own migration history.
pub async fn migrate(owner: &PgPool) -> Result<(), StoreError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.create_schema(SCHEMA);
    migrator.dangerous_set_table_name("taskr_work._sqlx_migrations");
    migrator.run(owner).await.map_err(|e| StoreError::Unavailable(format!("taskr-work migration failed: {e}")))
}

fn valid_role(role: &str) -> bool {
    !role.is_empty() && role.len() <= 63 && role.chars().enumerate().all(|(i, c)| c == '_' || c.is_ascii_lowercase() || (i > 0 && c.is_ascii_digit()))
}

/// Gives the runtime role exactly what the sample writes. The barrier table can never be deleted from.
pub async fn grant_runtime(owner: &PgPool, role: &str) -> Result<(), StoreError> {
    if !valid_role(role) {
        return Err(StoreError::Unavailable(format!("{role:?} is not a usable role name")));
    }
    for statement in [
        format!("GRANT USAGE ON SCHEMA {SCHEMA} TO {role}"),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON {SCHEMA}.enrollments, {SCHEMA}.works, {SCHEMA}.attempts, {SCHEMA}.erasure_resources TO {role}"),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON {SCHEMA}.summary_generations, {SCHEMA}.summaries TO {role}"),
        format!("GRANT SELECT, INSERT, UPDATE ON {SCHEMA}.erasures TO {role}"),
        format!("GRANT USAGE ON ALL SEQUENCES IN SCHEMA {SCHEMA} TO {role}"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement)).execute(owner).await?;
    }
    Ok(())
}

/// The proof that the pool's actual database holds both this sample's schema and billing's: the
/// combined erasure runs in one transaction on this one pool, so both must be reachable through
/// it. It asks the connected database itself rather than comparing connection strings.
pub async fn verify_same_database(pool: &PgPool) -> Result<(), StoreError> {
    let present: (bool, bool, bool) = io(sqlx::query_as(
        "SELECT to_regclass('taskr_work.works') IS NOT NULL, to_regclass('billing.accounts') IS NOT NULL, to_regclass('taskr_billing.customers') IS NOT NULL",
    )
    .fetch_one(pool)
    .await)?;
    let mut missing = Vec::new();
    for (found, what) in [(present.0, "the audio-work schema (taskr_work)"), (present.1, "the billing schema (billing)"), (present.2, "the billing sample schema (taskr_billing)")] {
        if !found {
            missing.push(what);
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(StoreError::Unavailable(format!(
            "the audio-work sample and the billing sample must share one database, and the connected database lacks {}",
            missing.join(", ")
        )))
    }
}

pub fn provider_str(provider: Provider) -> &'static str {
    match provider {
        Provider::Soniox => "soniox",
        Provider::Deepgram => "deepgram",
    }
}

pub fn parse_provider(text: &str) -> Option<Provider> {
    match text {
        "soniox" => Some(Provider::Soniox),
        "deepgram" => Some(Provider::Deepgram),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrollment {
    pub account: Uuid,
    pub email: String,
    pub enrolled: Micros,
    pub auto_delete: bool,
}

/// What a callback needs, loaded by the attempt id in its query string.
#[derive(Clone, Debug)]
pub struct Binding {
    pub work: Uuid,
    pub account: Uuid,
    pub attempt: Uuid,
    pub provider: Provider,
    pub project: String,
    pub remote_id: Option<String>,
    pub phase: AttemptPhase,
    pub secret: SecretString,
}

/// A secret generated for a new attempt, stored with it before any request is sent.
pub struct NewSecret<'a> {
    pub attempt: Uuid,
    pub secret: &'a SecretString,
    /// The provider project the attempt is submitted under: part of the callback binding.
    pub project: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErasureResource {
    pub id: i64,
    pub account: Uuid,
    pub kind: String,
    pub target: serde_json::Value,
    pub state: String,
    pub blocked_until: Option<Micros>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

pub struct Tx {
    inner: Transaction<'static, Postgres>,
}

fn io<T>(result: Result<T, sqlx::Error>) -> Result<T, StoreError> {
    result.map_err(StoreError::from)
}

fn enrollment(row: &PgRow) -> Result<Enrollment, StoreError> {
    Ok(Enrollment {
        account: io(row.try_get("account_id"))?,
        email: io(row.try_get("email"))?,
        enrolled: Micros(io(row.try_get("enrolled_us"))?),
        auto_delete: io(row.try_get("auto_delete"))?,
    })
}

const WORK_COLUMNS: &str =
    "id, account_id, object, name, language, requested, status, revision, transcript, error, created_us, completed_us";
const ATTEMPT_COLUMNS: &str =
    "id, number, provider, project, phase, lease_until_us, claimed_us, accepted_us, remote_id, reason, result_digest";

fn work_from(row: &PgRow, attempts: Vec<Attempt>) -> Result<Work, StoreError> {
    let corrupt = |what: &str| StoreError::Unavailable(format!("a stored work has an unreadable {what}"));
    let object: Json<ObjectRef> = io(row.try_get("object"))?;
    let status: String = io(row.try_get("status"))?;
    let requested: Option<String> = io(row.try_get("requested"))?;
    let revision: i64 = io(row.try_get("revision"))?;
    Ok(Work {
        id: io(row.try_get("id"))?,
        owner: io(row.try_get("account_id"))?,
        object: object.0,
        name: io(row.try_get("name"))?,
        language: io(row.try_get("language"))?,
        requested: match requested {
            Some(text) => Some(parse_provider(&text).ok_or_else(|| corrupt("provider"))?),
            None => None,
        },
        status: Status::parse(&status).ok_or_else(|| corrupt("status"))?,
        revision: revision as u64,
        attempts,
        transcript: io(row.try_get("transcript"))?,
        error: io(row.try_get("error"))?,
        created_at: Micros(io(row.try_get("created_us"))?),
        completed_at: io::<Option<i64>>(row.try_get("completed_us"))?.map(Micros),
    })
}

fn attempt_from(row: &PgRow) -> Result<Attempt, StoreError> {
    let corrupt = |what: &str| StoreError::Unavailable(format!("a stored attempt has an unreadable {what}"));
    let provider: String = io(row.try_get("provider"))?;
    let project: String = io(row.try_get("project"))?;
    let phase: String = io(row.try_get("phase"))?;
    let remote_id: Option<String> = io(row.try_get("remote_id"))?;
    let number: i32 = io(row.try_get("number"))?;
    let provider = parse_provider(&provider).ok_or_else(|| corrupt("provider"))?;
    Ok(Attempt {
        id: io(row.try_get("id"))?,
        number: number as u32,
        provider,
        phase: AttemptPhase::parse(&phase).ok_or_else(|| corrupt("phase"))?,
        lease_until: Micros(io(row.try_get("lease_until_us"))?),
        claimed_at: Micros(io(row.try_get("claimed_us"))?),
        accepted_at: io::<Option<i64>>(row.try_get("accepted_us"))?.map(Micros),
        remote: remote_id.map(|id| RemoteIdentity { provider, project, id }),
        reason: io(row.try_get("reason"))?,
        result_digest: io(row.try_get("result_digest"))?,
    })
}

impl Store {
    pub fn new(pool: PgPool) -> Store {
        Store { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn begin(&self) -> Result<Tx, StoreError> {
        Ok(Tx { inner: io(self.pool.begin().await)? })
    }

    pub async fn ping(&self) -> Result<(), StoreError> {
        io(sqlx::query("SELECT 1").execute(&self.pool).await).map(|_| ())
    }

    pub async fn erased(&self, account: Uuid) -> Result<bool, StoreError> {
        let found: Option<i32> = io(sqlx::query_scalar("SELECT 1 FROM taskr_work.erasures WHERE account_id = $1").bind(account).fetch_optional(&self.pool).await)?;
        Ok(found.is_some())
    }

    /// Work that is ready to be submitted: uploaded or pending with no live attempt, oldest first.
    pub async fn dispatchable(&self, limit: i64) -> Result<Vec<(Uuid, Uuid)>, StoreError> {
        let rows = io(sqlx::query(
            "SELECT w.id, w.account_id FROM taskr_work.works w
             WHERE w.status IN ('uploaded', 'pending')
               AND NOT EXISTS (SELECT 1 FROM taskr_work.erasures x WHERE x.account_id = w.account_id)
               AND NOT EXISTS (SELECT 1 FROM taskr_work.attempts a WHERE a.work_id = w.id AND a.phase IN ('claimed', 'accepted', 'uncertain'))
             ORDER BY w.created_us LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await)?;
        rows.iter().map(|r| Ok((io(r.try_get("id"))?, io(r.try_get("account_id"))?))).collect()
    }

    /// Work whose live attempt's lease has run out: a claim that never got an answer.
    pub async fn with_expired_attempts(&self, now: Micros, limit: i64) -> Result<Vec<(Uuid, Uuid)>, StoreError> {
        let rows = io(sqlx::query(
            "SELECT DISTINCT w.id, w.account_id, w.created_us FROM taskr_work.works w
             JOIN taskr_work.attempts a ON a.work_id = w.id
             WHERE w.status = 'pending' AND a.phase IN ('claimed', 'uncertain') AND a.lease_until_us <= $1
               AND NOT EXISTS (SELECT 1 FROM taskr_work.erasures x WHERE x.account_id = w.account_id)
             ORDER BY w.created_us LIMIT $2",
        )
        .bind(now.0)
        .bind(limit)
        .fetch_all(&self.pool)
        .await)?;
        rows.iter().map(|r| Ok((io(r.try_get("id"))?, io(r.try_get("account_id"))?))).collect()
    }

    /// Running work whose accepted attempt is older than the cutoff.
    pub async fn running_since_before(&self, cutoff_us: i64, limit: i64) -> Result<Vec<(Uuid, Uuid)>, StoreError> {
        let rows = io(sqlx::query(
            "SELECT w.id, w.account_id FROM taskr_work.works w
             JOIN taskr_work.attempts a ON a.work_id = w.id
             WHERE w.status = 'running' AND a.phase = 'accepted' AND a.accepted_us <= $1
               AND NOT EXISTS (SELECT 1 FROM taskr_work.erasures x WHERE x.account_id = w.account_id)
             ORDER BY w.created_us LIMIT $2",
        )
        .bind(cutoff_us)
        .bind(limit)
        .fetch_all(&self.pool)
        .await)?;
        rows.iter().map(|r| Ok((io(r.try_get("id"))?, io(r.try_get("account_id"))?))).collect()
    }

    /// Completed work with a transcript, created at or before the cutoff, of accounts that did not opt out.
    pub async fn retention_candidates(&self, cutoff_us: i64, limit: i64) -> Result<Vec<(Uuid, Uuid)>, StoreError> {
        let rows = io(sqlx::query(
            "SELECT w.id, w.account_id FROM taskr_work.works w
             JOIN taskr_work.enrollments e ON e.account_id = w.account_id
             WHERE w.status = 'completed' AND w.transcript IS NOT NULL AND e.auto_delete AND w.created_us <= $1
               AND NOT EXISTS (SELECT 1 FROM taskr_work.erasures x WHERE x.account_id = w.account_id)
             ORDER BY w.created_us LIMIT $2",
        )
        .bind(cutoff_us)
        .bind(limit)
        .fetch_all(&self.pool)
        .await)?;
        rows.iter().map(|r| Ok((io(r.try_get("id"))?, io(r.try_get("account_id"))?))).collect()
    }

    /// The stored binding for a callback's attempt query parameter. No account lock: this only reads.
    pub async fn binding(&self, attempt: Uuid) -> Result<Option<Binding>, StoreError> {
        let row = io(sqlx::query(
            "SELECT a.id, a.work_id, w.account_id, a.provider, a.project, a.remote_id, a.phase, a.callback_secret
             FROM taskr_work.attempts a JOIN taskr_work.works w ON w.id = a.work_id WHERE a.id = $1",
        )
        .bind(attempt)
        .fetch_optional(&self.pool)
        .await)?;
        let Some(row) = row else { return Ok(None) };
        let provider: String = io(row.try_get("provider"))?;
        let phase: String = io(row.try_get("phase"))?;
        let secret: String = io(row.try_get("callback_secret"))?;
        let corrupt = |what: &str| StoreError::Unavailable(format!("a stored attempt has an unreadable {what}"));
        Ok(Some(Binding {
            work: io(row.try_get("work_id"))?,
            account: io(row.try_get("account_id"))?,
            attempt: io(row.try_get("id"))?,
            provider: parse_provider(&provider).ok_or_else(|| corrupt("provider"))?,
            project: io(row.try_get("project"))?,
            remote_id: io(row.try_get("remote_id"))?,
            phase: AttemptPhase::parse(&phase).ok_or_else(|| corrupt("phase"))?,
            secret: SecretString::new(secret),
        }))
    }

    /// The account's works, newest first. A read: it neither creates nor refreshes anything.
    pub async fn works_of(&self, account: Uuid) -> Result<Vec<Work>, StoreError> {
        let rows = io(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {WORK_COLUMNS} FROM taskr_work.works WHERE account_id = $1 ORDER BY created_us DESC, id")))
            .bind(account)
            .fetch_all(&self.pool)
            .await)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = io(row.try_get("id"))?;
            let attempts = load_attempts(&self.pool, id).await?;
            out.push(work_from(row, attempts)?);
        }
        Ok(out)
    }

    pub async fn work(&self, account: Uuid, id: Uuid) -> Result<Option<Work>, StoreError> {
        let row = io(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {WORK_COLUMNS} FROM taskr_work.works WHERE id = $1 AND account_id = $2")))
            .bind(id)
            .bind(account)
            .fetch_optional(&self.pool)
            .await)?;
        match row {
            Some(row) => Ok(Some(work_from(&row, load_attempts(&self.pool, id).await?)?)),
            None => Ok(None),
        }
    }

    // ---- erasure ---------------------------------------------------------------

    /// Establishes the erasure barrier and nothing else: from here no create, upload link,
    /// edit or result is accepted for the account. Returns whether this call established it.
    pub async fn open_barrier(&self, account: Uuid, now: Micros) -> Result<bool, StoreError> {
        let mut tx = self.begin().await?;
        tx.lock_account_key(account).await?;
        let created = io(sqlx::query("INSERT INTO taskr_work.erasures (account_id, requested_us, state) VALUES ($1, $2, 'pending') ON CONFLICT (account_id) DO NOTHING")
            .bind(account)
            .bind(now.0)
            .execute(&mut *tx.inner)
            .await)?
        .rows_affected()
            == 1;
        tx.commit().await?;
        Ok(created)
    }

    /// `(data erased at, why it has not run)` of an account's erasure, if one was opened.
    pub async fn erasure_progress(&self, account: Uuid) -> Result<Option<(Option<Micros>, Option<String>)>, StoreError> {
        let row: Option<(Option<i64>, Option<String>)> =
            io(sqlx::query_as("SELECT data_erased_us, note FROM taskr_work.erasures WHERE account_id = $1").bind(account).fetch_optional(&self.pool).await)?;
        Ok(row.map(|(at, note)| (at.map(Micros), note)))
    }

    pub async fn set_erasure_note(&self, account: Uuid, note: &str) -> Result<(), StoreError> {
        io(sqlx::query("UPDATE taskr_work.erasures SET note = $2 WHERE account_id = $1 AND data_erased_us IS NULL").bind(account).bind(note).execute(&self.pool).await)?;
        Ok(())
    }

    /// The one combined erasure transaction. Billing's cascade is the first billing call (its
    /// account locks come before any host row lock), then the account's enrolled sample
    /// resources, then this schema's manifest and rows, all on this pool's one database: they
    /// commit together or not at all. Idempotent: a repeat after completion changes nothing.
    /// Returns whether this call erased.
    pub async fn erase_data(&self, account: Uuid, now: Micros, hold_objects_until: Micros) -> Result<bool, EraseDataError> {
        let mut tx = self.begin().await.map_err(EraseDataError::Store)?;
        io(sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED").execute(&mut *tx.inner).await).map_err(EraseDataError::Store)?;
        {
            let mut billing = PgBillingTx::attach(&mut tx.inner).await.map_err(|e| EraseDataError::Store(StoreError::Unavailable(e.to_string())))?;
            match erase_account_billing(&mut billing, AccountId(account), &[], BillingMicros(now.0)).await {
                Ok(_) => {}
                Err(ErasureError::Store(e)) => return Err(EraseDataError::Store(StoreError::Unavailable(e.to_string()))),
                Err(other) => return Err(EraseDataError::Blocked(other.to_string())),
            }
        }
        taskr_billing::erase_enrolled_sample_resources(&mut tx.inner, AccountId(account)).await.map_err(|e| EraseDataError::Store(StoreError::from(e)))?;
        tx.lock_account_key(account).await.map_err(EraseDataError::Store)?;
        let done: Option<Option<i64>> = io(sqlx::query_scalar("SELECT data_erased_us FROM taskr_work.erasures WHERE account_id = $1 FOR UPDATE").bind(account).fetch_optional(&mut *tx.inner).await)
            .map_err(EraseDataError::Store)?;
        match done {
            None => return Err(EraseDataError::Blocked("the erasure barrier was not opened".into())),
            Some(Some(_)) => {
                tx.commit().await.map_err(EraseDataError::Store)?;
                return Ok(false);
            }
            Some(None) => {}
        }
        let rows = io(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {WORK_COLUMNS} FROM taskr_work.works WHERE account_id = $1 ORDER BY created_us, id")))
            .bind(account)
            .fetch_all(&mut *tx.inner)
            .await)
        .map_err(EraseDataError::Store)?;
        for row in &rows {
            let id: Uuid = io(row.try_get("id")).map_err(EraseDataError::Store)?;
            let attempts = load_attempts_tx(&mut tx.inner, id).await.map_err(EraseDataError::Store)?;
            let work = work_from(row, attempts).map_err(EraseDataError::Store)?;
            let resources: Resources = work.resources();
            // Two resources for the audio. The generation the upload was confirmed at is deleted by
            // name, so an older version is never abandoned; that deletion is final. The key itself is
            // deleted as whatever is live and swept again after the outstanding upload links have
            // expired, and then stays an open, explicit resource: an upload accepted before the expiry
            // can still commit a newer generation that no earlier delete could have named, and an
            // absent key proves nothing until a provider bound or write fence does.
            if resources.object.version.is_some() {
                tx.add_resource(account, "object", serde_json::to_value(&resources.object).unwrap_or_default(), None).await.map_err(EraseDataError::Store)?;
            }
            let mut current = resources.object.clone();
            current.version = None;
            tx.add_resource(account, "object", serde_json::to_value(&current).unwrap_or_default(), Some(hold_objects_until)).await.map_err(EraseDataError::Store)?;
            for remote in &resources.remotes {
                tx.add_resource(account, "remote", serde_json::to_value(remote).unwrap_or_default(), None).await.map_err(EraseDataError::Store)?;
            }
            let projects: Vec<(Uuid, String)> = io(sqlx::query_as("SELECT id, project FROM taskr_work.attempts WHERE work_id = $1").bind(id).fetch_all(&mut *tx.inner).await).map_err(EraseDataError::Store)?;
            for attempt in work.attempts.iter().filter(|a| a.remote.is_none() && !matches!(a.phase, AttemptPhase::Rejected)) {
                let project = projects.iter().find(|(attempt_id, _)| *attempt_id == attempt.id).map(|(_, p)| p.clone()).unwrap_or_default();
                // Everything the attempt sent, so a listing can be correlated after these rows are gone.
                let target = serde_json::json!({
                    "work": work.id,
                    "attempt": attempt.id,
                    "provider": provider_str(attempt.provider),
                    "project": project,
                    "reference": reference_for(work.id),
                    "object_key": work.object.key,
                    "claimed_us": attempt.claimed_at.0,
                    "lease_until_us": attempt.lease_until.0,
                });
                tx.add_resource(account, "unidentified", target, None).await.map_err(EraseDataError::Store)?;
            }
        }
        // Keep only minimal external-processing facts, never a summary/transcript/prompt copy.
        // A successful completion is not evidence of provider deletion or non-retention.
        io(sqlx::query(
            "INSERT INTO taskr_work.erasure_resources (account_id, kind, target)
             SELECT $1, 'llm', jsonb_build_object('generation', g.id, 'provider', g.payload->'provider', 'remote_ids', g.payload->'remote_ids')
             FROM taskr_work.summary_generations g
             WHERE g.account_id = $1 AND g.payload->>'provider' IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM taskr_work.erasure_resources r WHERE r.account_id = $1 AND r.kind = 'llm' AND r.target->>'generation' = g.id::text)",
        ).bind(account).execute(&mut *tx.inner).await).map_err(EraseDataError::Store)?;
        // Deleting works cascades to generations, staged finals and saved summaries atomically.
        for statement in [
            "DELETE FROM taskr_work.attempts WHERE work_id IN (SELECT id FROM taskr_work.works WHERE account_id = $1)",
            "DELETE FROM taskr_work.works WHERE account_id = $1",
            "DELETE FROM taskr_work.enrollments WHERE account_id = $1",
        ] {
            io(sqlx::query(statement).bind(account).execute(&mut *tx.inner).await).map_err(EraseDataError::Store)?;
        }
        io(sqlx::query("UPDATE taskr_work.erasures SET data_erased_us = $2, note = NULL WHERE account_id = $1").bind(account).bind(now.0).execute(&mut *tx.inner).await).map_err(EraseDataError::Store)?;
        tx.commit().await.map_err(EraseDataError::Store)?;
        Ok(true)
    }

    /// Remote work learned after the account's erasure began (a submission answered after the
    /// barrier). While the data is still there the attempt row gets the identity, so the manifest
    /// takes it as a known remote; once the manifest exists the matching unidentified resource
    /// becomes that remote (or a new resource is added) and a closed erasure is reopened, so the
    /// saga deletes it and keeps retrying until the provider confirms.
    pub async fn record_late_remote(&self, account: Uuid, attempt: Uuid, remote: &RemoteIdentity) -> Result<(), StoreError> {
        let mut tx = self.begin().await?;
        tx.lock_account_key(account).await?;
        let progress: Option<Option<i64>> =
            io(sqlx::query_scalar("SELECT data_erased_us FROM taskr_work.erasures WHERE account_id = $1 FOR UPDATE").bind(account).fetch_optional(&mut *tx.inner).await)?;
        match progress {
            None => {}
            Some(None) => {
                io(sqlx::query("UPDATE taskr_work.attempts SET remote_id = $2 WHERE id = $1 AND remote_id IS NULL").bind(attempt).bind(&remote.id).execute(&mut *tx.inner).await)?;
            }
            Some(Some(_)) => {
                let converted = io(sqlx::query(
                    "UPDATE taskr_work.erasure_resources SET kind = 'remote', target = $3, state = 'pending', last_error = NULL
                     WHERE account_id = $1 AND kind = 'unidentified' AND target->>'attempt' = $2",
                )
                .bind(account)
                .bind(attempt.to_string())
                .bind(Json(remote))
                .execute(&mut *tx.inner)
                .await)?
                .rows_affected();
                if converted == 0 {
                    // Already a resource if a listing found it first: never twice.
                    io(sqlx::query(
                        "INSERT INTO taskr_work.erasure_resources (account_id, kind, target)
                         SELECT $1, 'remote', $2::jsonb
                         WHERE NOT EXISTS (SELECT 1 FROM taskr_work.erasure_resources WHERE account_id = $1 AND kind = 'remote' AND target = $2::jsonb)",
                    )
                    .bind(account)
                    .bind(Json(remote))
                    .execute(&mut *tx.inner)
                    .await)?;
                }
                io(sqlx::query("UPDATE taskr_work.erasures SET state = 'pending', finished_us = NULL WHERE account_id = $1 AND state = 'complete'").bind(account).execute(&mut *tx.inner).await)?;
            }
        }
        tx.commit().await
    }

    /// Every account whose erasure is not finished, the least recently tried first.
    pub async fn pending_erasures(&self, limit: i64) -> Result<Vec<Uuid>, StoreError> {
        io(sqlx::query_scalar("SELECT account_id FROM taskr_work.erasures WHERE state = 'pending' ORDER BY COALESCE(last_advanced_us, 0), requested_us LIMIT $1").bind(limit).fetch_all(&self.pool).await)
    }

    /// Moves an erasure to the back of the rotation.
    pub async fn touch_erasure(&self, account: Uuid, now: Micros) -> Result<(), StoreError> {
        io(sqlx::query("UPDATE taskr_work.erasures SET last_advanced_us = $2 WHERE account_id = $1").bind(account).bind(now.0).execute(&self.pool).await)?;
        Ok(())
    }

    pub async fn erasure_resources(&self, account: Uuid) -> Result<Vec<ErasureResource>, StoreError> {
        let rows = io(sqlx::query(
            "SELECT id, account_id, kind, target, state, blocked_until_us, last_error FROM taskr_work.erasure_resources WHERE account_id = $1 ORDER BY id",
        )
        .bind(account)
        .fetch_all(&self.pool)
        .await)?;
        rows.iter()
            .map(|row| {
                let target: Json<serde_json::Value> = io(row.try_get("target"))?;
                Ok(ErasureResource {
                    id: io(row.try_get("id"))?,
                    account: io(row.try_get("account_id"))?,
                    kind: io(row.try_get("kind"))?,
                    target: target.0,
                    state: io(row.try_get("state"))?,
                    blocked_until: io::<Option<i64>>(row.try_get("blocked_until_us"))?.map(Micros),
                    last_error: io(row.try_get("last_error"))?,
                })
            })
            .collect()
    }

    pub async fn set_resource(&self, id: i64, state: &str, blocked_until: Option<Micros>, last_error: Option<&str>) -> Result<(), StoreError> {
        io(sqlx::query("UPDATE taskr_work.erasure_resources SET state = $2, blocked_until_us = $3, last_error = $4 WHERE id = $1")
            .bind(id)
            .bind(state)
            .bind(blocked_until.map(|m| m.0))
            .bind(last_error)
            .execute(&self.pool)
            .await)?;
        Ok(())
    }

    /// A resource that was an unidentified attempt, now settled by a listing: the unresolved note only
    /// applies while it still is one. A late acknowledgement may have turned it into a known remote
    /// while the listing was in flight, and that remote must not be parked as unresolved.
    pub async fn set_unidentified(&self, id: i64, state: &str, last_error: Option<&str>) -> Result<(), StoreError> {
        io(sqlx::query("UPDATE taskr_work.erasure_resources SET state = $2, last_error = $3 WHERE id = $1 AND kind = 'unidentified'")
            .bind(id)
            .bind(state)
            .bind(last_error)
            .execute(&self.pool)
            .await)?;
        Ok(())
    }

    /// Registers every listed job that matched an unidentified attempt as remote work to delete, under
    /// the account lock every other writer of the manifest takes (`finish_erasure`,
    /// `record_late_remote`). The unidentified row becomes the first match if it still is one; if a
    /// late acknowledgement already converted it, another worker already deleted and removed it, or
    /// the erasure already closed, every match is still registered (once) and a closed erasure is
    /// reopened so the saga deletes them. A listing's result is never discarded.
    pub async fn identify_resource(&self, account: Uuid, id: i64, remotes: &[RemoteIdentity]) -> Result<(), StoreError> {
        let Some((first, rest)) = remotes.split_first() else { return Ok(()) };
        let mut tx = self.begin().await?;
        tx.lock_account_key(account).await?;
        let barrier: Option<i32> = io(sqlx::query_scalar("SELECT 1 FROM taskr_work.erasures WHERE account_id = $1").bind(account).fetch_optional(&mut *tx.inner).await)?;
        if barrier.is_none() {
            return tx.rollback().await;
        }
        let converted = io(sqlx::query(
            "UPDATE taskr_work.erasure_resources SET kind = 'remote', target = $3, state = 'pending', last_error = NULL
             WHERE id = $1 AND account_id = $2 AND kind = 'unidentified'",
        )
        .bind(id)
        .bind(account)
        .bind(Json(first))
        .execute(&mut *tx.inner)
        .await)?
        .rows_affected()
            == 1;
        let to_add: Vec<&RemoteIdentity> = if converted { rest.iter().collect() } else { remotes.iter().collect() };
        for remote in to_add {
            io(sqlx::query(
                "INSERT INTO taskr_work.erasure_resources (account_id, kind, target)
                 SELECT $1, 'remote', $2::jsonb
                 WHERE NOT EXISTS (SELECT 1 FROM taskr_work.erasure_resources WHERE account_id = $1 AND kind = 'remote' AND target = $2::jsonb)",
            )
            .bind(account)
            .bind(Json(remote))
            .execute(&mut *tx.inner)
            .await)?;
        }
        io(sqlx::query("UPDATE taskr_work.erasures SET state = 'pending', finished_us = NULL WHERE account_id = $1 AND state = 'complete'").bind(account).execute(&mut *tx.inner).await)?;
        tx.commit().await
    }

    /// Completes the erasure only when every resource is done or confirmed absent. The resource
    /// details are then dropped; the barrier row stays, with its outcome. Called only from the
    /// erasure driver beneath its per-account session lock; the account lock taken here serializes it
    /// with `record_late_remote` and `identify_resource`, which reopen a closed erasure.
    pub async fn finish_erasure(&self, account: Uuid, now: Micros) -> Result<bool, StoreError> {
        let mut tx = self.begin().await?;
        tx.lock_account_key(account).await?;
        let open: i64 = io(sqlx::query_scalar("SELECT count(*) FROM taskr_work.erasure_resources WHERE account_id = $1 AND state NOT IN ('done', 'absent')").bind(account).fetch_one(&mut *tx.inner).await)?;
        let erased: Option<Option<i64>> = io(sqlx::query_scalar("SELECT data_erased_us FROM taskr_work.erasures WHERE account_id = $1").bind(account).fetch_optional(&mut *tx.inner).await)?;
        if open > 0 || !matches!(erased, Some(Some(_))) {
            tx.rollback().await?;
            return Ok(false);
        }
        io(sqlx::query("DELETE FROM taskr_work.erasure_resources WHERE account_id = $1").bind(account).execute(&mut *tx.inner).await)?;
        io(sqlx::query("UPDATE taskr_work.erasures SET state = 'complete', finished_us = $2 WHERE account_id = $1 AND state = 'pending'").bind(account).bind(now.0).execute(&mut *tx.inner).await)?;
        tx.commit().await?;
        Ok(true)
    }
}

/// The reference a provider listing is searched for to find an attempt whose answer was lost.
pub fn reference_for(work: Uuid) -> String {
    format!("work_{}", work.simple())
}

async fn load_attempts(pool: &PgPool, work: Uuid) -> Result<Vec<Attempt>, StoreError> {
    let rows = io(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {ATTEMPT_COLUMNS} FROM taskr_work.attempts WHERE work_id = $1 ORDER BY number"))).bind(work).fetch_all(pool).await)?;
    rows.iter().map(attempt_from).collect()
}

async fn load_attempts_tx(tx: &mut Transaction<'static, Postgres>, work: Uuid) -> Result<Vec<Attempt>, StoreError> {
    let rows = io(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {ATTEMPT_COLUMNS} FROM taskr_work.attempts WHERE work_id = $1 ORDER BY number"))).bind(work).fetch_all(&mut **tx).await)?;
    rows.iter().map(attempt_from).collect()
}

impl Tx {
    pub async fn commit(self) -> Result<(), StoreError> {
        io(self.inner.commit().await)
    }

    pub async fn rollback(self) -> Result<(), StoreError> {
        io(self.inner.rollback().await)
    }

    /// The account's own lock for this transaction. Everything else comes after it.
    async fn lock_account_key(&mut self, account: Uuid) -> Result<(), StoreError> {
        io(sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(format!("taskr_work:{account}")).execute(&mut *self.inner).await)?;
        Ok(())
    }

    async fn barrier(&mut self, account: Uuid) -> Result<bool, StoreError> {
        let found: Option<i32> = io(sqlx::query_scalar("SELECT 1 FROM taskr_work.erasures WHERE account_id = $1").bind(account).fetch_optional(&mut *self.inner).await)?;
        Ok(found.is_some())
    }

    /// The account lock, then the barrier, then the enrollment. An erased account never passes.
    pub async fn lock_account(&mut self, account: Uuid) -> Result<Enrollment, StoreError> {
        self.lock_account_key(account).await?;
        if self.barrier(account).await? {
            return Err(StoreError::Erased);
        }
        let row = io(sqlx::query("SELECT account_id, email, enrolled_us, auto_delete FROM taskr_work.enrollments WHERE account_id = $1").bind(account).fetch_optional(&mut *self.inner).await)?;
        match row {
            Some(row) => enrollment(&row),
            None => Err(StoreError::NotEnrolled),
        }
    }

    /// Enrolls once. A repeat keeps the first enrollment time and refreshes the address.
    pub async fn enroll(&mut self, account: Uuid, email: &str, now: Micros) -> Result<Enrollment, StoreError> {
        self.lock_account_key(account).await?;
        if self.barrier(account).await? {
            return Err(StoreError::Erased);
        }
        io(sqlx::query(
            "INSERT INTO taskr_work.enrollments (account_id, email, enrolled_us) VALUES ($1, $2, $3)
             ON CONFLICT (account_id) DO UPDATE SET email = EXCLUDED.email",
        )
        .bind(account)
        .bind(email)
        .bind(now.0)
        .execute(&mut *self.inner)
        .await)?;
        self.lock_account(account).await
    }

    pub async fn set_auto_delete(&mut self, account: Uuid, auto_delete: bool) -> Result<(), StoreError> {
        io(sqlx::query("UPDATE taskr_work.enrollments SET auto_delete = $2 WHERE account_id = $1").bind(account).bind(auto_delete).execute(&mut *self.inner).await)?;
        Ok(())
    }

    pub async fn insert_work(&mut self, work: &Work) -> Result<(), StoreError> {
        io(sqlx::query(
            "INSERT INTO taskr_work.works (id, account_id, object, name, language, requested, status, revision, transcript, error, created_us, completed_us)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(work.id)
        .bind(work.owner)
        .bind(Json(&work.object))
        .bind(&work.name)
        .bind(&work.language)
        .bind(work.requested.map(provider_str))
        .bind(work.status.as_str())
        .bind(work.revision as i64)
        .bind(&work.transcript)
        .bind(&work.error)
        .bind(work.created_at.0)
        .bind(work.completed_at.map(|m| m.0))
        .execute(&mut *self.inner)
        .await)?;
        Ok(())
    }

    pub async fn load_work(&mut self, id: Uuid) -> Result<Option<Work>, StoreError> {
        let row = io(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {WORK_COLUMNS} FROM taskr_work.works WHERE id = $1 FOR UPDATE"))).bind(id).fetch_optional(&mut *self.inner).await)?;
        match row {
            Some(row) => Ok(Some(work_from(&row, load_attempts_tx(&mut self.inner, id).await?)?)),
            None => Ok(None),
        }
    }

    /// Saves the work if its revision is still `expected`; stores the secret of an attempt that was
    /// just claimed. A lost compare-and-swap is a conflict, never an overwrite.
    pub async fn save_work(&mut self, work: &Work, expected: u64, secrets: &[NewSecret<'_>]) -> Result<(), StoreError> {
        let updated = io(sqlx::query(
            "UPDATE taskr_work.works SET object = $3, status = $4, revision = $5, transcript = $6, error = $7, completed_us = $8
             WHERE id = $1 AND revision = $2",
        )
        .bind(work.id)
        .bind(expected as i64)
        .bind(Json(&work.object))
        .bind(work.status.as_str())
        .bind(work.revision as i64)
        .bind(&work.transcript)
        .bind(&work.error)
        .bind(work.completed_at.map(|m| m.0))
        .execute(&mut *self.inner)
        .await)?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        for attempt in &work.attempts {
            let new = secrets.iter().find(|s| s.attempt == attempt.id);
            let secret = new.map(|s| s.secret.expose().to_string());
            let project = new.map(|s| s.project.to_string()).or_else(|| attempt.remote.as_ref().map(|r| r.project.clone()));
            io(sqlx::query(
                "INSERT INTO taskr_work.attempts (id, work_id, number, provider, project, phase, lease_until_us, claimed_us, accepted_us, remote_id, reason, callback_secret, result_digest)
                 VALUES ($1, $2, $3, $4, COALESCE($5, ''), $6, $7, $8, $9, $10, $11, COALESCE($12, ''), $13)
                 ON CONFLICT (id) DO UPDATE SET
                   project = CASE WHEN EXCLUDED.project <> '' THEN EXCLUDED.project ELSE taskr_work.attempts.project END,
                   phase = EXCLUDED.phase, lease_until_us = EXCLUDED.lease_until_us, accepted_us = EXCLUDED.accepted_us,
                   remote_id = EXCLUDED.remote_id, reason = EXCLUDED.reason, result_digest = EXCLUDED.result_digest",
            )
            .bind(attempt.id)
            .bind(work.id)
            .bind(attempt.number as i32)
            .bind(provider_str(attempt.provider))
            .bind(project)
            .bind(attempt.phase.as_str())
            .bind(attempt.lease_until.0)
            .bind(attempt.claimed_at.0)
            .bind(attempt.accepted_at.map(|m| m.0))
            .bind(attempt.remote.as_ref().map(|r| r.id.clone()))
            .bind(&attempt.reason)
            .bind(secret)
            .bind(&attempt.result_digest)
            .execute(&mut *self.inner)
            .await)?;
        }
        // Retention (or any save of completed work without its source) clears non-canonical
        // summary text in this same transaction (lead decision D10).
        if work.status == Status::Completed && work.transcript.is_none() {
            self.purge_summary_source_content(work.id).await?;
        }
        Ok(())
    }

    async fn add_resource(&mut self, account: Uuid, kind: &str, target: serde_json::Value, blocked_until: Option<Micros>) -> Result<(), StoreError> {
        io(sqlx::query("INSERT INTO taskr_work.erasure_resources (account_id, kind, target, blocked_until_us) VALUES ($1, $2, $3, $4)")
            .bind(account)
            .bind(kind)
            .bind(Json(target))
            .bind(blocked_until.map(|m| m.0))
            .execute(&mut *self.inner)
            .await)?;
        Ok(())
    }
}
