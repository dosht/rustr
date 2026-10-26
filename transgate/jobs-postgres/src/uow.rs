use billing_core::ports::{BillingTx, StoreError};
use billing_postgres::PgBillingTx;
use media_ports::{DeepgramMetadata, DeepgramResults, DeepgramTranscript, ObjectRef, ProviderObservation, ProviderPayload, RemoteIdentity, SonioxReported};
use media_postgres::LockKey;
use sqlx::types::Json;
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Postgres, Row, Transaction};
use transgate_core::ids::{AccountId, AttemptId, ErasureId, InboxId, JobId, LineId, Micros};
use transgate_core::jobs::domain::{Attempt, Job, JobSource, NewJob, Provider};
use transgate_core::jobs::events::JobEvent;
use transgate_core::jobs::ports::{
    ArchiveWrite, ArchivedPayload, BoxFuture, InboxEntry, InboxState, IssuedUpload, JobUow, NewInboxEntry, StoreResult,
};
use transgate_core::privacy::erasure::{AccountResources, ErasureItem, ErasureRecord, ErasureState};
use transgate_core::privacy::retention::{RetentionCandidate, RetentionRun};
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::{Line, NewLine};
use uuid::Uuid;

use crate::db;
use crate::rows::{self, attempt_cols, erasure_cols, from_text, inbox_cols, job_cols, line_cols, to_text};

/// One host transaction (READ COMMITTED) on one connection of the runtime pool.
pub struct PgJobUow {
    tx: Transaction<'static, Postgres>,
}

impl PgJobUow {
    pub async fn begin(pool: &PgPool) -> Result<PgJobUow, StoreError> {
        let mut tx = pool.begin().await.map_err(db)?;
        // Every lock here serializes only when each statement takes a fresh snapshot.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED").execute(&mut *tx).await.map_err(db)?;
        Ok(PgJobUow { tx })
    }

    pub(crate) fn conn(&mut self) -> &mut PgConnection {
        &mut self.tx
    }
}

fn names(languages: &[transgate_core::languages::Language]) -> Vec<&'static str> {
    languages.iter().map(|l| l.name()).collect()
}

fn event_account(event: &JobEvent) -> AccountId {
    match event {
        JobEvent::Completed { account, .. } | JobEvent::Failed { account, .. } => *account,
    }
}

/// A settled observation keeps its identity and reported duration only; the
/// transcript content is dropped (seam 6: the fingerprint stays).
fn without_body(observation: ProviderObservation) -> ProviderObservation {
    let payload = match observation.payload {
        ProviderPayload::Soniox { reported, .. } => ProviderPayload::Soniox {
            reported: SonioxReported {
                id: reported.id,
                status: reported.status,
                audio_duration_ms: reported.audio_duration_ms,
                error: None,
                error_type: reported.error_type,
                error_message: None,
                extra: Default::default(),
            },
            transcript: None,
        },
        ProviderPayload::Deepgram(t) => ProviderPayload::Deepgram(DeepgramTranscript {
            metadata: DeepgramMetadata { request_id: t.metadata.request_id, duration: t.metadata.duration, extra: Default::default() },
            results: DeepgramResults { channels: vec![], extra: Default::default() },
            extra: Default::default(),
        }),
    };
    ProviderObservation { remote: observation.remote, origin: observation.origin, payload }
}

fn valid_savepoint(name: &str) -> Result<(), StoreError> {
    if !name.is_empty() && name.len() <= 48 && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        Ok(())
    } else {
        Err(StoreError::Conflict(format!("{name:?} is not a usable savepoint name")))
    }
}

const RETENTION_PREDICATE: &str = "
    SELECT j.account_id, j.id, COALESCE(s.auto_delete_transcripts, true) AS auto_delete, count(l.id) AS line_count
    FROM transgate_jobs.jobs j
    JOIN transgate_jobs.lines l ON l.job_id = j.id
    LEFT JOIN transgate_jobs.settings s ON s.account_id = j.account_id
    WHERE j.status = 'completed' AND j.created_at <= transgate_jobs.us_to_ts($1)
      AND ($2::bigint[] IS NULL OR j.id = ANY($2))
    GROUP BY j.account_id, j.id, s.auto_delete_transcripts
    ORDER BY j.id";

impl PgJobUow {
    async fn retention_rows(&mut self, cutoff: Micros, only: Option<Vec<i64>>) -> Result<Vec<RetentionCandidate>, StoreError> {
        let rows: Vec<(Uuid, i64, bool, i64)> = sqlx::query_as(RETENTION_PREDICATE)
            .bind(cutoff.0)
            .bind(only)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
        Ok(rows
            .into_iter()
            .map(|(account, job, auto_delete, lines)| RetentionCandidate {
                account: AccountId(account),
                job: JobId(job),
                auto_delete,
                line_count: lines as u64,
            })
            .collect())
    }
}

impl JobUow for PgJobUow {
    fn api(&mut self) -> &mut dyn transgate_core::api::ports::ApiTx {
        self
    }

    fn ai(&mut self) -> &mut dyn transgate_core::ai::ports::AiTx {
        self
    }

    fn account(&mut self) -> &mut dyn transgate_core::account::ports::AccountTx {
        self
    }

    fn billing(&mut self) -> BoxFuture<'_, Result<Box<dyn BillingTx + '_>, StoreError>> {
        Box::pin(async move {
            let billing = PgBillingTx::attach(self.conn()).await?;
            Ok(Box::new(billing) as Box<dyn BillingTx + '_>)
        })
    }

    fn lock_capacity<'a>(&'a mut self, provider: media_ports::Provider, project: &'a str) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let key = LockKey::named(&format!("transgate:capacity:{}:{project}", to_text(&provider)?));
            sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(key.0).execute(self.conn()).await.map_err(db)?;
            Ok(())
        })
    }

    fn inflight<'a>(&'a mut self, provider: media_ports::Provider, project: &'a str, now: Micros) -> StoreResult<'a, u32> {
        Box::pin(async move {
            let products: Vec<&str> =
                Provider::ALL.into_iter().filter(|p| p.transport() == Some(provider)).map(|p| p.as_str()).collect();
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM transgate_jobs.attempts a JOIN transgate_jobs.jobs j ON j.id = a.job_id
                 WHERE a.provider = ANY($1) AND a.project = $2
                   AND (a.state IN ('accepted', 'uncertain')
                        OR (a.state = 'claimed' AND a.lease_until > transgate_jobs.us_to_ts($3)))
                   AND j.status IN ('not_started', 'pending', 'running')",
            )
            .bind(&products)
            .bind(project)
            .bind(now.0)
            .fetch_one(self.conn())
            .await
            .map_err(db)?;
            Ok(n as u32)
        })
    }

    fn lock_jobs<'a>(&'a mut self, account: AccountId, ids: &'a [JobId]) -> StoreResult<'a, Vec<Job>> {
        Box::pin(async move {
            let mut ids: Vec<i64> = ids.iter().map(|i| i.0).collect();
            ids.sort();
            ids.dedup();
            let rows = sqlx::query(concat!(
                "SELECT ",
                job_cols!(),
                " FROM transgate_jobs.jobs WHERE account_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE"
            ))
            .bind(account.0)
            .bind(&ids)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            rows.iter().map(rows::job_from).collect()
        })
    }

    fn lock_job(&mut self, id: JobId) -> StoreResult<'_, Option<Job>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", job_cols!(), " FROM transgate_jobs.jobs WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(rows::job_from).transpose()
        })
    }

    fn completed_job_count(&mut self, account: AccountId) -> StoreResult<'_, u64> {
        Box::pin(async move {
            let n: i64 = sqlx::query_scalar("SELECT count(*) FROM transgate_jobs.jobs WHERE account_id = $1 AND status = 'completed'")
                .bind(account.0)
                .fetch_one(self.conn())
                .await
                .map_err(db)?;
            Ok(n as u64)
        })
    }

    fn insert_job<'a>(&'a mut self, job: &'a NewJob) -> StoreResult<'a, Job> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "INSERT INTO transgate_jobs.jobs
                   (account_id, status, job_name, source, language, translate_to, speakers, fix_grammer,
                    duration_minutes, size_bytes, callback_url, whisper_params, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                         transgate_jobs.us_to_ts($13), transgate_jobs.us_to_ts($13))
                 RETURNING ",
                job_cols!()
            ))
            .bind(job.account.0)
            .bind(job.status.as_str())
            .bind(&job.name)
            .bind(Json(&job.source))
            .bind(job.language.map(|l| l.name()))
            .bind(names(&job.translate_to))
            .bind(job.speakers)
            .bind(job.fix_grammer)
            .bind(job.duration_minutes)
            .bind(job.size_bytes)
            .bind(&job.callback_url)
            .bind(&job.whisper_params)
            .bind(job.created_at.0)
            .fetch_one(self.conn())
            .await
            .map_err(db)?;
            rows::job_from(&row)
        })
    }

    fn save_job<'a>(&'a mut self, job: &'a Job, expected_revision: i64) -> StoreResult<'a, i64> {
        Box::pin(async move {
            let deferred = to_text(&job.deferred_charge)?;
            let revision: Option<i64> = sqlx::query_scalar(
                "UPDATE transgate_jobs.jobs SET
                   revision = revision + 1, status = $4, job_name = $5, source = $6, language = $7, translate_to = $8,
                   speakers = $9, fix_grammer = $10, duration_minutes = $11, size_bytes = $12, provider = $13,
                   model = $14, callback_url = $15, whisper_params = $16, error = $17, retry_count = $18,
                   recovery_attempts = $19, soniox_transcription_id = $20,
                   started_at = transgate_jobs.us_to_ts($21), updated_at = transgate_jobs.us_to_ts($22),
                   current_attempt = $23, attempts_made = $24, deferred_charge = $25,
                   next_dispatch_at = transgate_jobs.us_to_ts($26)
                 WHERE id = $1 AND account_id = $2 AND revision = $3
                 RETURNING revision",
            )
            .bind(job.id.0)
            .bind(job.account.0)
            .bind(expected_revision)
            .bind(job.status.as_str())
            .bind(&job.name)
            .bind(Json(&job.source))
            .bind(job.language.map(|l| l.name()))
            .bind(names(&job.translate_to))
            .bind(job.speakers)
            .bind(job.fix_grammer)
            .bind(job.duration_minutes)
            .bind(job.size_bytes)
            .bind(job.provider.map(|p| p.as_str()))
            .bind(&job.model)
            .bind(&job.callback_url)
            .bind(&job.whisper_params)
            .bind(&job.error)
            .bind(job.retry_count)
            .bind(job.recovery_attempts)
            .bind(&job.soniox_transcription_id)
            .bind(job.started_at.map(|t| t.0))
            .bind(job.updated_at.0)
            .bind(job.current_attempt.map(|a| a.0))
            .bind(job.attempts_made)
            .bind(deferred)
            .bind(job.next_dispatch_at.map(|t| t.0))
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            revision.ok_or_else(|| {
                StoreError::Conflict(format!("job {} changed since revision {expected_revision}", job.id))
            })
        })
    }

    fn record_issued_upload<'a>(&'a mut self, upload: &'a IssuedUpload) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // The capability belongs to the caller it was signed for, whoever owns
            // the named job and whether or not that job exists (UPL-009..012 as built).
            sqlx::query(
                "INSERT INTO transgate_jobs.issued_uploads (job_id, account_id, object, expires_at)
                 VALUES ($1, $2, $3, transgate_jobs.us_to_ts($4))",
            )
            .bind(upload.job.0)
            .bind(upload.account.0)
            .bind(Json(&upload.object))
            .bind(upload.expires_at.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn record_archived_payload<'a>(&'a mut self, payload: &'a ArchivedPayload) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // Recorded inside the apply transaction under the job lock, before the PUT
            // (F2); the row stays the account's even after its job row is deleted.
            sqlx::query(
                "INSERT INTO transgate_jobs.archived_payloads (job_id, account_id, object, write)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(payload.job.0)
            .bind(payload.account.0)
            .bind(Json(&payload.object))
            .bind(to_text(&payload.write)?)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn settle_archived_payload<'a>(&'a mut self, account: AccountId, object: &'a ObjectRef, write: ArchiveWrite) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // Only an in-flight intent settles; a settled one or no row at all is left as is.
            sqlx::query(
                "UPDATE transgate_jobs.archived_payloads SET write = $3
                 WHERE account_id = $1 AND object = $2 AND write = 'in_flight'",
            )
            .bind(account.0)
            .bind(Json(object))
            .bind(to_text(&write)?)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn hard_delete_job(&mut self, id: JobId) -> StoreResult<'_, ()> {
        Box::pin(async move {
            sqlx::query("DELETE FROM transgate_jobs.lines WHERE job_id = $1").bind(id.0).execute(self.conn()).await.map_err(db)?;
            // The job's AI children (source revision, generations, chat, summaries,
            // summary requests and transcript-scoped quota counters) and its feedback
            // go with it by ON DELETE CASCADE; account-wide quota counters and support
            // requests, which keep the job id only as context, stay.
            sqlx::query("DELETE FROM transgate_jobs.jobs WHERE id = $1").bind(id.0).execute(self.conn()).await.map_err(db)?;
            Ok(())
        })
    }

    fn insert_attempt<'a>(&'a mut self, attempt: &'a Attempt) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.attempts
                   (id, job_id, account_id, number, provider, project, model, state, claimed_at, lease_until,
                    request, callback_secret, remote_id, accepted_at, last_error, remote_cleanup)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, transgate_jobs.us_to_ts($9), transgate_jobs.us_to_ts($10),
                         $11, $12, $13, transgate_jobs.us_to_ts($14), $15, $16)",
            )
            .bind(attempt.id.0)
            .bind(attempt.job.0)
            .bind(attempt.account.0)
            .bind(attempt.number)
            .bind(attempt.provider.as_str())
            .bind(&attempt.project)
            .bind(&attempt.model)
            .bind(to_text(&attempt.state)?)
            .bind(attempt.claimed_at.0)
            .bind(attempt.lease_until.0)
            .bind(Json(&attempt.request))
            .bind(attempt.callback_secret.expose())
            .bind(&attempt.remote_id)
            .bind(attempt.accepted_at.map(|t| t.0))
            .bind(&attempt.last_error)
            .bind(Json(&attempt.remote_cleanup))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn save_attempt<'a>(&'a mut self, attempt: &'a Attempt) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_jobs.attempts
                 SET state = $2, lease_until = transgate_jobs.us_to_ts($3), remote_id = $4,
                     accepted_at = transgate_jobs.us_to_ts($5), last_error = $6, remote_cleanup = $7
                 WHERE id = $1",
            )
            .bind(attempt.id.0)
            .bind(to_text(&attempt.state)?)
            .bind(attempt.lease_until.0)
            .bind(&attempt.remote_id)
            .bind(attempt.accepted_at.map(|t| t.0))
            .bind(&attempt.last_error)
            .bind(Json(&attempt.remote_cleanup))
            .execute(self.conn())
            .await
            .map_err(db)?;
            if done.rows_affected() == 1 { Ok(()) } else { Err(StoreError::Conflict(format!("attempt {} does not exist", attempt.id))) }
        })
    }

    fn lock_attempt(&mut self, id: AttemptId) -> StoreResult<'_, Option<Attempt>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", attempt_cols!(), " FROM transgate_jobs.attempts WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(rows::attempt_from).transpose()
        })
    }

    fn attempts_of(&mut self, job: JobId) -> StoreResult<'_, Vec<Attempt>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!("SELECT ", attempt_cols!(), " FROM transgate_jobs.attempts WHERE job_id = $1 ORDER BY number"))
                .bind(job.0)
                .fetch_all(self.conn())
                .await
                .map_err(db)?;
            rows.iter().map(rows::attempt_from).collect()
        })
    }

    fn insert_lines<'a>(&'a mut self, job: &'a Job, lines: &'a [NewLine]) -> StoreResult<'a, u64> {
        Box::pin(async move {
            for line in lines {
                let status = line.translation_status.map(|s| s.as_str());
                sqlx::query(
                    "INSERT INTO transgate_jobs.lines
                       (job_id, account_id, start_time, end_time, line, speaker, corrected_text,
                        split_on_speaker_change, language, translation_status)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                )
                .bind(job.id.0)
                .bind(job.account.0)
                .bind(&line.start_time)
                .bind(&line.end_time)
                .bind(&line.line)
                .bind(&line.speaker)
                .bind(&line.corrected_text)
                .bind(line.split_on_speaker_change)
                .bind(line.language.map(|l| l.name()))
                .bind(status)
                .execute(self.conn())
                .await
                .map_err(db)?;
            }
            Ok(lines.len() as u64)
        })
    }

    fn lock_line(&mut self, id: LineId) -> StoreResult<'_, Option<Line>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", line_cols!(), " FROM transgate_jobs.lines WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(rows::line_from).transpose()
        })
    }

    fn update_line_text<'a>(&'a mut self, id: LineId, text: &'a str) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query("UPDATE transgate_jobs.lines SET line = $2 WHERE id = $1")
                .bind(id.0)
                .bind(text)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }

    fn update_line_speaker<'a>(&'a mut self, id: LineId, speaker: &'a str) -> StoreResult<'a, bool> {
        Box::pin(async move {
            let done = sqlx::query("UPDATE transgate_jobs.lines SET speaker = $2 WHERE id = $1")
                .bind(id.0)
                .bind(speaker)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }

    fn rename_speaker<'a>(&'a mut self, job: JobId, from: &'a str, to: &'a str) -> StoreResult<'a, u64> {
        Box::pin(async move {
            let done = sqlx::query("UPDATE transgate_jobs.lines SET speaker = $3 WHERE job_id = $1 AND speaker = $2")
                .bind(job.0)
                .bind(from)
                .bind(to)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(done.rows_affected())
        })
    }

    fn insert_inbox<'a>(&'a mut self, entry: &'a NewInboxEntry) -> StoreResult<'a, (InboxEntry, bool)> {
        Box::pin(async move {
            let inserted = sqlx::query(concat!(
                "INSERT INTO transgate_jobs.inbox
                   (attempt_id, job_id, account_id, remote_id, origin, observation, fingerprint, received_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, transgate_jobs.us_to_ts($8))
                 ON CONFLICT (attempt_id, remote_id) WHERE conflict_of IS NULL DO NOTHING
                 RETURNING ",
                inbox_cols!()
            ))
            .bind(entry.attempt.0)
            .bind(entry.job.0)
            .bind(entry.account.0)
            .bind(&entry.remote_id)
            .bind(to_text(&entry.origin)?)
            .bind(Json(&entry.observation))
            .bind(&entry.fingerprint)
            .bind(entry.received_at.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            if let Some(row) = inserted {
                return Ok((rows::inbox_from(&row)?, true));
            }
            let existing = sqlx::query(concat!(
                "SELECT ",
                inbox_cols!(),
                " FROM transgate_jobs.inbox WHERE attempt_id = $1 AND remote_id = $2 AND conflict_of IS NULL"
            ))
            .bind(entry.attempt.0)
            .bind(&entry.remote_id)
            .fetch_one(self.conn())
            .await
            .map_err(db)?;
            Ok((rows::inbox_from(&existing)?, false))
        })
    }

    fn hold_inbox_conflict<'a>(&'a mut self, of: InboxId, entry: &'a NewInboxEntry) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.inbox
                   (attempt_id, job_id, account_id, remote_id, origin, observation, fingerprint, received_at,
                    state, settled_at, note, conflict_of)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, transgate_jobs.us_to_ts($8),
                         'conflict', transgate_jobs.us_to_ts($8), $9, $10)
                 ON CONFLICT (conflict_of, fingerprint) WHERE conflict_of IS NOT NULL DO NOTHING",
            )
            .bind(entry.attempt.0)
            .bind(entry.job.0)
            .bind(entry.account.0)
            .bind(&entry.remote_id)
            .bind(to_text(&entry.origin)?)
            .bind(Json(&entry.observation))
            .bind(&entry.fingerprint)
            .bind(entry.received_at.0)
            .bind(format!("conflicts with inbox {}", of.0))
            .bind(of.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn lock_inbox(&mut self, id: InboxId) -> StoreResult<'_, Option<InboxEntry>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", inbox_cols!(), " FROM transgate_jobs.inbox WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(rows::inbox_from).transpose()
        })
    }

    fn settle_inbox<'a>(&'a mut self, id: InboxId, state: InboxState, note: Option<&'a str>, at: Micros) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let observation: Option<Json<ProviderObservation>> =
                sqlx::query_scalar("SELECT observation FROM transgate_jobs.inbox WHERE id = $1 FOR UPDATE")
                    .bind(id.0)
                    .fetch_optional(self.conn())
                    .await
                    .map_err(db)?;
            let Some(Json(observation)) = observation else {
                return Err(StoreError::Conflict(format!("inbox entry {id} does not exist")));
            };
            let observation = match state {
                InboxState::Applied | InboxState::Ignored => without_body(observation),
                InboxState::Pending | InboxState::Conflict => observation,
            };
            sqlx::query(
                "UPDATE transgate_jobs.inbox
                 SET state = $2, note = $3, settled_at = transgate_jobs.us_to_ts($4), observation = $5
                 WHERE id = $1",
            )
            .bind(id.0)
            .bind(to_text(&state)?)
            .bind(note)
            .bind(at.0)
            .bind(Json(&observation))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn record_inbox_failure<'a>(&'a mut self, id: InboxId, error: &'a str) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query("UPDATE transgate_jobs.inbox SET failures = failures + 1, last_error = $2 WHERE id = $1")
                .bind(id.0)
                .bind(error)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(())
        })
    }

    fn emit<'a>(&'a mut self, event: &'a JobEvent) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query("INSERT INTO transgate_jobs.events (account_id, event) VALUES ($1, $2)")
                .bind(event_account(event).0)
                .bind(Json(event))
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(())
        })
    }

    fn lock_settings(&mut self, account: AccountId) -> StoreResult<'_, UserSettings> {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT account_id, auto_delete_transcripts, unlimited_history, blocked, deleted_monthly
                 FROM transgate_jobs.settings WHERE account_id = $1 FOR UPDATE",
            )
            .bind(account.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            match row {
                Some(row) => rows::settings_from(&row),
                None => Ok(UserSettings::defaults(account)),
            }
        })
    }

    fn save_settings<'a>(&'a mut self, settings: &'a UserSettings) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // Only the user flags are written; the operator fields keep their stored values.
            sqlx::query(
                "INSERT INTO transgate_jobs.settings (account_id, auto_delete_transcripts, deleted_monthly)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (account_id) DO UPDATE
                 SET auto_delete_transcripts = EXCLUDED.auto_delete_transcripts, deleted_monthly = EXCLUDED.deleted_monthly",
            )
            .bind(settings.account.0)
            .bind(settings.auto_delete_transcripts)
            .bind(settings.deleted_monthly)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn retention_candidates(&mut self, cutoff: Micros) -> StoreResult<'_, Vec<RetentionCandidate>> {
        Box::pin(self.retention_rows(cutoff, None))
    }

    fn lock_retention_set<'a>(&'a mut self, candidates: &'a [RetentionCandidate], cutoff: Micros) -> StoreResult<'a, Vec<RetentionCandidate>> {
        Box::pin(async move {
            let mut accounts: Vec<Uuid> = candidates.iter().map(|c| c.account.0).collect();
            accounts.sort();
            accounts.dedup();
            let mut jobs: Vec<i64> = candidates.iter().map(|c| c.job.0).collect();
            jobs.sort();
            jobs.dedup();
            // Missing settings retain the source's default-on behavior;
            // the caller's account lock also fences concurrent preference writes.
            sqlx::query("SELECT 1 FROM transgate_jobs.settings WHERE account_id = ANY($1) ORDER BY account_id FOR UPDATE")
                .bind(&accounts)
                .fetch_all(self.conn())
                .await
                .map_err(db)?;
            sqlx::query("SELECT 1 FROM transgate_jobs.jobs WHERE id = ANY($1) ORDER BY id FOR UPDATE")
                .bind(&jobs)
                .fetch_all(self.conn())
                .await
                .map_err(db)?;
            self.retention_rows(cutoff, Some(jobs)).await
        })
    }

    fn delete_lines_of<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, u64> {
        Box::pin(async move {
            let ids: Vec<i64> = jobs.iter().map(|j| j.0).collect();
            let done = sqlx::query("DELETE FROM transgate_jobs.lines WHERE job_id = ANY($1)")
                .bind(&ids)
                .execute(self.conn())
                .await
                .map_err(db)?;
            // P7 D10: a stored callback body copied these lines; it goes with them. The
            // dispatcher rebuilds the body from current content on every attempt.
            sqlx::query("UPDATE transgate_api.callback_deliveries SET payload = NULL WHERE job_id = ANY($1) AND payload IS NOT NULL")
                .bind(&ids)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(done.rows_affected())
        })
    }

    fn opted_out_count(&mut self, cutoff: Micros) -> StoreResult<'_, u64> {
        Box::pin(async move {
            let n: i64 = sqlx::query_scalar(
                "SELECT count(DISTINCT j.account_id) FROM transgate_jobs.jobs j
                 LEFT JOIN transgate_jobs.settings s ON s.account_id = j.account_id
                 WHERE j.status = 'completed' AND j.created_at <= transgate_jobs.us_to_ts($1)
                   AND NOT COALESCE(s.auto_delete_transcripts, true)
                   AND EXISTS (SELECT 1 FROM transgate_jobs.lines l WHERE l.job_id = j.id)",
            )
            .bind(cutoff.0)
            .fetch_one(self.conn())
            .await
            .map_err(db)?;
            Ok(n as u64)
        })
    }

    fn insert_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()> {
        Box::pin(async move { upsert_run(self.conn(), run).await })
    }

    fn save_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()> {
        Box::pin(async move { upsert_run(self.conn(), run).await })
    }

    fn fail_interrupted_runs<'a>(&'a mut self, job_name: &'a str, message: &'a str, _at: Micros) -> StoreResult<'a, u64> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_jobs.retention_runs SET status = 'failure', error_message = $2, rows_deleted = 0
                 WHERE job_name = $1 AND status = 'running'",
            )
            .bind(job_name)
            .bind(message)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(done.rows_affected())
        })
    }

    fn history_count<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64> {
        Box::pin(async move {
            media_postgres::count_finished_slots(self.conn(), schedules, cutoff.0)
                .await
                .map_err(|e| StoreError::Unavailable(e.to_string()))
        })
    }

    fn delete_history<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64> {
        Box::pin(async move {
            media_postgres::delete_finished_slots(self.conn(), schedules, cutoff.0)
                .await
                .map_err(|e| StoreError::Unavailable(e.to_string()))
        })
    }

    fn lock_barrier(&mut self, account: AccountId) -> StoreResult<'_, Option<ErasureRecord>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", erasure_cols!(), " FROM transgate_jobs.erasures WHERE account_id = $1 FOR UPDATE"))
                .bind(account.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(rows::erasure_from).transpose()
        })
    }

    fn insert_erasure<'a>(&'a mut self, record: &'a ErasureRecord) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // One barrier per account: a concurrent second request fails here (Conflict).
            sqlx::query(
                "INSERT INTO transgate_jobs.erasures
                   (id, account_id, requested_at, state, local_erased_at, identity_deleted_at, closed_at)
                 VALUES ($1, $2, transgate_jobs.us_to_ts($3), $4, transgate_jobs.us_to_ts($5),
                         transgate_jobs.us_to_ts($6), transgate_jobs.us_to_ts($7))",
            )
            .bind(record.id.0)
            .bind(record.account.0)
            .bind(record.requested_at.0)
            .bind(to_text(&record.state)?)
            .bind(record.local_erased_at.map(|t| t.0))
            .bind(record.identity_deleted_at.map(|t| t.0))
            .bind(record.closed_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn insert_erasure_items<'a>(&'a mut self, items: &'a [ErasureItem]) -> StoreResult<'a, ()> {
        Box::pin(async move {
            for item in items {
                sqlx::query(
                    "INSERT INTO transgate_jobs.erasure_items (id, erasure_id, target, state, attempts)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(item.id)
                .bind(item.erasure.0)
                .bind(Json(&item.target))
                .bind(Json(&item.state))
                .bind(item.attempts)
                .execute(self.conn())
                .await
                .map_err(db)?;
            }
            Ok(())
        })
    }

    fn lock_erasure(&mut self, id: ErasureId) -> StoreResult<'_, Option<ErasureRecord>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", erasure_cols!(), " FROM transgate_jobs.erasures WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(rows::erasure_from).transpose()
        })
    }

    fn save_erasure<'a>(&'a mut self, record: &'a ErasureRecord) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_jobs.erasures
                 SET state = $2, local_erased_at = transgate_jobs.us_to_ts($3),
                     identity_deleted_at = transgate_jobs.us_to_ts($4), closed_at = transgate_jobs.us_to_ts($5)
                 WHERE id = $1",
            )
            .bind(record.id.0)
            .bind(to_text(&record.state)?)
            .bind(record.local_erased_at.map(|t| t.0))
            .bind(record.identity_deleted_at.map(|t| t.0))
            .bind(record.closed_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            if done.rows_affected() != 1 {
                return Err(StoreError::Conflict(format!("erasure {} does not exist", record.id)));
            }
            // D9: once closed, confirmed and absent items drop their target details;
            // approved exceptions stay listed so the answer remains truthful.
            if matches!(record.state, ErasureState::FullyErased | ErasureState::ErasedWithExceptions) {
                sqlx::query(
                    "DELETE FROM transgate_jobs.erasure_items
                     WHERE erasure_id = $1 AND state ->> 'state' IN ('confirmed', 'absent')",
                )
                .bind(record.id.0)
                .execute(self.conn())
                .await
                .map_err(db)?;
            }
            Ok(())
        })
    }

    fn erasure_items(&mut self, id: ErasureId) -> StoreResult<'_, Vec<ErasureItem>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id, erasure_id, target, state, attempts FROM transgate_jobs.erasure_items
                 WHERE erasure_id = $1 ORDER BY seq",
            )
            .bind(id.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            rows.iter().map(rows::item_from).collect()
        })
    }

    fn save_erasure_item<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query("UPDATE transgate_jobs.erasure_items SET state = $2, attempts = $3 WHERE id = $1")
                .bind(item.id)
                .bind(Json(&item.state))
                .bind(item.attempts)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(())
        })
    }

    fn save_erasure_item_target<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // Late LLM dispatch identity only (D9): the stored and the new target must
            // both be llm_dispatch items, so no other manifest item can be rewritten.
            let done = sqlx::query(
                "UPDATE transgate_jobs.erasure_items SET target = $2, state = $3, attempts = $4
                 WHERE id = $1 AND target ->> 'kind' = 'llm_dispatch' AND $2::jsonb ->> 'kind' = 'llm_dispatch'",
            )
            .bind(item.id)
            .bind(Json(&item.target))
            .bind(Json(&item.state))
            .bind(item.attempts)
            .execute(self.conn())
            .await
            .map_err(db)?;
            if done.rows_affected() == 1 {
                Ok(())
            } else {
                Err(StoreError::Conflict(format!("erasure item {} is not an existing LLM dispatch item", item.id)))
            }
        })
    }

    fn account_resources(&mut self, account: AccountId) -> StoreResult<'_, AccountResources> {
        Box::pin(async move {
            let jobs: Vec<(i64, Json<JobSource>)> =
                sqlx::query_as("SELECT id, source FROM transgate_jobs.jobs WHERE account_id = $1 ORDER BY id")
                    .bind(account.0)
                    .fetch_all(self.conn())
                    .await
                    .map_err(db)?;
            let mut media: Vec<ObjectRef> = Vec::new();
            for (_, Json(source)) in &jobs {
                match source {
                    JobSource::Upload { object: Some(object), .. } | JobSource::ExternalUrl { staged: Some(object), .. } => {
                        media.push(object.clone())
                    }
                    _ => {}
                }
            }
            let archived: Vec<(i64, Json<ObjectRef>, String)> = sqlx::query_as(
                "SELECT job_id, object, write FROM transgate_jobs.archived_payloads
                 WHERE account_id = $1 ORDER BY id",
            )
            .bind(account.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            // Every attempt, current or superseded, of every job the account ever
            // had, including hard-deleted drafts.
            let attempts = sqlx::query(
                "SELECT provider, project, remote_id, state, request ->> 'client_reference' AS client_reference
                 FROM transgate_jobs.attempts WHERE account_id = $1 ORDER BY claimed_at, id",
            )
            .bind(account.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            let mut provider_copies = Vec::new();
            let mut unresolved_submissions = Vec::new();
            for row in &attempts {
                let name: String = row.try_get("provider").map_err(|e| rows::corrupt("provider", e))?;
                let provider = Provider::parse(&name).ok_or_else(|| rows::corrupt("provider", &name))?;
                let Some(transport) = provider.transport() else { continue };
                let remote_id: Option<String> = row.try_get("remote_id").map_err(|e| rows::corrupt("remote id", e))?;
                let state: String = row.try_get("state").map_err(|e| rows::corrupt("attempt state", e))?;
                match remote_id {
                    Some(id) => {
                        let project: String = row.try_get("project").map_err(|e| rows::corrupt("project", e))?;
                        provider_copies.push((provider, RemoteIdentity { provider: transport, project, id }));
                    }
                    // Acceptance unknown: the call may have reached the provider.
                    None if state == "claimed" || state == "uncertain" => {
                        let reference: Option<String> =
                            row.try_get("client_reference").map_err(|e| rows::corrupt("client reference", e))?;
                        if let Some(reference) = reference {
                            unresolved_submissions.push((provider, reference));
                        }
                    }
                    None => {}
                }
            }
            let uploads: Vec<(i64, Json<ObjectRef>, i64)> = sqlx::query_as(
                "SELECT job_id, object, transgate_jobs.ts_to_us(expires_at) FROM transgate_jobs.issued_uploads
                 WHERE account_id = $1 ORDER BY id",
            )
            .bind(account.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            // Every dispatch of the account, whatever happened to its job or generation.
            let llm_dispatches = sqlx::query(
                "SELECT generation_id, account_id, provider, model, disposition, remote_ids, unkept_id_observed,
                        transgate_jobs.ts_to_us(created_at) AS created_us
                 FROM transgate_jobs.llm_dispatches WHERE account_id = $1 ORDER BY created_at, generation_id",
            )
            .bind(account.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?
            .iter()
            .map(crate::ai::dispatch_from)
            .collect::<Result<Vec<_>, _>>()?;
            Ok(AccountResources {
                jobs: jobs.iter().map(|(id, _)| JobId(*id)).collect(),
                media,
                archived_payloads: archived
                    .into_iter()
                    .map(|(job, object, write)| {
                        Ok(ArchivedPayload { account, job: JobId(job), object: object.0, write: from_text(&write, "archive write")? })
                    })
                    .collect::<Result<_, StoreError>>()?,
                provider_copies,
                unresolved_submissions,
                issued_uploads: uploads
                    .into_iter()
                    .map(|(job, object, expires)| IssuedUpload { account, job: JobId(job), object: object.0, expires_at: Micros(expires) })
                    .collect(),
                llm_dispatches,
            })
        })
    }

    fn erase_account_rows(&mut self, account: AccountId) -> StoreResult<'_, ()> {
        Box::pin(async move {
            for statement in [
                "DELETE FROM transgate_jobs.lines
                 WHERE account_id = $1 OR job_id IN (SELECT id FROM transgate_jobs.jobs WHERE account_id = $1)",
                "DELETE FROM transgate_jobs.inbox WHERE account_id = $1",
                "DELETE FROM transgate_jobs.attempts WHERE account_id = $1",
                "DELETE FROM transgate_jobs.issued_uploads WHERE account_id = $1",
                "DELETE FROM transgate_jobs.archived_payloads WHERE account_id = $1",
                "DELETE FROM transgate_jobs.events WHERE account_id = $1",
                "DELETE FROM transgate_jobs.chat_messages WHERE account_id = $1",
                "DELETE FROM transgate_jobs.completion_summaries WHERE account_id = $1",
                "DELETE FROM transgate_jobs.summary_requests WHERE account_id = $1",
                "DELETE FROM transgate_jobs.generations WHERE account_id = $1",
                "DELETE FROM transgate_jobs.jobs WHERE account_id = $1",
                "DELETE FROM transgate_jobs.settings WHERE account_id = $1",
                // Content-free quota and consent survive refusal and revocation, not erasure (D3).
                "DELETE FROM transgate_jobs.quota_counters WHERE account_id = $1",
                "DELETE FROM transgate_jobs.ai_consent WHERE account_id = $1",
                // The dispatch ledger outlives purge and hard deletion; the manifest
                // captured it from account_resources earlier in this transaction.
                "DELETE FROM transgate_jobs.llm_dispatches WHERE account_id = $1",
                "DELETE FROM transgate_jobs.job_feedback WHERE account_id = $1",
                // Undelivered support requests and their messages (P5 D7).
                "DELETE FROM transgate_jobs.support_requests WHERE account_id = $1",
                // P7 (D10): keys with their audit rows and rate windows, idempotency records,
                // deliveries with their attempts and stored payloads, and the signing secret.
                "DELETE FROM transgate_api.callback_deliveries WHERE account_id = $1",
                "DELETE FROM transgate_api.idempotency WHERE account_id = $1",
                "DELETE FROM transgate_api.signing_secrets WHERE account_id = $1",
                "DELETE FROM transgate_api.key_uses WHERE account_id = $1",
                "DELETE FROM transgate_api.api_keys WHERE account_id = $1",
            ] {
                sqlx::query(statement).bind(account.0).execute(self.conn()).await.map_err(db)?;
            }
            // The host customer directory row (transgate-billing), where that schema is installed.
            let directory: bool = sqlx::query_scalar("SELECT to_regclass('transgate_billing.customers') IS NOT NULL")
                .fetch_one(self.conn())
                .await
                .map_err(db)?;
            if directory {
                // Exclusive against the shared lock its insert/update trigger takes before
                // checking the barrier, so an in-flight refresh cannot recreate the row.
                sqlx::query("SELECT pg_advisory_xact_lock(transgate_billing.customer_lock_key($1))")
                    .bind(account.0)
                    .execute(self.conn())
                    .await
                    .map_err(db)?;
                sqlx::query("DELETE FROM transgate_billing.customers WHERE account_id = $1")
                    .bind(account.0)
                    .execute(self.conn())
                    .await
                    .map_err(db)?;
            }
            Ok(())
        })
    }

    fn savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        Box::pin(async move {
            valid_savepoint(name)?;
            // `name` is restricted to [a-z_] above.
            sqlx::query(AssertSqlSafe(format!("SAVEPOINT {name}"))).execute(self.conn()).await.map_err(db)?;
            Ok(())
        })
    }

    fn rollback_to_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        Box::pin(async move {
            valid_savepoint(name)?;
            sqlx::query(AssertSqlSafe(format!("ROLLBACK TO SAVEPOINT {name}"))).execute(self.conn()).await.map_err(db)?;
            Ok(())
        })
    }

    fn release_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        Box::pin(async move {
            valid_savepoint(name)?;
            sqlx::query(AssertSqlSafe(format!("RELEASE SAVEPOINT {name}"))).execute(self.conn()).await.map_err(db)?;
            Ok(())
        })
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        Box::pin(async move { self.tx.commit().await.map_err(db) })
    }

    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        Box::pin(async move { self.tx.rollback().await.map_err(db) })
    }
}

async fn upsert_run(conn: &mut PgConnection, run: &RetentionRun) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO transgate_jobs.retention_runs
           (id, job_name, table_name, retention_days, cutoff, rows_deleted, execution_time_ms, status,
            error_message, users_opted_out_count, executed_at, executed_by)
         VALUES ($1, $2, $3, $4, transgate_jobs.us_to_ts($5), $6, $7, $8, $9, $10, transgate_jobs.us_to_ts($11), $12)
         ON CONFLICT (id) DO UPDATE
         SET rows_deleted = EXCLUDED.rows_deleted, execution_time_ms = EXCLUDED.execution_time_ms,
             status = EXCLUDED.status, error_message = EXCLUDED.error_message,
             users_opted_out_count = EXCLUDED.users_opted_out_count",
    )
    .bind(run.id)
    .bind(&run.job_name)
    .bind(&run.table_name)
    .bind(i32::from(run.retention_days))
    .bind(run.cutoff.0)
    .bind(run.rows_deleted as i64)
    .bind(run.execution_time_ms as i64)
    .bind(run.status.as_str())
    .bind(&run.error_message)
    .bind(run.users_opted_out_count.map(|n| n as i64))
    .bind(run.executed_at.0)
    .bind(&run.executed_by)
    .execute(conn)
    .await
    .map_err(db)?;
    Ok(())
}
