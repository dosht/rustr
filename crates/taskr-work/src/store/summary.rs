//! Summary operations on the existing account-lock/transaction domain. No provider I/O here.
//! Mutations take the account lock first; unchanged polling reads without any lock and falls back
//! to the locked path only when there is real work to reconcile.

use llm_core::RemoteOutcome;
use sqlx::types::Json;
use uuid::Uuid;

use super::{Store, StoreError, Tx, io};
use crate::summary::domain::{ADMISSION_LEASE_US, Generation, GenerationStatus, RemoteIdLimit, SavedSummary, SummaryView, merge_remote_ids};
use crate::summary::error::SummaryError as Error;

pub(crate) struct Admission {
    pub generation: Generation,
    pub created: bool,
}

struct Source { revision: u64, completed: bool, has_transcript: bool }
impl Source {
    fn permits(&self, g: &Generation) -> bool { self.completed && self.has_transcript && self.revision == g.source_revision }
    /// Exactly when `summary_reconcile` would change `g`: a fenced source, a staged final or an
    /// expired lease. Anything else is an unchanged read that needs no lock.
    fn needs_reconcile(&self, g: &Generation, now: i64) -> bool {
        !g.status.terminal() && (!self.permits(g) || g.staged.is_some() || now >= g.lease_until_us)
    }
}

/// One unlocked fence read: barrier, enrollment, work revision/completed/transcript, generation.
type PollRow = (bool, bool, Option<i64>, Option<bool>, Option<bool>, Option<Json<Generation>>);

/// A truthful interruption reason, not storage advice.
fn limit(refused: RemoteIdLimit) -> Error { Error::Invalid(refused.reason().into()) }

fn remote_ids_of(value: Option<&serde_json::Value>) -> Vec<String> {
    value.and_then(|v| v.as_array()).map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

impl Tx {
    async fn summary_source(&mut self, account: Uuid, work: Uuid) -> Result<Source, Error> {
        self.lock_account(account).await?;
        let row: Option<(i64, bool, bool)> = sqlx::query_as(
            "SELECT revision, status = 'completed', transcript IS NOT NULL FROM taskr_work.works
             WHERE id = $1 AND account_id = $2 FOR UPDATE",
        ).bind(work).bind(account).fetch_optional(&mut *self.inner).await?;
        let (revision, completed, has_transcript) = row.ok_or(Error::NotFound)?;
        Ok(Source { revision: revision as u64, completed, has_transcript })
    }

    async fn summary_generation(&mut self, account: Uuid, work: Uuid, id: Uuid) -> Result<Generation, Error> {
        let row: Option<Json<Generation>> = sqlx::query_scalar(
            "SELECT payload FROM taskr_work.summary_generations WHERE id = $1 AND account_id = $2 AND work_id = $3 FOR UPDATE",
        ).bind(id).bind(account).bind(work).fetch_optional(&mut *self.inner).await?;
        row.map(|g| g.0).ok_or(Error::NotFound)
    }

    async fn summary_request(&mut self, account: Uuid, request: Uuid) -> Result<Option<Generation>, Error> {
        let row: Option<Json<Generation>> = sqlx::query_scalar(
            "SELECT payload FROM taskr_work.summary_generations WHERE account_id = $1 AND request_id = $2 FOR UPDATE",
        ).bind(account).bind(request).fetch_optional(&mut *self.inner).await?;
        Ok(row.map(|g| g.0))
    }

    /// The lease column mirrors the payload so recovery reads the claim-relative lease.
    async fn summary_save(&mut self, g: &Generation) -> Result<(), Error> {
        let changed = sqlx::query("UPDATE taskr_work.summary_generations SET payload = $2, lease_until_us = $4 WHERE id = $1 AND execution_id = $3")
            .bind(g.id).bind(Json(g)).bind(g.execution).bind(g.lease_until_us).execute(&mut *self.inner).await?;
        if changed.rows_affected() != 1 { return Err(Error::Conflict); }
        Ok(())
    }

    async fn summary_save_final(&mut self, g: &mut Generation, now: i64) -> Result<bool, Error> {
        if !g.complete(now) { return Ok(false); }
        let saved = g.saved().ok_or(Error::Conflict)?;
        sqlx::query(
            "INSERT INTO taskr_work.summaries (work_id, generation_id, account_id, payload) VALUES ($1,$2,$3,$4)
             ON CONFLICT (work_id) DO UPDATE SET generation_id = EXCLUDED.generation_id, payload = EXCLUDED.payload",
        ).bind(g.work).bind(g.id).bind(g.account).bind(Json(&saved)).execute(&mut *self.inner).await?;
        self.summary_save(g).await?;
        Ok(true)
    }

    /// Reconcile a possibly unknown stage/final COMMIT locally, never by replaying provider HTTP.
    async fn summary_reconcile(&mut self, g: &mut Generation, source: &Source, now: i64) -> Result<(), Error> {
        if g.status.terminal() { return Ok(()); }
        if !source.permits(g) {
            g.interrupt("The source transcript is no longer available at this revision.".into(), now);
            self.summary_save(g).await?;
        } else if g.staged.is_some() {
            self.summary_save_final(g, now).await?;
        } else if now >= g.lease_until_us {
            g.interrupt("The generation was interrupted; the provider outcome may be unknown. Start a new request explicitly to retry.".into(), now);
            self.summary_save(g).await?;
        }
        Ok(())
    }

    /// Under the account key with the barrier present: the local cascade may already have removed the
    /// generation, so observed identities go to its erasure resource, reopening a finished erasure.
    /// Known identities are never replaced. Returns an identity that could not be retained.
    async fn summary_erased_remote(&mut self, original: &Generation, observed: &[String], outcome: Option<RemoteOutcome>) -> Result<Option<RemoteIdLimit>, Error> {
        let key = original.id.to_string();
        let resource: Option<(i64, Json<serde_json::Value>)> = sqlx::query_as(
            "SELECT id, target FROM taskr_work.erasure_resources WHERE account_id = $1 AND kind = 'llm' AND target->>'generation' = $2 FOR UPDATE",
        ).bind(original.account).bind(&key).fetch_optional(&mut *self.inner).await?;
        // Before the cascade the row still holds what was already recorded; it seeds a new resource.
        let row: Option<Json<Generation>> = sqlx::query_scalar("SELECT payload FROM taskr_work.summary_generations WHERE id = $1 AND account_id = $2 FOR UPDATE")
            .bind(original.id).bind(original.account).fetch_optional(&mut *self.inner).await?;
        let (mut known, provider) = match (&resource, &row) {
            (Some((_, Json(target))), _) => (remote_ids_of(target.get("remote_ids")), target.get("provider").and_then(|p| p.as_str()).map(str::to_owned)),
            (None, Some(Json(g))) => (g.remote_ids.clone(), g.provider.clone()),
            (None, None) => (Vec::new(), original.provider.clone()),
        };
        if outcome == Some(RemoteOutcome::NotDispatched) && known.is_empty() && observed.is_empty() {
            // The transport proved nothing was sent; keep the local manifest from capturing it.
            sqlx::query("DELETE FROM taskr_work.erasure_resources WHERE account_id = $1 AND kind = 'llm' AND target->>'generation' = $2")
                .bind(original.account).bind(&key).execute(&mut *self.inner).await?;
            sqlx::query("UPDATE taskr_work.summary_generations SET payload = jsonb_set(jsonb_set(payload, '{provider}', 'null'::jsonb), '{remote_outcome_unknown}', 'false'::jsonb) WHERE id = $1 AND account_id = $2 AND execution_id = $3")
                .bind(original.id).bind(original.account).bind(original.execution).execute(&mut *self.inner).await?;
            return Ok(None);
        }
        let refused = merge_remote_ids(&mut known, observed).err();
        let target = serde_json::json!({"generation": original.id, "provider": provider, "remote_ids": known});
        match resource {
            Some((id, _)) => {
                sqlx::query("UPDATE taskr_work.erasure_resources SET target = $2 WHERE id = $1").bind(id).bind(Json(&target)).execute(&mut *self.inner).await?;
            }
            None => self.add_resource(original.account, "llm", target, None).await?,
        }
        sqlx::query("UPDATE taskr_work.erasures SET state = 'pending', finished_us = NULL WHERE account_id = $1")
            .bind(original.account).execute(&mut *self.inner).await?;
        Ok(refused)
    }

    /// Called by `save_work` whenever a completed work is saved without its transcript (retention):
    /// lead decision D10 clears non-canonical derived text in the same transaction.
    pub(crate) async fn purge_summary_source_content(&mut self, work: Uuid) -> Result<(), StoreError> {
        let rows: Vec<Json<Generation>> = io(sqlx::query_scalar(
            "SELECT payload FROM taskr_work.summary_generations WHERE work_id = $1 AND payload->>'status' <> 'completed' FOR UPDATE",
        ).bind(work).fetch_all(&mut *self.inner).await)?;
        for Json(mut g) in rows {
            if g.purge_source_content() {
                io(sqlx::query("UPDATE taskr_work.summary_generations SET payload = $2 WHERE id = $1")
                    .bind(g.id).bind(Json(&g)).execute(&mut *self.inner).await)?;
            }
        }
        Ok(())
    }
}

impl Store {
    /// Auth happened at the route; this resolves ownership, the barrier and an existing request
    /// identity, so a replay is answered before any new-admission disclosure/configuration check.
    pub(crate) async fn replay_summary(&self, account: Uuid, work: Uuid, request: Uuid, now: i64) -> Result<Option<Generation>, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(account, work).await?;
        let Some(mut g) = tx.summary_request(account, request).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        if g.work != work { return Err(Error::Conflict); }
        tx.summary_reconcile(&mut g, &source, now).await?;
        tx.commit().await?;
        Ok(Some(g))
    }

    pub(crate) async fn admit_summary(&self, account: Uuid, work: Uuid, request: Uuid, now: i64) -> Result<Admission, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(account, work).await?;
        if let Some(mut g) = tx.summary_request(account, request).await? {
            if g.work != work { return Err(Error::Conflict); }
            tx.summary_reconcile(&mut g, &source, now).await?;
            tx.commit().await?;
            return Ok(Admission { generation: g, created: false });
        }
        if !source.completed || !source.has_transcript { return Err(Error::NotReady); }
        let active: Option<Json<Generation>> = sqlx::query_scalar(
            "SELECT payload FROM taskr_work.summary_generations WHERE work_id = $1 AND payload->>'status' IN ('accepted','running') FOR UPDATE",
        ).bind(work).fetch_optional(&mut *tx.inner).await?;
        if let Some(Json(mut active)) = active {
            tx.summary_reconcile(&mut active, &source, now).await?;
            if !active.status.terminal() { return Err(Error::Conflict); }
        }
        let g = Generation::new(account, work, request, source.revision, now, now + ADMISSION_LEASE_US);
        sqlx::query(
            "INSERT INTO taskr_work.summary_generations (id,account_id,work_id,request_id,execution_id,source_revision,payload,created_us,lease_until_us)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        ).bind(g.id).bind(account).bind(work).bind(request).bind(g.execution).bind(g.source_revision as i64)
            .bind(Json(&g)).bind(g.created_us).bind(g.lease_until_us).execute(&mut *tx.inner).await?;
        tx.commit().await?;
        Ok(Admission { generation: g, created: true })
    }

    /// Persist dispatch ownership before HTTP; only one replica can claim an accepted generation,
    /// and the claim starts the execution lease.
    pub(crate) async fn claim_summary(&self, account: Uuid, work: Uuid, id: Uuid, now: i64, input_limit: usize, provider: &str) -> Result<Option<(Generation, String)>, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(account, work).await?;
        let mut g = tx.summary_generation(account, work, id).await?;
        tx.summary_reconcile(&mut g, &source, now).await?;
        if g.status != GenerationStatus::Accepted { tx.commit().await?; return Ok(None); }
        let bytes: i64 = sqlx::query_scalar("SELECT octet_length(transcript)::bigint FROM taskr_work.works WHERE id = $1")
            .bind(work).fetch_one(&mut *tx.inner).await?;
        if bytes > input_limit as i64 {
            g.interrupt("The transcript exceeds the completion input byte limit.".into(), now);
            tx.summary_save(&g).await?;
            tx.commit().await?;
            return Err(Error::Invalid("The transcript exceeds the completion input byte limit.".into()));
        }
        let text: String = sqlx::query_scalar("SELECT transcript FROM taskr_work.works WHERE id = $1")
            .bind(work).fetch_one(&mut *tx.inner).await?;
        if !g.claim(now) { tx.commit().await?; return Ok(None); }
        g.provider = Some(provider.to_string());
        tx.summary_save(&g).await?;
        tx.commit().await?;
        Ok(Some((g, text)))
    }

    /// The fence read for watch/driver polling: barrier, enrollment, ownership and source facts are
    /// read fresh in one statement, without the account lock or row locks, so an unchanged poll
    /// never waits behind (or delays) account work. Only real reconciliation takes the locks.
    pub(crate) async fn poll_summary_generation(&self, account: Uuid, work: Uuid, id: Uuid, now: i64) -> Result<Generation, Error> {
        let row: PollRow = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM taskr_work.erasures WHERE account_id = $1),
                    EXISTS (SELECT 1 FROM taskr_work.enrollments WHERE account_id = $1),
                    w.revision, w.status = 'completed', w.transcript IS NOT NULL, g.payload
             FROM (SELECT 1) AS one
             LEFT JOIN taskr_work.works w ON w.id = $2 AND w.account_id = $1
             LEFT JOIN taskr_work.summary_generations g ON g.id = $3 AND g.account_id = $1 AND g.work_id = $2",
        ).bind(account).bind(work).bind(id).fetch_one(self.pool()).await?;
        let (erased, enrolled, revision, completed, has_transcript, payload) = row;
        if erased { return Err(Error::Erased); }
        if !enrolled { return Err(Error::NotEnrolled); }
        let (Some(revision), Some(completed), Some(has_transcript), Some(Json(g))) = (revision, completed, has_transcript, payload) else {
            return Err(Error::NotFound);
        };
        let source = Source { revision: revision as u64, completed, has_transcript };
        if source.needs_reconcile(&g, now) {
            return self.read_summary_generation(account, work, id, now).await;
        }
        Ok(g)
    }

    pub(crate) async fn read_summary_generation(&self, account: Uuid, work: Uuid, id: Uuid, now: i64) -> Result<Generation, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(account, work).await?;
        let mut g = tx.summary_generation(account, work, id).await?;
        tx.summary_reconcile(&mut g, &source, now).await?;
        tx.commit().await?;
        Ok(g)
    }

    pub(crate) async fn read_summary(&self, account: Uuid, work: Uuid, now: i64) -> Result<SummaryView, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(account, work).await?;
        let latest: Option<Json<Generation>> = sqlx::query_scalar(
            "SELECT payload FROM taskr_work.summary_generations WHERE work_id = $1 ORDER BY created_us DESC,id DESC LIMIT 1 FOR UPDATE",
        ).bind(work).fetch_optional(&mut *tx.inner).await?;
        let generation = if let Some(Json(mut g)) = latest {
            tx.summary_reconcile(&mut g, &source, now).await?;
            Some(g.snapshot())
        } else { None };
        let saved: Option<Json<SavedSummary>> = sqlx::query_scalar("SELECT payload FROM taskr_work.summaries WHERE work_id = $1")
            .bind(work).fetch_optional(&mut *tx.inner).await?;
        tx.commit().await?;
        Ok(SummaryView { summary: saved.map(|s| s.0), generation })
    }

    /// Account -> work -> generation; CAS execution/revision/deadline at every content write. The
    /// claimed generation names the account, work, generation and execution being fenced.
    pub(crate) async fn checkpoint_summary(&self, claimed: &Generation, text: String, staged: bool, now: i64) -> Result<bool, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(claimed.account, claimed.work).await?;
        let mut g = tx.summary_generation(claimed.account, claimed.work, claimed.id).await?;
        if g.execution != claimed.execution || g.status != GenerationStatus::Running { tx.commit().await?; return Ok(false); }
        if !source.permits(&g) || now >= g.lease_until_us {
            g.interrupt("The generation's source or execution lease is no longer current.".into(), now);
            tx.summary_save(&g).await?;
            tx.commit().await?;
            return Ok(false);
        }
        let changed = if staged { g.stage(text, now) } else { g.checkpoint(text, now) }
            .map_err(|_| Error::Invalid("The completion output exceeds its byte limit.".into()))?;
        if changed { tx.summary_save(&g).await?; }
        tx.commit().await?;
        Ok(changed)
    }

    pub(crate) async fn settle_summary(&self, account: Uuid, work: Uuid, id: Uuid, execution: Uuid, now: i64) -> Result<bool, Error> {
        let mut tx = self.begin().await?;
        let source = tx.summary_source(account, work).await?;
        let mut g = tx.summary_generation(account, work, id).await?;
        if g.execution != execution { return Err(Error::Conflict); }
        tx.summary_reconcile(&mut g, &source, now).await?;
        let completed = g.status == GenerationStatus::Completed;
        tx.commit().await?;
        Ok(completed)
    }

    /// Lead decision D10: cancel does not reconcile first. A staged final that was never settled is
    /// not canonical and the cancel wins; a completed generation is returned unchanged.
    pub(crate) async fn cancel_summary(&self, account: Uuid, work: Uuid, id: Uuid, now: i64) -> Result<Generation, Error> {
        let mut tx = self.begin().await?;
        tx.summary_source(account, work).await?;
        let mut g = tx.summary_generation(account, work, id).await?;
        if g.cancel(now) { tx.summary_save(&g).await?; }
        tx.commit().await?;
        Ok(g)
    }

    /// Interrupts `original`'s execution and carries identities the driver observed in memory into
    /// the same transaction, best effort: if this transaction or the process fails they are lost,
    /// and nothing claims otherwise. Under the barrier they go to the erasure resource instead.
    pub(crate) async fn interrupt_summary(&self, original: &Generation, why: String, observed: &[String], now: i64) -> Result<(), Error> {
        let mut tx = self.begin().await?;
        tx.lock_account_key(original.account).await?;
        if tx.barrier(original.account).await? {
            if !observed.is_empty() { tx.summary_erased_remote(original, observed, None).await?; }
            tx.commit().await?;
            return Ok(());
        }
        let source = tx.summary_source(original.account, original.work).await?;
        let mut g = tx.summary_generation(original.account, original.work, original.id).await?;
        // First reconcile any staged/final COMMIT whose response may have been lost. Never discard a
        // durable successful final merely because the worker saw a database/transport error.
        tx.summary_reconcile(&mut g, &source, now).await?;
        if g.execution == original.execution {
            let known = g.remote_ids.len();
            // Identities that do not fit are named by the interruption reason itself.
            let _ = merge_remote_ids(&mut g.remote_ids, observed);
            let interrupted = g.interrupt(why, now);
            if interrupted || g.remote_ids.len() != known {
                g.updated_us = now;
                tx.summary_save(&g).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Remote metadata may arrive after the local cascade. It can update only minimal cleanup
    /// facts, never content or a deleted generation. All writers share the account erasure lock.
    /// Distinct identities accumulate; an identity that does not fit is an explicit `Invalid` after
    /// the ones that fit are committed. Returns whether the execution is still running.
    pub(crate) async fn record_summary_remote(&self, original: &Generation, remote: Option<&str>, outcome: Option<RemoteOutcome>, now: i64) -> Result<bool, Error> {
        let observed: Vec<String> = remote.map(|id| vec![id.to_string()]).unwrap_or_default();
        let mut tx = self.begin().await?;
        tx.lock_account_key(original.account).await?;
        if tx.barrier(original.account).await? {
            let refused = tx.summary_erased_remote(original, &observed, outcome).await?;
            tx.commit().await?;
            return match refused { Some(refused) => Err(limit(refused)), None => Ok(false) };
        }
        let row: Option<Json<Generation>> = sqlx::query_scalar("SELECT payload FROM taskr_work.summary_generations WHERE id = $1 AND account_id = $2 FOR UPDATE")
            .bind(original.id).bind(original.account).fetch_optional(&mut *tx.inner).await?;
        let Some(Json(mut current)) = row else { tx.commit().await?; return Ok(false) };
        if current.execution != original.execution { tx.commit().await?; return Ok(false); }
        let refused = merge_remote_ids(&mut current.remote_ids, &observed).err();
        match outcome {
            // Only a request with no observed identity can be proved never sent.
            Some(RemoteOutcome::NotDispatched) if current.remote_ids.is_empty() => {
                current.provider = None;
                current.remote_outcome_unknown = false;
            }
            Some(RemoteOutcome::NotDispatched) | None => {}
            Some(seen) => current.remote_outcome_unknown = seen == RemoteOutcome::Unknown,
        }
        current.updated_us = now;
        tx.summary_save(&current).await?;
        let running = current.status == GenerationStatus::Running;
        tx.commit().await?;
        match refused { Some(refused) => Err(limit(refused)), None => Ok(running) }
    }
}
