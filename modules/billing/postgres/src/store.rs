//! `BillingStore`: transactions billing opens itself, and the outbox.

use billing_core::model::{BillingEvent, OutboxEvent};
use billing_core::ports::{BillingStore, StoreError, StoreResult, UnitOfWork};
use billing_core::types::Micros;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Json;
use uuid::Uuid;

use crate::tx::{PgBillingTx, db};

#[derive(Clone)]
pub struct PgBillingStore {
    pool: PgPool,
}

impl PgBillingStore {
    pub fn new(pool: PgPool) -> PgBillingStore {
        PgBillingStore { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl BillingStore for PgBillingStore {
    fn begin(&self) -> StoreResult<'_, Box<dyn UnitOfWork>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(db)?;
            // Explicit, not assumed: the account lock relies on a fresh
            // snapshot per statement.
            sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            Ok(Box::new(PgBillingTx(tx)) as Box<dyn UnitOfWork>)
        })
    }

    fn ping(&self) -> StoreResult<'_, ()> {
        Box::pin(async move {
            sqlx::query("SELECT 1").execute(&self.pool).await.map_err(db)?;
            Ok(())
        })
    }

    fn claim_outbox(&self, limit: i64, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<OutboxEvent>> {
        Box::pin(async move {
            if limit <= 0 || lease_seconds <= 0 {
                return Err(StoreError::Conflict("an outbox claim needs a positive limit and a positive lease".into()));
            }
            // Short transaction: claim, commit, and only then talk to anyone.
            // SKIP LOCKED plus the lease keeps two consumers off one event.
            let rows: Vec<(i64, Uuid, Uuid, i64, i32, Json<Value>)> = sqlx::query_as(
                "WITH picked AS (
                   SELECT id FROM billing.outbox
                   WHERE published_at IS NULL
                     AND (claimed_until IS NULL OR claimed_until < billing.us_to_ts($3))
                     AND (retry_after IS NULL OR retry_after <= billing.us_to_ts($3))
                   ORDER BY id
                   LIMIT $1
                   FOR UPDATE SKIP LOCKED)
                 UPDATE billing.outbox o
                 SET claim_token = gen_random_uuid(),
                     claimed_until = billing.us_to_ts($3) + ($2::bigint::float8 * interval '1 second')
                 FROM picked WHERE o.id = picked.id
                 RETURNING o.id, o.event_id, o.claim_token, billing.ts_to_us(o.occurred_at), o.attempts, o.payload",
            )
            .bind(limit)
            .bind(lease_seconds)
            .bind(now.0)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let mut events = rows
                .into_iter()
                .map(|(id, event_id, claim_token, occurred, attempts, payload)| {
                    let event: BillingEvent = serde_json::from_value(payload.0)
                        .map_err(|e| StoreError::Unavailable(format!("outbox event {id} is unreadable: {e}")))?;
                    Ok(OutboxEvent { id, event_id, claim_token, occurred_at: Micros(occurred), attempts, event })
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            events.sort_by_key(|e| e.id);
            Ok(events)
        })
    }

    fn delivered_targets(&self, event_id: Uuid) -> StoreResult<'_, Vec<String>> {
        Box::pin(async move {
            sqlx::query_scalar("SELECT target FROM billing.outbox_deliveries WHERE event_id = $1 ORDER BY target")
                .bind(event_id)
                .fetch_all(&self.pool)
                .await
                .map_err(db)
        })
    }

    fn record_delivery<'a>(&'a self, event_id: Uuid, target: &'a str, at: Micros) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO billing.outbox_deliveries (event_id, target, delivered_at)
                 VALUES ($1, $2, billing.us_to_ts($3)) ON CONFLICT DO NOTHING",
            )
            .bind(event_id)
            .bind(target)
            .bind(at.0)
            .execute(&self.pool)
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn ack_outbox<'a>(&'a self, event: &'a OutboxEvent, at: Micros) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let result = sqlx::query(
                "UPDATE billing.outbox SET published_at = billing.us_to_ts($3), claimed_until = NULL
                 WHERE id = $1 AND claim_token = $2 AND published_at IS NULL",
            )
            .bind(event.id)
            .bind(event.claim_token)
            .bind(at.0)
            .execute(&self.pool)
            .await
            .map_err(db)?;
            Ok(result.rows_affected() == 1)
        })
    }

    fn release_outbox<'a>(&'a self, event: &'a OutboxEvent, error: &'a str, retry_at: Micros) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE billing.outbox
                 SET claimed_until = NULL, claim_token = NULL, attempts = attempts + 1,
                     last_error = $3, retry_after = billing.us_to_ts($4)
                 WHERE id = $1 AND claim_token = $2",
            )
            .bind(event.id)
            .bind(event.claim_token)
            .bind(error)
            .bind(retry_at.0)
            .execute(&self.pool)
            .await
            .map_err(db)?;
            Ok(())
        })
    }
}
