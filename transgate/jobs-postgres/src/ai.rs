//! `AiTx` on the same connection and transaction as the `PgJobUow` that lends
//! it. The caller holds the billing account lock; nothing here locks the
//! account or a job, and nothing opens network I/O.

use billing_core::ports::StoreError;
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use transgate_core::ai::GenerationId;
use transgate_core::ai::ports::{
    AiTx, ChatMessage, CompletionSummary, ConsentRecord, CounterKey, CounterScope, DispatchDisposition, Execution,
    Generation, GenerationKind, LlmDispatch, MAX_DISPATCH_REMOTE_ID_BYTES, MAX_DISPATCH_REMOTE_IDS, QuotaCounter,
    SummaryRequest,
};
use transgate_core::ids::{AccountId, JobId, Micros};
use transgate_core::jobs::ports::StoreResult;
use uuid::Uuid;

use crate::db;
use crate::rows::{corrupt, from_text, get, to_text};
use crate::uow::PgJobUow;

macro_rules! generation_cols {
    () => {
        "id, account_id, job_id, kind, request_key, request_digest, source_revision,
         COALESCE(consent_epoch, 0) AS consent_epoch, input, status, sequence, draft, staged, execution_token,
         transgate_jobs.ts_to_us(lease_until) AS lease_us, quota, interruption, remote_outcome_unknown, remote_id, tokens_used,
         transgate_jobs.ts_to_us(created_at) AS created_us, transgate_jobs.ts_to_us(updated_at) AS updated_us,
         transgate_jobs.ts_to_us(finished_at) AS finished_us"
    };
}

macro_rules! chat_cols {
    () => {
        "id, account_id, job_id, generation_id, message_order, role, content, COALESCE(consent_epoch, 0) AS consent_epoch,
         transgate_jobs.ts_to_us(created_at) AS created_us"
    };
}

macro_rules! consent_cols {
    () => {
        "account_id, has_consented, version, provider, policy_url,
         transgate_jobs.ts_to_us(consent_date) AS consent_us, transgate_jobs.ts_to_us(revoked_date) AS revoked_us,
         epoch, purge_cutoff, purge_pending, transgate_jobs.ts_to_us(updated_at) AS updated_us"
    };
}

macro_rules! summary_request_cols {
    () => {
        "job_id, source_revision, account_id, transgate_jobs.ts_to_us(requested_at) AS requested_us, generation_id,
         outcome, transgate_jobs.ts_to_us(decided_at) AS decided_us"
    };
}

macro_rules! counter_cols {
    () => {
        "account_id, scope, job_id, kind, period, count, max_limit, transgate_jobs.ts_to_us(reset_at) AS reset_us"
    };
}

fn generation_from(row: &PgRow) -> Result<Generation, StoreError> {
    let kind: String = get(row, "kind")?;
    let status: String = get(row, "status")?;
    let token: Option<Uuid> = get(row, "execution_token")?;
    let lease: Option<i64> = get(row, "lease_us")?;
    let execution = match (token, lease) {
        (Some(token), Some(lease)) => Some(Execution { token, lease_until: Micros(lease) }),
        (None, None) => None,
        _ => return Err(corrupt("generation execution", "token and lease disagree")),
    };
    Ok(Generation {
        id: GenerationId(get(row, "id")?),
        account: AccountId(get(row, "account_id")?),
        job: JobId(get(row, "job_id")?),
        kind: from_text(&kind, "generation kind")?,
        request_key: get(row, "request_key")?,
        request_digest: get(row, "request_digest")?,
        source_revision: get(row, "source_revision")?,
        consent_epoch: get(row, "consent_epoch")?,
        input: get::<Json<_>>(row, "input")?.0,
        status: from_text(&status, "generation status")?,
        sequence: get(row, "sequence")?,
        draft: get::<Json<_>>(row, "draft")?.0,
        staged: get::<Option<Json<_>>>(row, "staged")?.map(|j| j.0),
        execution,
        quota: get::<Option<Json<_>>>(row, "quota")?.map(|j| j.0),
        interruption: get::<Option<Json<_>>>(row, "interruption")?.map(|j| j.0),
        remote_outcome_unknown: get(row, "remote_outcome_unknown")?,
        remote_id: get(row, "remote_id")?,
        tokens_used: get(row, "tokens_used")?,
        created_at: Micros(get(row, "created_us")?),
        updated_at: Micros(get(row, "updated_us")?),
        finished_at: get::<Option<i64>>(row, "finished_us")?.map(Micros),
    })
}

fn chat_from(row: &PgRow) -> Result<ChatMessage, StoreError> {
    let role: String = get(row, "role")?;
    Ok(ChatMessage {
        id: get(row, "id")?,
        account: AccountId(get(row, "account_id")?),
        job: JobId(get(row, "job_id")?),
        generation: GenerationId(get(row, "generation_id")?),
        order: get(row, "message_order")?,
        role: from_text(&role, "chat role")?,
        content: get(row, "content")?,
        consent_epoch: get(row, "consent_epoch")?,
        created_at: Micros(get(row, "created_us")?),
    })
}

fn consent_from(row: &PgRow) -> Result<ConsentRecord, StoreError> {
    Ok(ConsentRecord {
        account: AccountId(get(row, "account_id")?),
        has_consented: get(row, "has_consented")?,
        version: get(row, "version")?,
        provider: get(row, "provider")?,
        policy_url: get(row, "policy_url")?,
        consent_date: get::<Option<i64>>(row, "consent_us")?.map(Micros),
        revoked_date: get::<Option<i64>>(row, "revoked_us")?.map(Micros),
        epoch: get(row, "epoch")?,
        purge_cutoff: get(row, "purge_cutoff")?,
        purge_pending: get(row, "purge_pending")?,
        updated_at: Micros(get(row, "updated_us")?),
    })
}

fn summary_request_from(row: &PgRow) -> Result<SummaryRequest, StoreError> {
    Ok(SummaryRequest {
        account: AccountId(get(row, "account_id")?),
        job: JobId(get(row, "job_id")?),
        source_revision: get(row, "source_revision")?,
        requested_at: Micros(get(row, "requested_us")?),
        generation: get::<Option<Uuid>>(row, "generation_id")?.map(GenerationId),
        outcome: get::<Option<Json<_>>>(row, "outcome")?.map(|j| j.0),
        decided_at: get::<Option<i64>>(row, "decided_us")?.map(Micros),
    })
}

fn counter_from(row: &PgRow) -> Result<QuotaCounter, StoreError> {
    let scope: String = get(row, "scope")?;
    let job: Option<i64> = get(row, "job_id")?;
    let kind: String = get(row, "kind")?;
    let period: String = get(row, "period")?;
    let scope = match (scope.as_str(), job) {
        ("transcript", Some(job)) => CounterScope::Transcript(JobId(job)),
        ("user", None) => CounterScope::User,
        _ => return Err(corrupt("counter scope", &scope)),
    };
    Ok(QuotaCounter {
        key: CounterKey {
            account: AccountId(get(row, "account_id")?),
            scope,
            kind: from_text(&kind, "counter kind")?,
            period: from_text(&period, "counter period")?,
        },
        count: get(row, "count")?,
        max_limit: get(row, "max_limit")?,
        reset_at: get::<Option<i64>>(row, "reset_us")?.map(Micros),
    })
}

pub(crate) fn dispatch_from(row: &PgRow) -> Result<LlmDispatch, StoreError> {
    let disposition: String = get(row, "disposition")?;
    Ok(LlmDispatch {
        account: AccountId(get(row, "account_id")?),
        generation: GenerationId(get(row, "generation_id")?),
        provider: get(row, "provider")?,
        model: get(row, "model")?,
        disposition: from_text(&disposition, "dispatch disposition")?,
        remote_ids: get(row, "remote_ids")?,
        unkept_id_observed: get(row, "unkept_id_observed")?,
        created_at: Micros(get(row, "created_us")?),
    })
}

fn scope_columns(scope: CounterScope) -> (&'static str, Option<i64>) {
    match scope {
        CounterScope::Transcript(job) => ("transcript", Some(job.0)),
        CounterScope::User => ("user", None),
    }
}

fn one_generation(row: Option<PgRow>) -> Result<Option<Generation>, StoreError> {
    row.as_ref().map(generation_from).transpose()
}

impl AiTx for PgJobUow {
    fn consent(&mut self, account: AccountId) -> StoreResult<'_, Option<ConsentRecord>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", consent_cols!(), " FROM transgate_jobs.ai_consent WHERE account_id = $1"))
                .bind(account.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            row.as_ref().map(consent_from).transpose()
        })
    }

    fn save_consent<'a>(&'a mut self, record: &'a ConsentRecord) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.ai_consent
                   (account_id, has_consented, version, provider, policy_url, consent_date, revoked_date, epoch,
                    purge_cutoff, purge_pending, updated_at)
                 VALUES ($1, $2, $3, $4, $5, transgate_jobs.us_to_ts($6), transgate_jobs.us_to_ts($7), $8, $9, $10,
                         transgate_jobs.us_to_ts($11))
                 ON CONFLICT (account_id) DO UPDATE SET
                   has_consented = EXCLUDED.has_consented, version = EXCLUDED.version, provider = EXCLUDED.provider,
                   policy_url = EXCLUDED.policy_url, consent_date = EXCLUDED.consent_date,
                   revoked_date = EXCLUDED.revoked_date, epoch = EXCLUDED.epoch, purge_cutoff = EXCLUDED.purge_cutoff,
                   purge_pending = EXCLUDED.purge_pending, updated_at = EXCLUDED.updated_at",
            )
            .bind(record.account.0)
            .bind(record.has_consented)
            .bind(&record.version)
            .bind(&record.provider)
            .bind(&record.policy_url)
            .bind(record.consent_date.map(|t| t.0))
            .bind(record.revoked_date.map(|t| t.0))
            .bind(record.epoch)
            .bind(record.purge_cutoff)
            .bind(record.purge_pending)
            .bind(record.updated_at.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn source_revision(&mut self, job: JobId) -> StoreResult<'_, i64> {
        Box::pin(async move {
            let revision: Option<i64> = sqlx::query_scalar("SELECT revision FROM transgate_jobs.source_revisions WHERE job_id = $1")
                .bind(job.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            Ok(revision.unwrap_or(0))
        })
    }

    fn bump_source_revision(&mut self, job: JobId) -> StoreResult<'_, i64> {
        Box::pin(async move {
            // A missing job fails the foreign key (Conflict): a revision never outlives its job.
            sqlx::query_scalar(
                "INSERT INTO transgate_jobs.source_revisions (job_id, revision) VALUES ($1, 1)
                 ON CONFLICT (job_id) DO UPDATE SET revision = source_revisions.revision + 1
                 RETURNING revision",
            )
            .bind(job.0)
            .fetch_one(self.conn())
            .await
            .map_err(db)
        })
    }

    fn bump_source_revisions<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // One +1 per listed job: a repeated id would make ON CONFLICT touch a row twice.
            let mut ids: Vec<i64> = jobs.iter().map(|j| j.0).collect();
            ids.sort();
            ids.dedup();
            sqlx::query(
                "INSERT INTO transgate_jobs.source_revisions (job_id, revision)
                 SELECT job_id, 1 FROM unnest($1::bigint[]) AS t(job_id)
                 ON CONFLICT (job_id) DO UPDATE SET revision = source_revisions.revision + 1",
            )
            .bind(&ids)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn generation_by_request(&mut self, account: AccountId, request_key: Uuid) -> StoreResult<'_, Option<Generation>> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "SELECT ",
                generation_cols!(),
                " FROM transgate_jobs.generations WHERE account_id = $1 AND request_key = $2"
            ))
            .bind(account.0)
            .bind(request_key)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            one_generation(row)
        })
    }

    fn active_metered(&mut self, account: AccountId) -> StoreResult<'_, Option<Generation>> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "SELECT ",
                generation_cols!(),
                " FROM transgate_jobs.generations
                 WHERE account_id = $1 AND quota IS NOT NULL AND status IN ('accepted', 'running')"
            ))
            .bind(account.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            one_generation(row)
        })
    }

    fn active_for_job(&mut self, account: AccountId, job: JobId, kind: GenerationKind) -> StoreResult<'_, Option<Generation>> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "SELECT ",
                generation_cols!(),
                " FROM transgate_jobs.generations
                 WHERE account_id = $1 AND job_id = $2 AND kind = $3 AND status IN ('accepted', 'running')
                 ORDER BY created_at DESC, id LIMIT 1"
            ))
            .bind(account.0)
            .bind(job.0)
            .bind(to_text(&kind)?)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            one_generation(row)
        })
    }

    fn insert_generation<'a>(&'a mut self, g: &'a Generation) -> StoreResult<'a, ()> {
        Box::pin(async move {
            // The (account, request_key) unique key turns a duplicate into StoreError::Conflict.
            sqlx::query(
                "INSERT INTO transgate_jobs.generations
                   (id, account_id, job_id, kind, request_key, request_digest, source_revision, consent_epoch, input,
                    status, sequence, draft, staged, execution_token, lease_until, quota, interruption,
                    remote_outcome_unknown, remote_id, tokens_used, created_at, updated_at, finished_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, transgate_jobs.us_to_ts($15),
                         $16, $17, $18, $19, $20, transgate_jobs.us_to_ts($21), transgate_jobs.us_to_ts($22),
                         transgate_jobs.us_to_ts($23))",
            )
            .bind(g.id.0)
            .bind(g.account.0)
            .bind(g.job.0)
            .bind(to_text(&g.kind)?)
            .bind(g.request_key)
            .bind(&g.request_digest)
            .bind(g.source_revision)
            .bind(g.consent_epoch)
            .bind(Json(&g.input))
            .bind(to_text(&g.status)?)
            .bind(g.sequence)
            .bind(Json(&g.draft))
            .bind(g.staged.as_ref().map(Json))
            .bind(g.execution.map(|e| e.token))
            .bind(g.execution.map(|e| e.lease_until.0))
            .bind(g.quota.as_ref().map(Json))
            .bind(g.interruption.as_ref().map(Json))
            .bind(g.remote_outcome_unknown)
            .bind(&g.remote_id)
            .bind(g.tokens_used)
            .bind(g.created_at.0)
            .bind(g.updated_at.0)
            .bind(g.finished_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn lock_generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", generation_cols!(), " FROM transgate_jobs.generations WHERE id = $1 FOR UPDATE"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            one_generation(row)
        })
    }

    fn save_generation<'a>(&'a mut self, g: &'a Generation) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_jobs.generations SET
                   status = $2, sequence = $3, draft = $4, staged = $5, execution_token = $6,
                   lease_until = transgate_jobs.us_to_ts($7), quota = $8, interruption = $9,
                   remote_outcome_unknown = $10, remote_id = $11, tokens_used = $12,
                   updated_at = transgate_jobs.us_to_ts($13), finished_at = transgate_jobs.us_to_ts($14)
                 WHERE id = $1",
            )
            .bind(g.id.0)
            .bind(to_text(&g.status)?)
            .bind(g.sequence)
            .bind(Json(&g.draft))
            .bind(g.staged.as_ref().map(Json))
            .bind(g.execution.map(|e| e.token))
            .bind(g.execution.map(|e| e.lease_until.0))
            .bind(g.quota.as_ref().map(Json))
            .bind(g.interruption.as_ref().map(Json))
            .bind(g.remote_outcome_unknown)
            .bind(&g.remote_id)
            .bind(g.tokens_used)
            .bind(g.updated_at.0)
            .bind(g.finished_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            if done.rows_affected() == 1 {
                Ok(())
            } else {
                Err(StoreError::Conflict(format!("generation {} does not exist", g.id.0)))
            }
        })
    }

    fn generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>> {
        Box::pin(async move {
            let row = sqlx::query(concat!("SELECT ", generation_cols!(), " FROM transgate_jobs.generations WHERE id = $1"))
                .bind(id.0)
                .fetch_optional(self.conn())
                .await
                .map_err(db)?;
            one_generation(row)
        })
    }

    fn latest_analysis(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<Generation>> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "SELECT ",
                generation_cols!(),
                " FROM transgate_jobs.generations
                 WHERE account_id = $1 AND job_id = $2 AND kind = 'analysis'
                 ORDER BY created_at DESC, id DESC LIMIT 1"
            ))
            .bind(account.0)
            .bind(job.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            one_generation(row)
        })
    }

    fn lock_counter(&mut self, key: CounterKey) -> StoreResult<'_, Option<QuotaCounter>> {
        Box::pin(async move {
            let (scope, job) = scope_columns(key.scope);
            let row = sqlx::query(concat!(
                "SELECT ",
                counter_cols!(),
                " FROM transgate_jobs.quota_counters
                 WHERE account_id = $1 AND scope = $2 AND job_id IS NOT DISTINCT FROM $3 AND kind = $4 AND period = $5
                 FOR UPDATE"
            ))
            .bind(key.account.0)
            .bind(scope)
            .bind(job)
            .bind(to_text(&key.kind)?)
            .bind(to_text(&key.period)?)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            row.as_ref().map(counter_from).transpose()
        })
    }

    fn save_counter<'a>(&'a mut self, counter: &'a QuotaCounter) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let (scope, job) = scope_columns(counter.key.scope);
            sqlx::query(
                "INSERT INTO transgate_jobs.quota_counters (account_id, scope, job_id, kind, period, count, max_limit, reset_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, transgate_jobs.us_to_ts($8))
                 ON CONFLICT ON CONSTRAINT quota_counters_key DO UPDATE SET
                   count = EXCLUDED.count, max_limit = EXCLUDED.max_limit, reset_at = EXCLUDED.reset_at",
            )
            .bind(counter.key.account.0)
            .bind(scope)
            .bind(job)
            .bind(to_text(&counter.key.kind)?)
            .bind(to_text(&counter.key.period)?)
            .bind(counter.count)
            .bind(counter.max_limit)
            .bind(counter.reset_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn counters(&mut self, account: AccountId, job: Option<JobId>) -> StoreResult<'_, Vec<QuotaCounter>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                counter_cols!(),
                " FROM transgate_jobs.quota_counters
                 WHERE account_id = $1 AND (scope = 'user' OR job_id = $2)
                 ORDER BY scope, kind, period"
            ))
            .bind(account.0)
            .bind(job.map(|j| j.0))
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            rows.iter().map(counter_from).collect()
        })
    }

    fn append_chat_pair<'a>(&'a mut self, question: &'a ChatMessage, answer: &'a ChatMessage) -> StoreResult<'a, [ChatMessage; 2]> {
        Box::pin(async move {
            if question.generation != answer.generation || question.job != answer.job || question.account != answer.account {
                return Err(StoreError::Conflict("a chat pair belongs to one generation, job and account".into()));
            }
            // Source get_next_message_order: max + 1 per job, 1 for the first. The
            // caller's account lock serializes pairs of the account's jobs.
            let first: i64 = sqlx::query_scalar(
                "SELECT COALESCE(max(message_order) + 1, 1) FROM transgate_jobs.chat_messages WHERE job_id = $1",
            )
            .bind(question.job.0)
            .fetch_one(self.conn())
            .await
            .map_err(db)?;
            let mut stored = [question.clone(), answer.clone()];
            for (offset, message) in stored.iter_mut().enumerate() {
                message.order = first + offset as i64;
                // The (generation, role) unique key turns a second pair into StoreError::Conflict.
                sqlx::query(
                    "INSERT INTO transgate_jobs.chat_messages
                       (id, account_id, job_id, generation_id, message_order, role, content, consent_epoch, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, transgate_jobs.us_to_ts($9))",
                )
                .bind(message.id)
                .bind(message.account.0)
                .bind(message.job.0)
                .bind(message.generation.0)
                .bind(message.order)
                .bind(to_text(&message.role)?)
                .bind(&message.content)
                .bind(message.consent_epoch)
                .bind(message.created_at.0)
                .execute(self.conn())
                .await
                .map_err(db)?;
            }
            Ok(stored)
        })
    }

    fn chat_pair(&mut self, generation: GenerationId) -> StoreResult<'_, Option<[ChatMessage; 2]>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                chat_cols!(),
                " FROM transgate_jobs.chat_messages WHERE generation_id = $1 ORDER BY message_order"
            ))
            .bind(generation.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            let mut messages = rows.iter().map(chat_from).collect::<Result<Vec<_>, _>>()?;
            match messages.len() {
                0 => Ok(None),
                2 => {
                    let answer = messages.pop().unwrap();
                    let question = messages.pop().unwrap();
                    Ok(Some([question, answer]))
                }
                n => Err(corrupt("chat pair", format!("generation {} has {n} messages", generation.0))),
            }
        })
    }

    fn chat_history(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Vec<ChatMessage>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT ",
                chat_cols!(),
                " FROM transgate_jobs.chat_messages WHERE account_id = $1 AND job_id = $2 ORDER BY message_order"
            ))
            .bind(account.0)
            .bind(job.0)
            .fetch_all(self.conn())
            .await
            .map_err(db)?;
            rows.iter().map(chat_from).collect()
        })
    }

    fn save_completion_summary<'a>(&'a mut self, summary: &'a CompletionSummary) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.completion_summaries
                   (job_id, source_revision, account_id, generation_id, text, consent_epoch, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, transgate_jobs.us_to_ts($7))
                 ON CONFLICT (job_id, source_revision) DO NOTHING",
            )
            .bind(summary.job.0)
            .bind(summary.source_revision)
            .bind(summary.account.0)
            .bind(summary.generation.0)
            .bind(&summary.text)
            .bind(summary.consent_epoch)
            .bind(summary.created_at.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn completion_summary(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<CompletionSummary>> {
        Box::pin(async move {
            let row: Option<(i64, i64, Uuid, Uuid, String, i64, i64)> = sqlx::query_as(
                "SELECT job_id, source_revision, account_id, generation_id, text, COALESCE(consent_epoch, 0),
                        transgate_jobs.ts_to_us(created_at)
                 FROM transgate_jobs.completion_summaries WHERE account_id = $1 AND job_id = $2
                 ORDER BY source_revision DESC LIMIT 1",
            )
            .bind(account.0)
            .bind(job.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            Ok(row.map(|(job, source_revision, account, generation, text, consent_epoch, created)| CompletionSummary {
                account: AccountId(account),
                job: JobId(job),
                source_revision,
                generation: GenerationId(generation),
                text,
                consent_epoch,
                created_at: Micros(created),
            }))
        })
    }

    fn request_summary<'a>(&'a mut self, request: &'a SummaryRequest) -> StoreResult<'a, ()> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO transgate_jobs.summary_requests
                   (job_id, source_revision, account_id, requested_at, generation_id, outcome, decided_at)
                 VALUES ($1, $2, $3, transgate_jobs.us_to_ts($4), $5, $6, transgate_jobs.us_to_ts($7))
                 ON CONFLICT (job_id, source_revision) DO NOTHING",
            )
            .bind(request.job.0)
            .bind(request.source_revision)
            .bind(request.account.0)
            .bind(request.requested_at.0)
            .bind(request.generation.map(|g| g.0))
            .bind(request.outcome.as_ref().map(Json))
            .bind(request.decided_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn lock_summary_request(&mut self, job: JobId, source_revision: i64) -> StoreResult<'_, Option<SummaryRequest>> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "SELECT ",
                summary_request_cols!(),
                " FROM transgate_jobs.summary_requests WHERE job_id = $1 AND source_revision = $2 FOR UPDATE"
            ))
            .bind(job.0)
            .bind(source_revision)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            row.as_ref().map(summary_request_from).transpose()
        })
    }

    fn save_summary_request<'a>(&'a mut self, request: &'a SummaryRequest) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let done = sqlx::query(
                "UPDATE transgate_jobs.summary_requests
                 SET generation_id = $3, outcome = $4, decided_at = transgate_jobs.us_to_ts($5)
                 WHERE job_id = $1 AND source_revision = $2",
            )
            .bind(request.job.0)
            .bind(request.source_revision)
            .bind(request.generation.map(|g| g.0))
            .bind(request.outcome.as_ref().map(Json))
            .bind(request.decided_at.map(|t| t.0))
            .execute(self.conn())
            .await
            .map_err(db)?;
            if done.rows_affected() == 1 {
                Ok(())
            } else {
                Err(StoreError::Conflict(format!(
                    "no summary request for job {} at revision {}",
                    request.job, request.source_revision
                )))
            }
        })
    }

    fn summary_request(&mut self, job: JobId) -> StoreResult<'_, Option<SummaryRequest>> {
        Box::pin(async move {
            let row = sqlx::query(concat!(
                "SELECT ",
                summary_request_cols!(),
                " FROM transgate_jobs.summary_requests WHERE job_id = $1 ORDER BY source_revision DESC LIMIT 1"
            ))
            .bind(job.0)
            .fetch_optional(self.conn())
            .await
            .map_err(db)?;
            row.as_ref().map(summary_request_from).transpose()
        })
    }

    fn record_dispatch<'a>(&'a mut self, d: &'a LlmDispatch) -> StoreResult<'a, ()> {
        Box::pin(async move {
            let inserted = sqlx::query(
                "INSERT INTO transgate_jobs.llm_dispatches
                   (generation_id, account_id, provider, model, disposition, remote_ids, unkept_id_observed, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, transgate_jobs.us_to_ts($8))
                 ON CONFLICT (generation_id) DO NOTHING",
            )
            .bind(d.generation.0)
            .bind(d.account.0)
            .bind(&d.provider)
            .bind(&d.model)
            .bind(to_text(&d.disposition)?)
            .bind(&d.remote_ids)
            .bind(d.unkept_id_observed)
            .bind(d.created_at.0)
            .execute(self.conn())
            .await
            .map_err(db)?;
            if inserted.rows_affected() == 1 {
                return Ok(());
            }
            // The existing row is kept; re-recording it for another owner or provider is a conflict.
            let (account, provider, model): (Uuid, String, String) = sqlx::query_as(
                "SELECT account_id, provider, model FROM transgate_jobs.llm_dispatches WHERE generation_id = $1",
            )
            .bind(d.generation.0)
            .fetch_one(self.conn())
            .await
            .map_err(db)?;
            if account == d.account.0 && provider == d.provider && model == d.model {
                Ok(())
            } else {
                Err(StoreError::Conflict(format!("generation {} already has another dispatch record", d.generation.0)))
            }
        })
    }

    fn settle_dispatch<'a>(
        &'a mut self,
        account: AccountId,
        generation: GenerationId,
        disposition: DispatchDisposition,
        observed: Option<&'a str>,
    ) -> StoreResult<'a, bool> {
        Box::pin(async move {
            // LlmDispatch::settle in one statement: a new, nonempty, bounded id is
            // appended in first-seen order; a new nonempty id that cannot be kept
            // (over 256 bytes, or 8 already stored) sets the durable unkept flag
            // instead, never a truncated id. Any such evidence keeps the row possible:
            // not_dispatched holds only while no id is stored, the flag is false and
            // none was observed in this settle.
            let done = sqlx::query(
                "WITH next AS (
                   SELECT generation_id, remote_ids, unkept_id_observed,
                          ($4::text IS NOT NULL AND $4 <> '' AND NOT ($4 = ANY(remote_ids))) AS new_id,
                          (octet_length($4) <= $5 AND cardinality(remote_ids) < $6) AS keepable
                   FROM transgate_jobs.llm_dispatches
                   WHERE account_id = $1 AND generation_id = $2
                   FOR UPDATE
                 ), settled AS (
                   SELECT generation_id,
                          CASE WHEN new_id AND keepable THEN array_append(remote_ids, $4::text) ELSE remote_ids END AS ids,
                          unkept_id_observed OR (new_id AND NOT keepable) AS unkept
                   FROM next
                 )
                 UPDATE transgate_jobs.llm_dispatches d
                 SET remote_ids = settled.ids,
                     unkept_id_observed = settled.unkept,
                     disposition = CASE WHEN $3 = 'not_dispatched' AND cardinality(settled.ids) = 0 AND NOT settled.unkept
                                             AND ($4::text IS NULL OR $4 = '')
                                        THEN 'not_dispatched' ELSE 'possible' END
                 FROM settled WHERE d.generation_id = settled.generation_id",
            )
            .bind(account.0)
            .bind(generation.0)
            .bind(to_text(&disposition)?)
            .bind(observed)
            .bind(MAX_DISPATCH_REMOTE_ID_BYTES as i32)
            .bind(MAX_DISPATCH_REMOTE_IDS as i32)
            .execute(self.conn())
            .await
            .map_err(db)?;
            Ok(done.rows_affected() == 1)
        })
    }

    fn purge_ai_artifacts(&mut self, account: AccountId, cutoff: i64) -> StoreResult<'_, u64> {
        Box::pin(async move {
            // Legacy rows carry no epoch and are always covered (D3). Summary
            // requests and quota counters are content-free and stay.
            let mut removed = 0;
            for statement in [
                "DELETE FROM transgate_jobs.chat_messages
                 WHERE account_id = $1 AND (consent_epoch IS NULL OR consent_epoch <= $2)",
                "DELETE FROM transgate_jobs.completion_summaries
                 WHERE account_id = $1 AND (consent_epoch IS NULL OR consent_epoch <= $2)",
                "DELETE FROM transgate_jobs.generations
                 WHERE account_id = $1 AND (consent_epoch IS NULL OR consent_epoch <= $2)",
            ] {
                removed += sqlx::query(statement)
                    .bind(account.0)
                    .bind(cutoff)
                    .execute(self.conn())
                    .await
                    .map_err(db)?
                    .rows_affected();
            }
            // P7 D10: a stored callback body may carry the consent-gated summary. Deliveries have
            // no consent epoch, so every stored body of the account is cleared.
            sqlx::query("UPDATE transgate_api.callback_deliveries SET payload = NULL WHERE account_id = $1 AND payload IS NOT NULL")
                .bind(account.0)
                .execute(self.conn())
                .await
                .map_err(db)?;
            Ok(removed)
        })
    }
}
