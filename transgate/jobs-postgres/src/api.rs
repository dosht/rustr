//! P7 persistence in schema `transgate_api` (docs/plans/p7-api/decisions.md):
//! - [`ApiTx`] on the same connection and transaction as the `PgJobUow` that lends
//!   it, so key and idempotency writes share the billing account lock and the
//!   erasure barrier of every other account write. Nothing here locks the account.
//! - [`PgApiRateLimiter`], D3: two aligned windows consumed together, never half.
//! - [`PgCallbackOutbox`], D5: leased, token-fenced delivery rows with an attempt
//!   row committed before every POST.

use billing_core::ports::StoreError;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::PgPool;
use transgate_core::api::KeyId;
use transgate_core::api::callbacks::{CallbackDelivery, DeliveryId, DeliveryState, Settlement};
use transgate_core::api::idempotency::IdempotencyRecord;
use transgate_core::api::keys::ApiKey;
use transgate_core::api::limits::{RateCounts, RateLimits, RateWindows};
use transgate_core::api::ports::{ApiRateLimiter, ApiTx, CallbackAttempt, CallbackOutbox, ClaimedDelivery, KeyUse};
use transgate_core::ids::{AccountId, JobId, Micros};
use transgate_core::jobs::app::StartAnswer;
use transgate_core::jobs::ports::{BoxFuture, StoreResult};

use crate::db;
use crate::rows::{from_text, get, to_text};
use crate::uow::PgJobUow;

macro_rules! key_cols {
    () => {
        "id, account_id, name, transgate_jobs.ts_to_us(created_at) AS created_us,
         transgate_jobs.ts_to_us(expires_at) AS expires_us, transgate_jobs.ts_to_us(last_used_at) AS last_used_us,
         active, scopes, expiration_policy, metadata, digest, legacy_bcrypt, hint"
    };
}

macro_rules! delivery_cols {
    () => {
        "id, account_id, job_id, event_id, url, state, attempts, transgate_jobs.ts_to_us(next_attempt_at) AS next_us,
         transgate_jobs.ts_to_us(completed_at) AS completed_us, transgate_jobs.ts_to_us(created_at) AS created_us,
         transgate_jobs.ts_to_us(delivered_at) AS delivered_us, payload"
    };
}

fn key_from(row: &PgRow) -> Result<ApiKey, StoreError> {
    let policy: String = get(row, "expiration_policy")?;
    Ok(ApiKey {
        id: KeyId(get(row, "id")?),
        account: AccountId(get(row, "account_id")?),
        name: get(row, "name")?,
        created_at: Micros(get(row, "created_us")?),
        expires_at: get::<Option<i64>>(row, "expires_us")?.map(Micros),
        last_used_at: get::<Option<i64>>(row, "last_used_us")?.map(Micros),
        active: get(row, "active")?,
        scopes: get(row, "scopes")?,
        expiration_policy: from_text(&policy, "expiration policy")?,
        metadata: get::<Json<Value>>(row, "metadata")?.0,
        digest: get(row, "digest")?,
        legacy_bcrypt: get(row, "legacy_bcrypt")?,
        hint: get(row, "hint")?,
    })
}

fn delivery_from(row: &PgRow) -> Result<CallbackDelivery, StoreError> {
    let state: String = get(row, "state")?;
    Ok(CallbackDelivery {
        id: DeliveryId(get(row, "id")?),
        account: AccountId(get(row, "account_id")?),
        job: JobId(get(row, "job_id")?),
        event_id: get(row, "event_id")?,
        url: get(row, "url")?,
        state: from_text(&state, "delivery state")?,
        attempts: u32::try_from(get::<i32>(row, "attempts")?).unwrap_or(0),
        next_attempt_at: Micros(get(row, "next_us")?),
        completed_at: Micros(get(row, "completed_us")?),
        created_at: Micros(get(row, "created_us")?),
        delivered_at: get::<Option<i64>>(row, "delivered_us")?.map(Micros),
        payload: get::<Option<Json<Value>>>(row, "payload")?.map(|j| j.0),
    })
}

fn record_from(row: &PgRow) -> Result<IdempotencyRecord, StoreError> {
    let jobs: Vec<i64> = get(row, "jobs")?;
    Ok(IdempotencyRecord {
        account: AccountId(get(row, "account_id")?),
        key: get(row, "key")?,
        request_digest: get(row, "request_digest")?,
        request_type: get(row, "request_type")?,
        jobs: jobs.into_iter().map(JobId).collect(),
        answer: get::<Option<Json<StartAnswer>>>(row, "answer")?.map(|j| j.0),
        created_at: Micros(get(row, "created_us")?),
    })
}

impl ApiTx for PgJobUow {
    fn insert_key<'a>(&'a mut self, key: &'a ApiKey) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // A duplicate id or digest is a unique violation, reported as `Conflict`.
            sqlx::query(
                "INSERT INTO transgate_api.api_keys
                   (id, account_id, name, created_at, expires_at, last_used_at, active, scopes, expiration_policy,
                    metadata, digest, legacy_bcrypt, hint)
                 VALUES ($1, $2, $3, transgate_jobs.us_to_ts($4), transgate_jobs.us_to_ts($5), transgate_jobs.us_to_ts($6),
                         $7, $8, $9, $10, $11, $12, $13)",
            )
            .bind(key.id.0)
            .bind(key.account.0)
            .bind(&key.name)
            .bind(key.created_at.0)
            .bind(key.expires_at.map(|t| t.0))
            .bind(key.last_used_at.map(|t| t.0))
            .bind(key.active)
            .bind(&key.scopes)
            .bind(to_text(&key.expiration_policy)?)
            .bind(Json(&key.metadata))
            .bind(&key.digest)
            .bind(&key.legacy_bcrypt)
            .bind(&key.hint)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn lock_key(&mut self, id: KeyId) -> StoreResult<'_, Option<ApiKey>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", key_cols!(), " FROM transgate_api.api_keys WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(key_from).transpose()
        })
    }

    fn save_key<'a>(&'a mut self, key: &'a ApiKey) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_api.api_keys
                 SET name = $2, active = $3, last_used_at = transgate_jobs.us_to_ts($4), digest = $5, legacy_bcrypt = $6, hint = $7
                 WHERE id = $1",
            )
            .bind(key.id.0)
            .bind(&key.name)
            .bind(key.active)
            .bind(key.last_used_at.map(|t| t.0))
            .bind(&key.digest)
            .bind(&key.legacy_bcrypt)
            .bind(&key.hint)
            .execute(self.conn())
            .await
            .map_err(db)?;
            if done.rows_affected() == 1 { Ok(()) } else { Err(StoreError::Conflict(format!("api key {} does not exist", key.id))) }
        })
    }

    fn key_by_digest<'a>(&'a mut self, digest: &'a str) -> StoreResult<'a, Option<ApiKey>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", key_cols!(), " FROM transgate_api.api_keys WHERE digest = $1"))
                .bind(digest)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(key_from).transpose()
        })
    }

    fn legacy_candidates(&mut self, now: Micros) -> StoreResult<'_, Vec<ApiKey>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                key_cols!(),
                " FROM transgate_api.api_keys
                  WHERE active AND digest IS NULL AND legacy_bcrypt IS NOT NULL
                    AND (expires_at IS NULL OR expires_at >= transgate_jobs.us_to_ts($1))
                  ORDER BY id"
            ))
            .bind(now.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            rows.iter().map(key_from).collect()
        })
    }

    fn list_keys(&mut self, account: AccountId) -> StoreResult<'_, Vec<ApiKey>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                key_cols!(),
                " FROM transgate_api.api_keys WHERE account_id = $1 ORDER BY created_at DESC, id DESC"
            ))
            .bind(account.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            rows.iter().map(key_from).collect()
        })
    }

    fn record_use<'a>(&'a mut self, usage: &'a KeyUse) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_api.key_uses (key_id, account_id, at, endpoint, method, status, ip, user_agent, duration_ms)
                 VALUES ($1, $2, transgate_jobs.us_to_ts($3), $4, $5, $6, $7, $8, $9)",
            )
            .bind(usage.key.0)
            .bind(usage.account.0)
            .bind(usage.at.0)
            .bind(&usage.endpoint)
            .bind(&usage.method)
            .bind(i32::from(usage.status))
            .bind(&usage.ip)
            .bind(&usage.user_agent)
            .bind(i32::try_from(usage.duration_ms).unwrap_or(i32::MAX))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn idempotency<'a>(&'a mut self, account: AccountId, key: &'a str, expired_before: Micros) -> StoreResult<'a, Option<IdempotencyRecord>> {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT account_id, key, request_digest, request_type, jobs, answer,
                        transgate_jobs.ts_to_us(created_at) AS created_us
                 FROM transgate_api.idempotency
                 WHERE account_id = $1 AND key = $2 AND created_at > transgate_jobs.us_to_ts($3)",
            )
            .bind(account.0)
            .bind(key)
            .bind(expired_before.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            row.as_ref().map(record_from).transpose()
        })
    }

    fn insert_idempotency<'a>(&'a mut self, record: &'a IdempotencyRecord, expired_before: Micros) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // A live record of the same key wins: the conflict clause only replaces an expired one.
            let jobs: Vec<i64> = record.jobs.iter().map(|j| j.0).collect();
            let stored: Option<i32> = sqlx::query_scalar(
                "INSERT INTO transgate_api.idempotency (account_id, key, request_digest, request_type, jobs, answer, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, transgate_jobs.us_to_ts($7))
                 ON CONFLICT (account_id, key) DO UPDATE SET
                   request_digest = EXCLUDED.request_digest, request_type = EXCLUDED.request_type,
                   jobs = EXCLUDED.jobs, answer = EXCLUDED.answer, created_at = EXCLUDED.created_at
                 WHERE idempotency.created_at <= transgate_jobs.us_to_ts($8)
                 RETURNING 1",
            )
            .bind(record.account.0)
            .bind(&record.key)
            .bind(&record.request_digest)
            .bind(&record.request_type)
            .bind(&jobs)
            .bind(record.answer.as_ref().map(Json))
            .bind(record.created_at.0)
            .bind(expired_before.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            if stored.is_some() { Ok(()) } else { Err(StoreError::Conflict("the idempotency key is in use".into())) }
        })
    }

    fn save_idempotency_answer<'a>(&'a mut self, account: AccountId, key: &'a str, answer: &'a StartAnswer) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let done = sqlx::query("UPDATE transgate_api.idempotency SET answer = $3 WHERE account_id = $1 AND key = $2")
                .bind(account.0)
                .bind(key)
                .bind(Json(answer))
                .execute(self.conn())
                .await
                .map_err(db)?;
            if done.rows_affected() == 1 { Ok(()) } else { Err(StoreError::Conflict("the idempotency record is gone".into())) }
        })
    }

    fn insert_delivery<'a>(&'a mut self, delivery: &'a CallbackDelivery) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query(
                "INSERT INTO transgate_api.callback_deliveries
                   (id, account_id, job_id, event_id, url, state, attempts, next_attempt_at, completed_at, created_at,
                    delivered_at, payload)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, transgate_jobs.us_to_ts($8), transgate_jobs.us_to_ts($9),
                         transgate_jobs.us_to_ts($10), transgate_jobs.us_to_ts($11), $12)
                 ON CONFLICT (event_id) DO NOTHING",
            )
            .bind(delivery.id.0)
            .bind(delivery.account.0)
            .bind(delivery.job.0)
            .bind(delivery.event_id)
            .bind(&delivery.url)
            .bind(to_text(&delivery.state)?)
            .bind(i32::try_from(delivery.attempts).unwrap_or(i32::MAX))
            .bind(delivery.next_attempt_at.0)
            .bind(delivery.completed_at.0)
            .bind(delivery.created_at.0)
            .bind(delivery.delivered_at.map(|t| t.0))
            .bind(delivery.payload.as_ref().map(Json))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }
}

/// D3: the per-key limiter shared by every replica.
#[derive(Clone)]
pub struct PgApiRateLimiter {
    pool: PgPool,
}

impl PgApiRateLimiter {
    pub fn new(pool: PgPool) -> PgApiRateLimiter {
        PgApiRateLimiter { pool }
    }
}

impl ApiRateLimiter for PgApiRateLimiter {
    fn consume(&self, key: KeyId, windows: RateWindows, limits: RateLimits) -> BoxFuture<'_, Result<RateCounts, StoreError>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(db)?;
            // One writer per key at a time. READ COMMITTED gives the next statement a fresh
            // snapshot after the lock is held, so no two requests both see room for one unit.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(format!("transgate-api-rate:{key}"))
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            let (allowed, minute, hour): (bool, i32, i32) = sqlx::query_as(
                "WITH cur AS (
                   SELECT COALESCE(max(count) FILTER (WHERE kind = 'minute' AND window_start = transgate_jobs.us_to_ts($2)), 0) AS minute,
                          COALESCE(max(count) FILTER (WHERE kind = 'hour' AND window_start = transgate_jobs.us_to_ts($3)), 0) AS hour
                   FROM transgate_api.rate_windows WHERE key_id = $1
                 ), room AS (
                   SELECT minute, hour FROM cur WHERE minute < $4 AND hour < $5
                 ), bump AS (
                   INSERT INTO transgate_api.rate_windows (key_id, kind, window_start, count)
                   SELECT $1, 'minute', transgate_jobs.us_to_ts($2), 1 FROM room
                   UNION ALL
                   SELECT $1, 'hour', transgate_jobs.us_to_ts($3), 1 FROM room
                   ON CONFLICT (key_id, kind, window_start) DO UPDATE SET count = rate_windows.count + 1
                   RETURNING 1
                 )
                 SELECT (SELECT count(*) FROM room) = 1, cur.minute, cur.hour FROM cur",
            )
            .bind(key.0)
            .bind(windows.minute_start.0)
            .bind(windows.hour_start.0)
            .bind(i64::from(limits.per_minute))
            .bind(i64::from(limits.per_hour))
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            if allowed && minute == 0 {
                // First request of this key's minute: windows older than the previous hour are dead.
                sqlx::query("DELETE FROM transgate_api.rate_windows WHERE key_id = $1 AND window_start < transgate_jobs.us_to_ts($2)")
                    .bind(key.0)
                    .bind(windows.hour_start.0 - 3_600_000_000)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
            }
            tx.commit().await.map_err(db)?;
            let (minute, hour) = (u32::try_from(minute).unwrap_or(0), u32::try_from(hour).unwrap_or(0));
            // A denied call reports the counts it found, unchanged; an allowed one the counts after it.
            Ok(if allowed { RateCounts { allowed, minute: minute + 1, hour: hour + 1 } } else { RateCounts { allowed, minute, hour } })
        })
    }
}

/// D5: the callback outbox. Every write is fenced by the claim's token.
#[derive(Clone)]
pub struct PgCallbackOutbox {
    pool: PgPool,
}

impl PgCallbackOutbox {
    pub fn new(pool: PgPool) -> PgCallbackOutbox {
        PgCallbackOutbox { pool }
    }
}

impl CallbackOutbox for PgCallbackOutbox {
    fn claim_deliveries(&self, limit: u32, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<ClaimedDelivery>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "UPDATE transgate_api.callback_deliveries
                 SET claim_token = gen_random_uuid(),
                     claimed_until = transgate_jobs.us_to_ts($1 + $2 * 1000000)
                 WHERE id IN (
                   SELECT id FROM transgate_api.callback_deliveries
                   WHERE state = 'pending' AND next_attempt_at <= transgate_jobs.us_to_ts($1)
                     AND (claimed_until IS NULL OR claimed_until <= transgate_jobs.us_to_ts($1))
                   ORDER BY next_attempt_at, id LIMIT $3 FOR UPDATE SKIP LOCKED)
                 RETURNING ",
                delivery_cols!(),
                ", claim_token"
            ))
            .bind(now.0)
            .bind(lease_seconds)
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let mut claims = rows
                .iter()
                .map(|row| Ok(ClaimedDelivery { delivery: delivery_from(row)?, token: get(row, "claim_token")? }))
                .collect::<Result<Vec<_>, StoreError>>()?;
            claims.sort_by_key(|c| (c.delivery.next_attempt_at, c.delivery.id));
            Ok(claims)
        })
    }

    fn postpone<'a>(&'a self, claim: &'a ClaimedDelivery, until: Micros) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_api.callback_deliveries
                 SET next_attempt_at = transgate_jobs.us_to_ts($3), claim_token = NULL, claimed_until = NULL
                 WHERE id = $1 AND claim_token = $2 AND state = 'pending'",
            )
            .bind(claim.delivery.id.0)
            .bind(claim.token)
            .bind(until.0)
            .execute(&self.pool)
            .await
            .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }

    fn begin_attempt<'a>(&'a self, claim: &'a ClaimedDelivery, attempt: &'a CallbackAttempt, payload: &'a Value) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(db)?;
            let fenced = sqlx::query(
                "UPDATE transgate_api.callback_deliveries SET attempts = $3, payload = $4
                 WHERE id = $1 AND claim_token = $2 AND state = 'pending'",
            )
            .bind(claim.delivery.id.0)
            .bind(claim.token)
            .bind(i32::try_from(attempt.n).unwrap_or(i32::MAX))
            .bind(Json(payload))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            if fenced.rows_affected() != 1 {
                tx.rollback().await.map_err(db)?;
                return Ok(false);
            }
            sqlx::query("INSERT INTO transgate_api.callback_attempts (delivery_id, n, started_at) VALUES ($1, $2, transgate_jobs.us_to_ts($3))")
                .bind(attempt.delivery.0)
                .bind(i32::try_from(attempt.n).unwrap_or(i32::MAX))
                .bind(attempt.started_at.0)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            tx.commit().await.map_err(db)?;
            Ok(true)
        })
    }

    fn finish_attempt<'a>(&'a self, claim: &'a ClaimedDelivery, attempt: &'a CallbackAttempt, settlement: &'a Settlement) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(db)?;
            // The delivery row is fenced first: a superseded claim must record nothing.
            let fenced = sqlx::query(
                "UPDATE transgate_api.callback_deliveries
                 SET state = $3,
                     next_attempt_at = COALESCE(transgate_jobs.us_to_ts($4), next_attempt_at),
                     delivered_at = transgate_jobs.us_to_ts($5),
                     claim_token = NULL, claimed_until = NULL
                 WHERE id = $1 AND claim_token = $2 AND state = 'pending'",
            )
            .bind(claim.delivery.id.0)
            .bind(claim.token)
            .bind(to_text(&settlement.state)?)
            .bind(settlement.next_attempt_at.map(|t| t.0))
            .bind(settlement.delivered_at.map(|t| t.0))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            if fenced.rows_affected() != 1 {
                tx.rollback().await.map_err(db)?;
                return Ok(false);
            }
            sqlx::query(
                "UPDATE transgate_api.callback_attempts
                 SET outcome = $3, finished_at = transgate_jobs.us_to_ts($4)
                 WHERE delivery_id = $1 AND n = $2",
            )
            .bind(attempt.delivery.0)
            .bind(i32::try_from(attempt.n).unwrap_or(i32::MAX))
            .bind(attempt.outcome.as_ref().map(Json))
            .bind(attempt.finished_at.map(|t| t.0))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            tx.commit().await.map_err(db)?;
            Ok(true)
        })
    }

    fn abandon<'a>(&'a self, claim: &'a ClaimedDelivery, reason: &'a str, _now: Micros) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_api.callback_deliveries
                 SET state = $3, note = $4, claim_token = NULL, claimed_until = NULL
                 WHERE id = $1 AND claim_token = $2 AND state = 'pending'",
            )
            .bind(claim.delivery.id.0)
            .bind(claim.token)
            .bind(to_text(&DeliveryState::Abandoned)?)
            .bind(reason)
            .execute(&self.pool)
            .await
            .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }

    fn signing_secret<'a>(&'a self, account: AccountId, candidate: &'a str) -> StoreResult<'a, String> {
        Box::pin(async move {
            sqlx::query("INSERT INTO transgate_api.signing_secrets (account_id, secret) VALUES ($1, $2) ON CONFLICT (account_id) DO NOTHING")
                .bind(account.0)
                .bind(candidate)
                .execute(&self.pool)
                .await
                .map_err(db)?;
            sqlx::query_scalar("SELECT secret FROM transgate_api.signing_secrets WHERE account_id = $1")
                .bind(account.0)
                .fetch_one(&self.pool)
                .await
                .map_err(db)
        })
    }
}
