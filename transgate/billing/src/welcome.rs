//! The welcome mail queue (EML-002). The database queues a row when a customer record is created;
//! the host's worker claims a few under a lease, sends each with a stable key and marks it sent.
//! The sent mark is durable: a row marked sent is never claimed again, a claim whose mail failed
//! (or whose process died) becomes claimable again when its lease ends, and the stable key keeps a
//! provider from sending an unknown-outcome retry twice.

use billing_core::types::{AccountId, Micros};
use sqlx::PgPool;
use uuid::Uuid;

/// Who to greet, read from the customer record at delivery time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    /// How many times this welcome has been claimed, this claim included.
    pub attempts: i32,
}

/// The stable key of one account's welcome: every retry carries it.
pub fn idempotency_key(account: AccountId) -> String {
    format!("welcome-{}", account.0)
}

/// Claims up to `limit` unsent welcomes whose lease is free, oldest first, for `lease_seconds`.
pub async fn claim(pool: &PgPool, limit: i64, lease_seconds: i64, now: Micros) -> Result<Vec<Welcome>, String> {
    let rows: Vec<(Uuid, String, Option<String>, i32)> = sqlx::query_as(
        "WITH claimed AS (
           UPDATE transgate_billing.welcome_mails w
              SET attempts = w.attempts + 1, claimed_until = billing.us_to_ts($3) + make_interval(secs => $2)
            WHERE w.account_id IN (
              SELECT account_id FROM transgate_billing.welcome_mails
               WHERE sent_at IS NULL AND (claimed_until IS NULL OR claimed_until <= billing.us_to_ts($3))
               ORDER BY created_at, account_id LIMIT $1 FOR UPDATE SKIP LOCKED)
            RETURNING w.account_id, w.attempts)
         SELECT c.account_id, c.email, c.full_name, claimed.attempts
           FROM claimed JOIN transgate_billing.customers c ON c.account_id = claimed.account_id
          ORDER BY c.created_at, c.account_id",
    )
    .bind(limit)
    .bind(lease_seconds as f64)
    .bind(now.0)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(account, email, full_name, attempts)| Welcome { account: AccountId(account), email, full_name, attempts }).collect())
}

/// Marks the welcome sent. `false` when it already was.
pub async fn mark_sent(pool: &PgPool, account: AccountId, now: Micros) -> Result<bool, String> {
    let done = sqlx::query("UPDATE transgate_billing.welcome_mails SET sent_at = billing.us_to_ts($2) WHERE account_id = $1 AND sent_at IS NULL")
        .bind(account.0)
        .bind(now.0)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(done.rows_affected() == 1)
}
