use billing_core::ports::StoreError;
use transgate_core::ai::GenerationId;
use sqlx::PgPool;
use sqlx::types::Json;
use transgate_core::ids::{AccountId, AttemptId, ErasureId, JobId, Micros};
use transgate_core::jobs::domain::{Attempt, Job};
use transgate_core::jobs::events::JobEvent;
use transgate_core::jobs::ports::{InboxEntry, JobListQuery, JobQueries, JobStore, JobUow, LanguageUsage, StoreResult};
use transgate_core::languages::Language;
use transgate_core::privacy::erasure::{self, ErasureState};
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::Line;
use uuid::Uuid;

use crate::rows::{self, attempt_cols, erasure_cols, inbox_cols, job_cols, line_cols};
use crate::uow::PgJobUow;
use crate::db;

/// The job store over the verified billing runtime pool.
#[derive(Clone)]
pub struct PgJobStore {
    pool: PgPool,
}

/// A job event claimed for delivery. Every change to it is fenced by `token`,
/// so a delivery whose lease expired and was reclaimed cannot acknowledge it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaimedEvent {
    pub id: i64,
    pub token: Uuid,
    pub attempts: i32,
    pub event: JobEvent,
}

impl PgJobStore {
    /// `pool` must be the runtime pool from `billing_postgres::connect_runtime`,
    /// checked with [`crate::verify_runtime`].
    pub fn new(pool: PgPool) -> PgJobStore {
        PgJobStore { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Claims up to `limit` undelivered job events until `now + lease_seconds`,
    /// oldest first, in one statement; no transaction stays open during delivery.
    pub async fn claim_events(&self, limit: i64, lease_seconds: i64, now: Micros) -> Result<Vec<ClaimedEvent>, StoreError> {
        let token = Uuid::new_v4();
        let rows: Vec<(i64, i32, Json<JobEvent>)> = sqlx::query_as(
            "UPDATE transgate_jobs.events
             SET claim_token = $2, attempts = attempts + 1,
                 claimed_until = transgate_jobs.us_to_ts($3 + $4 * 1000000)
             WHERE id IN (SELECT id FROM transgate_jobs.events
                          WHERE published_at IS NULL
                            AND (claimed_until IS NULL OR claimed_until <= transgate_jobs.us_to_ts($3))
                          ORDER BY id LIMIT $1 FOR UPDATE SKIP LOCKED)
             RETURNING id, attempts, event",
        )
        .bind(limit)
        .bind(token)
        .bind(now.0)
        .bind(lease_seconds)
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        let mut claimed: Vec<ClaimedEvent> =
            rows.into_iter().map(|(id, attempts, event)| ClaimedEvent { id, token, attempts, event: event.0 }).collect();
        claimed.sort_by_key(|e| e.id);
        Ok(claimed)
    }

    /// Marks a claimed event delivered; false when the claim was lost.
    pub async fn ack_event(&self, event: &ClaimedEvent, now: Micros) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "UPDATE transgate_jobs.events SET published_at = transgate_jobs.us_to_ts($3), claim_token = NULL
             WHERE id = $1 AND claim_token = $2 AND published_at IS NULL",
        )
        .bind(event.id)
        .bind(event.token)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        Ok(done.rows_affected() == 1)
    }

    /// Gives a claimed event back after a failed delivery, not claimable before `retry_at`.
    pub async fn release_event(&self, event: &ClaimedEvent, error: &str, retry_at: Micros) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "UPDATE transgate_jobs.events
             SET claim_token = NULL, claimed_until = transgate_jobs.us_to_ts($3), last_error = $4
             WHERE id = $1 AND claim_token = $2 AND published_at IS NULL",
        )
        .bind(event.id)
        .bind(event.token)
        .bind(retry_at.0)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        Ok(done.rows_affected() == 1)
    }

    async fn jobs_where(&self, sql: &'static str, now: i64, limit: u32) -> Result<Vec<Job>, StoreError> {
        let rows = sqlx::query(sql).bind(now).bind(i64::from(limit)).fetch_all(&self.pool).await.map_err(db)?;
        rows.iter().map(rows::job_from).collect()
    }
}

impl JobQueries for PgJobStore {
    fn list_jobs<'a>(&'a self, query: &'a JobListQuery) -> StoreResult<'a, Vec<Job>> {
        Box::pin(async move {
            let statuses: Vec<&str> = query.statuses.iter().map(|s| s.as_str()).collect();
            let rows = sqlx::query(concat!(
                "SELECT ",
                job_cols!(),
                " FROM transgate_jobs.jobs
                 WHERE account_id = $1 AND status = ANY($2)
                   AND ($3::bigint IS NULL OR created_at >= transgate_jobs.us_to_ts($3))
                   AND ($4::bigint IS NULL OR id < $4)
                 ORDER BY id DESC LIMIT $5"
            ))
            .bind(query.account.0)
            .bind(&statuses)
            .bind(query.since.map(|s| s.0))
            .bind(query.before.map(|b| b.0))
            .bind(i64::from(query.limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            rows.iter().map(rows::job_from).collect()
        })
    }

    fn owned_job(&self, account: AccountId, id: JobId) -> StoreResult<'_, Option<Job>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", job_cols!(), " FROM transgate_jobs.jobs WHERE id = $1 AND account_id = $2"))
                .bind(id.0)
                .bind(account.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(db)?;
            row.as_ref().map(rows::job_from).transpose()
        })
    }

    fn lines(&self, job: JobId) -> StoreResult<'_, Vec<Line>> {
        Box::pin(async move {
            // Byte order of the stored text, the order EDT-011 sorts by.
            let rows = sqlx::query(concat!(
                "SELECT ",
                line_cols!(),
                " FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY start_time COLLATE \"C\", id"
            ))
            .bind(job.0)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            rows.iter().map(rows::line_from).collect()
        })
    }

    fn language_usage(&self, account: AccountId) -> StoreResult<'_, LanguageUsage> {
        Box::pin(async move {
            let languages: Vec<(String, i64)> = sqlx::query_as(
                "SELECT language, count(*) FROM transgate_jobs.jobs
                 WHERE account_id = $1 AND language IS NOT NULL
                 GROUP BY language ORDER BY count(*) DESC, language",
            )
            .bind(account.0)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let targets: Vec<(String, i64)> = sqlx::query_as(
                "SELECT t.target, count(*) FROM transgate_jobs.jobs j, unnest(j.translate_to) AS t(target)
                 WHERE j.account_id = $1
                 GROUP BY t.target ORDER BY count(*) DESC, t.target",
            )
            .bind(account.0)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let named = |rows: Vec<(String, i64)>| {
                rows.into_iter()
                    .map(|(name, n)| {
                        Language::from_name(&name)
                            .map(|l| (l, n as u64))
                            .ok_or_else(|| rows::corrupt("language", format!("unknown language {name:?}")))
                    })
                    .collect::<Result<Vec<_>, _>>()
            };
            Ok(LanguageUsage { languages: named(languages)?, targets: named(targets)? })
        })
    }

    fn settings(&self, account: AccountId) -> StoreResult<'_, UserSettings> {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT account_id, auto_delete_transcripts, unlimited_history, blocked, deleted_monthly
                 FROM transgate_jobs.settings WHERE account_id = $1",
            )
            .bind(account.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db)?;
            match row {
                Some(row) => rows::settings_from(&row),
                None => Ok(UserSettings::defaults(account)),
            }
        })
    }

    fn dispatch_candidates(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Job>> {
        Box::pin(self.jobs_where(
            concat!(
                "SELECT ",
                job_cols!(),
                " FROM transgate_jobs.jobs j
                 WHERE status IN ('not_started', 'pending')
                   AND (next_dispatch_at IS NULL OR next_dispatch_at <= transgate_jobs.us_to_ts($1))
                   AND NOT EXISTS (SELECT 1 FROM transgate_jobs.attempts a
                                   WHERE a.id = j.current_attempt
                                     AND (a.state IN ('accepted', 'uncertain')
                                          OR (a.state = 'claimed' AND a.lease_until > transgate_jobs.us_to_ts($1))))
                   AND NOT EXISTS (SELECT 1 FROM transgate_jobs.erasures e WHERE e.account_id = j.account_id)
                 ORDER BY status <> 'pending', created_at, id LIMIT $2"
            ),
            now.0,
            limit,
        ))
    }

    fn stuck_candidates(&self, started_before: Micros, limit: u32) -> StoreResult<'_, Vec<Job>> {
        Box::pin(self.jobs_where(
            concat!(
                "SELECT ",
                job_cols!(),
                " FROM transgate_jobs.jobs j
                 WHERE status = 'running' AND started_at < transgate_jobs.us_to_ts($1)
                   AND NOT EXISTS (SELECT 1 FROM transgate_jobs.erasures e WHERE e.account_id = j.account_id)
                 ORDER BY started_at, id LIMIT $2"
            ),
            started_before.0,
            limit,
        ))
    }

    fn unresolved_attempts(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Attempt>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                attempt_cols!(),
                " FROM transgate_jobs.attempts a
                 WHERE ((state = 'claimed' AND lease_until <= transgate_jobs.us_to_ts($1)) OR state = 'uncertain')
                   AND NOT EXISTS (SELECT 1 FROM transgate_jobs.erasures e WHERE e.account_id = a.account_id)
                 ORDER BY claimed_at, id LIMIT $2"
            ))
            .bind(now.0)
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            rows.iter().map(rows::attempt_from).collect()
        })
    }

    fn pending_inbox(&self, limit: u32) -> StoreResult<'_, Vec<InboxEntry>> {
        Box::pin(async move {
            // Entries of an erased account are included: ingestion settles them
            // as ignored and drops their bodies.
            let rows = sqlx::query(concat!(
                "SELECT ",
                inbox_cols!(),
                " FROM transgate_jobs.inbox WHERE state = 'pending' ORDER BY id LIMIT $1"
            ))
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            rows.iter().map(rows::inbox_from).collect()
        })
    }

    fn remote_cleanup_due(&self, limit: u32) -> StoreResult<'_, Vec<Attempt>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                attempt_cols!(),
                " FROM transgate_jobs.attempts WHERE remote_cleanup ->> 'state' = 'pending'
                 ORDER BY claimed_at, id LIMIT $1"
            ))
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            rows.iter().map(rows::attempt_from).collect()
        })
    }

    fn erasures_due(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<ErasureId>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                erasure_cols!(),
                " FROM transgate_jobs.erasures WHERE state IN ('requested', 'local_erased') ORDER BY requested_at, id"
            ))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let mut due = Vec::new();
            for row in &rows {
                if due.len() >= limit as usize {
                    break;
                }
                let record = rows::erasure_from(row)?;
                if record.state == ErasureState::Requested {
                    due.push(record.id);
                    continue;
                }
                let items = sqlx::query(
                    "SELECT id, erasure_id, target, state, attempts FROM transgate_jobs.erasure_items
                     WHERE erasure_id = $1 ORDER BY seq",
                )
                .bind(record.id.0)
                .fetch_all(&self.pool)
                .await
                .map_err(db)?
                .iter()
                .map(rows::item_from)
                .collect::<Result<Vec<_>, _>>()?;
                if !erasure::runnable(&items, now).is_empty() {
                    due.push(record.id);
                }
            }
            Ok(due)
        })
    }

    fn attempt(&self, id: AttemptId) -> StoreResult<'_, Option<Attempt>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", attempt_cols!(), " FROM transgate_jobs.attempts WHERE id = $1"))
                .bind(id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(db)?;
            row.as_ref().map(rows::attempt_from).transpose()
        })
    }

    fn barrier(&self, account: AccountId) -> StoreResult<'_, bool> {
        Box::pin(async move {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM transgate_jobs.erasures WHERE account_id = $1)")
                .bind(account.0)
                .fetch_one(&self.pool)
                .await
                .map_err(db)
        })
    }

    fn runnable_generations(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<GenerationId>> {
        Box::pin(async move {
            // Barrier accounts are included: the worker interrupts them as erased.
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT id FROM transgate_jobs.generations
                 WHERE status = 'accepted' OR (status = 'running' AND lease_until <= transgate_jobs.us_to_ts($1))
                 ORDER BY created_at, id LIMIT $2",
            )
            .bind(now.0)
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            Ok(ids.into_iter().map(GenerationId).collect())
        })
    }

    fn pending_summary_requests(&self, limit: u32) -> StoreResult<'_, Vec<(JobId, i64)>> {
        Box::pin(async move {
            let rows: Vec<(i64, i64)> = sqlx::query_as(
                "SELECT job_id, source_revision FROM transgate_jobs.summary_requests
                 WHERE outcome IS NULL ORDER BY requested_at, job_id, source_revision LIMIT $1",
            )
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            Ok(rows.into_iter().map(|(job, revision)| (JobId(job), revision)).collect())
        })
    }

    fn pending_ai_purges(&self, limit: u32) -> StoreResult<'_, Vec<AccountId>> {
        Box::pin(async move {
            let accounts: Vec<Uuid> = sqlx::query_scalar(
                "SELECT account_id FROM transgate_jobs.ai_consent WHERE purge_pending ORDER BY updated_at, account_id LIMIT $1",
            )
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            Ok(accounts.into_iter().map(AccountId).collect())
        })
    }
}

impl JobStore for PgJobStore {
    fn begin(&self) -> StoreResult<'_, Box<dyn JobUow>> {
        Box::pin(async move {
            Ok(Box::new(PgJobUow::begin(&self.pool).await?) as Box<dyn JobUow>)
        })
    }
}
