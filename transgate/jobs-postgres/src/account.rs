//! Job feedback and the support request outbox (P5 D7): `AccountTx` on the same
//! connection and transaction as the `PgJobUow` that lends it, and the delivery
//! side on the store. The caller holds the billing account lock for writes;
//! nothing here opens network I/O.

use transgate_core::account::ports::{AccountTx, ClaimedSupport, SupportOutbox};
use transgate_core::account::{JobFeedback, SupportId, SupportRequest};
use transgate_core::ids::{AccountId, JobId, Micros};
use transgate_core::jobs::ports::StoreResult;
use uuid::Uuid;

use crate::db;
use crate::store::PgJobStore;
use crate::uow::PgJobUow;

impl AccountTx for PgJobUow {
    fn insert_feedback<'a>(&'a mut self, feedback: &'a JobFeedback) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // A missing job fails the foreign key (Conflict): feedback never outlives its job.
            sqlx::query(
                "INSERT INTO transgate_jobs.job_feedback (id, job_id, account_id, rating, comment, created_at)
                 VALUES ($1, $2, $3, $4, $5, transgate_jobs.us_to_ts($6))",
            )
            .bind(feedback.id)
            .bind(feedback.job.0)
            .bind(feedback.account.0)
            .bind(feedback.rating.value())
            .bind(&feedback.comment)
            .bind(feedback.created_at.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn insert_support<'a>(&'a mut self, request: &'a SupportRequest) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.support_requests (id, account_id, job_id, message, created_at)
                 VALUES ($1, $2, $3, $4, transgate_jobs.us_to_ts($5))",
            )
            .bind(request.id.0)
            .bind(request.account.0)
            .bind(request.job.map(|j| j.0))
            .bind(&request.message)
            .bind(request.created_at.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }
}

impl SupportOutbox for PgJobStore {
    fn claim_support(&self, limit: u32, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<ClaimedSupport>> {
        Box::pin(async move {
            // One statement: due rows are leased and returned; a concurrent claimer skips them.
            let rows: Vec<(Uuid, Uuid, Option<i64>, String, i64, i32)> = sqlx::query_as(
                "UPDATE transgate_jobs.support_requests
                 SET attempts = attempts + 1, claimed_until = transgate_jobs.us_to_ts($2 + $3 * 1000000)
                 WHERE id IN (SELECT id FROM transgate_jobs.support_requests
                              WHERE claimed_until IS NULL OR claimed_until <= transgate_jobs.us_to_ts($2)
                              ORDER BY created_at, id LIMIT $1 FOR UPDATE SKIP LOCKED)
                 RETURNING id, account_id, job_id, message, transgate_jobs.ts_to_us(created_at), attempts",
            )
            .bind(i64::from(limit))
            .bind(now.0)
            .bind(lease_seconds)
            .fetch_all(self.pool())
            .await
            .map_err(db)?;
            let mut claimed: Vec<ClaimedSupport> = rows
                .into_iter()
                .map(|(id, account, job, message, created, attempts)| ClaimedSupport {
                    request: SupportRequest {
                        id: SupportId(id),
                        account: AccountId(account),
                        job: job.map(JobId),
                        message,
                        created_at: Micros(created),
                    },
                    attempts,
                })
                .collect();
            claimed.sort_by_key(|c| (c.request.created_at, c.request.id.0));
            Ok(claimed)
        })
    }

    // Every claim increments `attempts`, so the claim's attempts is its lease token: a
    // worker whose lease expired and was reclaimed changes nothing (false).
    fn ack_support<'a>(&'a self, claim: &'a ClaimedSupport) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query("DELETE FROM transgate_jobs.support_requests WHERE id = $1 AND attempts = $2")
                .bind(claim.request.id.0)
                .bind(claim.attempts)
                .execute(self.pool())
                .await
                .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }

    fn release_support<'a>(&'a self, claim: &'a ClaimedSupport, error: &'a str, retry_at: Micros) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_jobs.support_requests
                 SET claimed_until = transgate_jobs.us_to_ts($3), last_error = $4
                 WHERE id = $1 AND attempts = $2",
            )
            .bind(claim.request.id.0)
            .bind(claim.attempts)
            .bind(retry_at.0)
            .bind(error)
            .execute(self.pool())
            .await
            .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }
}
