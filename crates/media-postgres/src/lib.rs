//! Shared PostgreSQL scheduling primitives for media work, used by Transgate
//! and by the taskr audio-work sample, each on its own database.
//!
//! - [`try_lock`]: a session advisory lock held on a dedicated connection. A
//!   crashed holder's connection closes and the server releases the lock.
//! - [`claim_slot`] / [`renew_slot`] / [`finish_slot`]: one run per
//!   `(schedule, UTC slot)`, claimable once, reclaimed after its lease expires
//!   (an interrupted run), never again once finished.
//!
//! Schema `media`, history `media._sqlx_migrations`. The runtime authority is a
//! role the host names in [`grant_runtime`]; this crate depends on no host and
//! no billing module. Instants are UTC microseconds since the Unix epoch.

use std::fmt;

use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool, Row};
use uuid::Uuid;

pub const SCHEMA: &str = "media";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaStoreError {
    /// The database could not be reached or a statement failed.
    Unavailable(String),
    /// An argument or role does not satisfy this crate's contract.
    Invalid(String),
}

impl fmt::Display for MediaStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaStoreError::Unavailable(e) => write!(f, "media store unavailable: {e}"),
            MediaStoreError::Invalid(e) => write!(f, "media store: {e}"),
        }
    }
}

impl std::error::Error for MediaStoreError {}

fn unavailable(e: impl fmt::Display) -> MediaStoreError {
    MediaStoreError::Unavailable(e.to_string())
}

/// Applies the media migrations into schema `media` with their own history,
/// beside any host or billing history. Owner authority only.
pub async fn migrate(owner: &PgPool) -> Result<(), MediaStoreError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.create_schema(SCHEMA);
    migrator.dangerous_set_table_name("media._sqlx_migrations");
    migrator.run(owner).await.map_err(|e| unavailable(format!("media migration failed: {e}")))
}

fn valid_role(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.starts_with("pg_")
}

/// Columns of `media.schedule_slots` the runtime may change. The slot identity
/// (`schedule`, `slot_at`) is written once, by the claiming insert.
const SLOT_UPDATE: [&str; 6] = ["attempts", "claim_token", "claimed_until", "started_at", "finished_at", "outcome"];

/// Grants `role` exactly what a running service needs here: claim, renew and
/// finish slots, and purge finished runs (row security hides unfinished ones
/// from its DELETE). No DDL. Owner authority only; `role` must exist.
pub async fn grant_runtime(owner: &PgPool, role: &str) -> Result<(), MediaStoreError> {
    if !valid_role(role) {
        return Err(MediaStoreError::Invalid(format!("{role:?} is not a usable role name")));
    }
    let columns = SLOT_UPDATE.join(", ");
    // `role` is restricted to [a-z0-9_] above.
    for statement in [
        format!("GRANT USAGE ON SCHEMA media TO \"{role}\""),
        format!("GRANT EXECUTE ON FUNCTION media.us_to_ts(bigint), media.ts_to_us(timestamptz) TO \"{role}\""),
        format!("GRANT SELECT, INSERT, DELETE ON media.schedule_slots TO \"{role}\""),
        format!("GRANT UPDATE ({columns}) ON media.schedule_slots TO \"{role}\""),
    ] {
        sqlx::query(AssertSqlSafe(statement)).execute(owner).await.map_err(unavailable)?;
    }
    Ok(())
}

/// What `role` can do in schema `media` beyond the runtime contract: schema
/// creation or ownership, table ownership, changing a slot's identity, any
/// TRUNCATE, TRIGGER or REFERENCES, and a DELETE that row security does not
/// restrict to finished runs. Empty means within the contract.
pub async fn runtime_authority_problems(pool: &PgPool, role: &str) -> Result<Vec<String>, MediaStoreError> {
    let mut problems = Vec::new();
    let schema: Option<(bool, bool)> = sqlx::query_as(
        "SELECT has_schema_privilege($1::name, 'media', 'CREATE'),
                COALESCE((SELECT pg_get_userbyid(nspowner)::text = $1 FROM pg_namespace WHERE nspname = 'media'), false)",
    )
    .bind(role)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    if let Some((can_create, owns)) = schema {
        if can_create {
            problems.push("can create objects in schema media".into());
        }
        if owns {
            problems.push("owns schema media".into());
        }
    }
    let owned: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'media' AND c.relkind = 'r' AND pg_get_userbyid(c.relowner)::text = $1",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    problems.extend(owned.into_iter().map(|t| format!("owns media.{t}")));
    let identity_updates: Vec<String> = sqlx::query_scalar(
        "SELECT a.attname::text FROM pg_attribute a
         WHERE a.attrelid = 'media.schedule_slots'::regclass AND a.attname IN ('schedule', 'slot_at')
           AND has_column_privilege($1::name, a.attrelid, a.attnum, 'UPDATE')",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    problems.extend(identity_updates.into_iter().map(|c| format!("can UPDATE media.schedule_slots.{c}")));
    let table_rights: Vec<String> = sqlx::query_scalar(
        "SELECT p.priv FROM (VALUES ('DELETE'), ('TRUNCATE'), ('TRIGGER'), ('REFERENCES')) AS p(priv)
         WHERE has_table_privilege($1::name, 'media.schedule_slots'::regclass, p.priv)",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    for privilege in table_rights {
        if privilege == "DELETE" && delete_limited_to_finished(pool, role).await? {
            continue;
        }
        problems.push(format!("can {privilege} media.schedule_slots"));
    }
    Ok(problems)
}

/// True when row security limits every DELETE `role` can make on
/// `media.schedule_slots` to finished runs: enabled, the role does not bypass
/// it, and each permissive policy that applies to DELETE says exactly that.
async fn delete_limited_to_finished(pool: &PgPool, role: &str) -> Result<bool, MediaStoreError> {
    let (enabled, bypass): (bool, bool) = sqlx::query_as(
        "SELECT c.relrowsecurity, COALESCE((SELECT rolbypassrls FROM pg_roles WHERE rolname = $1), false)
         FROM pg_class c WHERE c.oid = 'media.schedule_slots'::regclass",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .map_err(unavailable)?;
    if !enabled || bypass {
        return Ok(false);
    }
    let quals: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT qual FROM pg_policies
         WHERE schemaname = 'media' AND tablename = 'schedule_slots' AND cmd IN ('DELETE', 'ALL')
           AND permissive = 'PERMISSIVE'",
    )
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    Ok(!quals.is_empty() && quals.iter().all(|q| q.as_deref() == Some("(finished_at IS NOT NULL)")))
}

fn check_history_args(schedules: &[&str]) -> Result<(), MediaStoreError> {
    if schedules.is_empty() || schedules.iter().any(|s| s.is_empty()) {
        return Err(MediaStoreError::Invalid("history purge needs the caller's schedule names".into()));
    }
    Ok(())
}

/// Finished runs of the named `schedules` whose run started at or before
/// `started_at_or_before_us`, counted inside the caller's transaction. A
/// pending or running claim is never counted.
pub async fn count_finished_slots(
    conn: &mut PgConnection,
    schedules: &[&str],
    started_at_or_before_us: i64,
) -> Result<u64, MediaStoreError> {
    check_history_args(schedules)?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.schedule_slots
         WHERE schedule = ANY($1) AND finished_at IS NOT NULL AND started_at <= media.us_to_ts($2)",
    )
    .bind(schedules)
    .bind(started_at_or_before_us)
    .fetch_one(&mut *conn)
    .await
    .map_err(unavailable)?;
    Ok(n as u64)
}

/// Deletes exactly the runs [`count_finished_slots`] counts, inside the
/// caller's transaction, so the purge and its audit row commit together.
pub async fn delete_finished_slots(
    conn: &mut PgConnection,
    schedules: &[&str],
    started_at_or_before_us: i64,
) -> Result<u64, MediaStoreError> {
    check_history_args(schedules)?;
    let result = sqlx::query(
        "DELETE FROM media.schedule_slots
         WHERE schedule = ANY($1) AND finished_at IS NOT NULL AND started_at <= media.us_to_ts($2)",
    )
    .bind(schedules)
    .bind(started_at_or_before_us)
    .execute(&mut *conn)
    .await
    .map_err(unavailable)?;
    Ok(result.rows_affected())
}

// ---- advisory locks ---------------------------------------------------------

/// A 64-bit advisory lock key. Derive one from a stable name so every replica
/// and every process computes the same key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LockKey(pub i64);

/// Namespace for [`LockKey::named`]: keys of this crate never collide with an
/// ad hoc integer key chosen elsewhere unless that key was derived the same way.
const LOCK_NAMESPACE: Uuid = Uuid::from_u128(0x6d65_6469_612d_6c6f_636b_2d6b_6579_7331);

impl LockKey {
    pub fn named(name: &str) -> LockKey {
        LockKey(Uuid::new_v5(&LOCK_NAMESPACE, name.as_bytes()).as_u128() as i64)
    }
}

/// A held session advisory lock. It lives on a connection taken out of the
/// pool for good, so neither a returned pooled connection nor a crash can leave
/// it held: dropping this closes the connection and the server releases it.
pub struct HeldLock {
    conn: PgConnection,
    key: LockKey,
}

/// Tries once to take `key` without waiting. `None` means another session
/// holds it.
pub async fn try_lock(pool: &PgPool, key: LockKey) -> Result<Option<HeldLock>, MediaStoreError> {
    let mut conn = pool.acquire().await.map_err(unavailable)?.detach();
    let taken: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(key.0)
        .fetch_one(&mut conn)
        .await
        .map_err(unavailable)?;
    if taken {
        Ok(Some(HeldLock { conn, key }))
    } else {
        let _ = conn.close().await;
        Ok(None)
    }
}

impl HeldLock {
    pub fn key(&self) -> LockKey {
        self.key
    }

    /// True while this session still holds the lock. A broken connection has
    /// lost it, whatever happened on the server.
    pub async fn still_held(&mut self) -> bool {
        let held: Result<bool, _> = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks
                            WHERE locktype = 'advisory' AND pid = pg_backend_pid() AND granted
                              AND objsubid = 1
                              AND ((classid::bigint << 32) | objid::bigint) = $1)",
        )
        .bind(self.key.0)
        .fetch_one(&mut self.conn)
        .await;
        held.unwrap_or(false)
    }

    /// Releases the lock and closes its connection.
    pub async fn release(mut self) -> Result<(), MediaStoreError> {
        let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
            .bind(self.key.0)
            .fetch_one(&mut self.conn)
            .await
            .map_err(unavailable)?;
        let _ = self.conn.close().await;
        if released { Ok(()) } else { Err(MediaStoreError::Invalid("the advisory lock was not held".into())) }
    }
}

// ---- schedule slots -----------------------------------------------------------

/// A claimed run of one `(schedule, slot)`. Every change to it is fenced by
/// `token`, so a run whose lease expired and was reclaimed cannot finish it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotClaim {
    pub schedule: String,
    pub slot_at_us: i64,
    pub token: Uuid,
    /// 1 for the first run of this slot, 2 after one interrupted run, and so on.
    pub attempt: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotState {
    pub attempts: i32,
    pub claimed_until_us: Option<i64>,
    pub finished_at_us: Option<i64>,
    pub outcome: Option<String>,
}

fn check_slot_args(schedule: &str, lease_seconds: Option<i64>) -> Result<(), MediaStoreError> {
    if schedule.is_empty() {
        return Err(MediaStoreError::Invalid("a schedule needs a name".into()));
    }
    if lease_seconds.is_some_and(|l| l <= 0) {
        return Err(MediaStoreError::Invalid("a slot lease must be positive".into()));
    }
    Ok(())
}

/// Claims the run of `schedule` for the UTC instant `slot_at_us` at `now_us`,
/// leased for `lease_seconds`. `None` when that slot is finished or another
/// run's lease is still active. A slot whose earlier run stopped without
/// finishing is reclaimed once its lease has expired.
pub async fn claim_slot(
    pool: &PgPool,
    schedule: &str,
    slot_at_us: i64,
    now_us: i64,
    lease_seconds: i64,
) -> Result<Option<SlotClaim>, MediaStoreError> {
    check_slot_args(schedule, Some(lease_seconds))?;
    let token = Uuid::new_v4();
    let row = sqlx::query(
        "INSERT INTO media.schedule_slots AS s (schedule, slot_at, attempts, claim_token, claimed_until, started_at)
         VALUES ($1, media.us_to_ts($2), 1, $3, media.us_to_ts($4) + $5::bigint * interval '1 second', media.us_to_ts($4))
         ON CONFLICT (schedule, slot_at) DO UPDATE
           SET attempts = s.attempts + 1,
               claim_token = EXCLUDED.claim_token,
               claimed_until = EXCLUDED.claimed_until,
               started_at = EXCLUDED.started_at
           WHERE s.finished_at IS NULL AND (s.claimed_until IS NULL OR s.claimed_until <= media.us_to_ts($4))
         RETURNING s.attempts",
    )
    .bind(schedule)
    .bind(slot_at_us)
    .bind(token)
    .bind(now_us)
    .bind(lease_seconds)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    match row {
        Some(row) => Ok(Some(SlotClaim {
            schedule: schedule.to_string(),
            slot_at_us,
            token,
            attempt: row.try_get("attempts").map_err(unavailable)?,
        })),
        None => Ok(None),
    }
}

/// Extends a live claim. False when the lease already expired or the slot was
/// reclaimed or finished: the caller must stop.
pub async fn renew_slot(pool: &PgPool, claim: &SlotClaim, now_us: i64, lease_seconds: i64) -> Result<bool, MediaStoreError> {
    check_slot_args(&claim.schedule, Some(lease_seconds))?;
    let result = sqlx::query(
        "UPDATE media.schedule_slots
         SET claimed_until = media.us_to_ts($4) + $5::bigint * interval '1 second'
         WHERE schedule = $1 AND slot_at = media.us_to_ts($2) AND claim_token = $3
           AND finished_at IS NULL AND claimed_until > media.us_to_ts($4)",
    )
    .bind(&claim.schedule)
    .bind(claim.slot_at_us)
    .bind(claim.token)
    .bind(now_us)
    .bind(lease_seconds)
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(result.rows_affected() == 1)
}

/// Records the final outcome of a claimed run (for example `succeeded`,
/// `failed`, `skipped`, `aborted`: the product chooses). False when this claim
/// no longer owns the slot. A finished slot is never claimed again.
pub async fn finish_slot(pool: &PgPool, claim: &SlotClaim, now_us: i64, outcome: &str) -> Result<bool, MediaStoreError> {
    check_slot_args(&claim.schedule, None)?;
    if outcome.is_empty() {
        return Err(MediaStoreError::Invalid("a finished slot needs an outcome".into()));
    }
    let result = sqlx::query(
        "UPDATE media.schedule_slots
         SET finished_at = media.us_to_ts($4), outcome = $5, claimed_until = NULL
         WHERE schedule = $1 AND slot_at = media.us_to_ts($2) AND claim_token = $3 AND finished_at IS NULL",
    )
    .bind(&claim.schedule)
    .bind(claim.slot_at_us)
    .bind(claim.token)
    .bind(now_us)
    .bind(outcome)
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(result.rows_affected() == 1)
}

/// The stored state of one slot, if it was ever claimed.
pub async fn slot_state(pool: &PgPool, schedule: &str, slot_at_us: i64) -> Result<Option<SlotState>, MediaStoreError> {
    type StateRow = (i32, Option<i64>, Option<i64>, Option<String>);
    let row: Option<StateRow> = sqlx::query_as(
        "SELECT attempts, media.ts_to_us(claimed_until), media.ts_to_us(finished_at), outcome
         FROM media.schedule_slots WHERE schedule = $1 AND slot_at = media.us_to_ts($2)",
    )
    .bind(schedule)
    .bind(slot_at_us)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    Ok(row.map(|(attempts, claimed_until_us, finished_at_us, outcome)| SlotState {
        attempts,
        claimed_until_us,
        finished_at_us,
        outcome,
    }))
}
