use billing_core::ports::StoreError;
use sqlx::PgPool;
use transgate_core::ids::Micros;
use transgate_core::jobs::ports::{BoxFuture, RateDecision, RateLimiter};
use uuid::Uuid;

use crate::db;

/// Namespace of the stored key hashes; the caller's key itself is never stored.
const KEY_NAMESPACE: Uuid = Uuid::from_u128(0x7472_616e_7367_6174_652d_7261_7465_6b31);

/// `RateLimiter` shared by every replica: a fixed window that starts at the
/// first request of a key; a denied request does not count (RTL-005).
#[derive(Clone)]
pub struct PgRateLimiter {
    pool: PgPool,
}

impl PgRateLimiter {
    pub fn new(pool: PgPool) -> PgRateLimiter {
        PgRateLimiter { pool }
    }
}

impl RateLimiter for PgRateLimiter {
    fn hit<'a>(&'a self, key: &'a str, limit: u32, window_ms: i64, now: Micros) -> BoxFuture<'a, Result<RateDecision, StoreError>> {
        Box::pin(async move {
            let window_us = window_ms.saturating_mul(1000);
            if limit == 0 {
                return Ok(RateDecision { allowed: false, limit, remaining: 0, reset_at: Micros(now.0 + window_us) });
            }
            let hash = Uuid::new_v5(&KEY_NAMESPACE, key.as_bytes());
            // One statement under the row lock: an expired window restarts at this
            // request, a full one is left untouched and returns nothing.
            let counted: Option<(i32, i64)> = sqlx::query_as(
                "INSERT INTO transgate_jobs.rate_windows (key_hash, window_start_us, reset_at_us, hits)
                 VALUES ($1, $2, $2 + $3, 1)
                 ON CONFLICT (key_hash) DO UPDATE SET
                   window_start_us = CASE WHEN rate_windows.reset_at_us <= $2 THEN $2 ELSE rate_windows.window_start_us END,
                   reset_at_us = CASE WHEN rate_windows.reset_at_us <= $2 THEN $2 + $3 ELSE rate_windows.reset_at_us END,
                   hits = CASE WHEN rate_windows.reset_at_us <= $2 THEN 1 ELSE rate_windows.hits + 1 END
                 WHERE rate_windows.reset_at_us <= $2 OR rate_windows.hits < $4
                 RETURNING hits, reset_at_us",
            )
            .bind(hash)
            .bind(now.0)
            .bind(window_us)
            .bind(i64::from(limit))
            .fetch_optional(&self.pool)
            .await
            .map_err(db)?;
            if let Some((hits, reset_at)) = counted {
                let remaining = limit.saturating_sub(hits.max(0) as u32);
                return Ok(RateDecision { allowed: true, limit, remaining, reset_at: Micros(reset_at) });
            }
            let reset_at: i64 = sqlx::query_scalar("SELECT reset_at_us FROM transgate_jobs.rate_windows WHERE key_hash = $1")
                .bind(hash)
                .fetch_one(&self.pool)
                .await
                .map_err(db)?;
            Ok(RateDecision { allowed: false, limit, remaining: 0, reset_at: Micros(reset_at) })
        })
    }
}
