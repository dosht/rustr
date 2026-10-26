//! Retention sweep and account-erasure use cases.

use std::sync::Arc;

use billing_core::erasure::{ErasureError, SubscriptionTermination, erase_account_billing, terminate_subscriptions};
use billing_core::ports::{BillingStore, StoreError};
use billing_core::types::{Clock, SubjectRef};
use media_ports::{DeleteOutcome, ObjectInventory, ObjectStore, Transcriber};

use crate::ids::{AccountId, ErasureId, Micros};
use crate::jobs::app::JobsError;
use crate::jobs::domain::Provider;
use crate::jobs::ports::{IdentityAdmin, IdentityOutcome, JobStore, JobUow, Locks};
use crate::privacy::erasure::{
    self, ErasureItem, ErasureRecord, ErasureState, ErasureView, ExceptionPolicy, ItemState, ItemTarget,
    NO_PROVIDER_DELETE_API,
};
use crate::privacy::retention::{
    self, HISTORY_LOCK, HistoryOutcome, INTERRUPTED_MESSAGE, RetentionDays, RetentionOutcome, RetentionPlan,
    RetentionRun, SAFETY_THRESHOLD, TRANSCRIPT_LOCK,
};
use crate::schedule::Schedule;

#[derive(Clone)]
pub struct PrivacyServices {
    pub store: Arc<dyn JobStore>,
    pub clock: Arc<dyn Clock>,
    pub locks: Arc<dyn Locks>,
    pub objects: Vec<Arc<dyn ObjectStore>>,
    /// Prefix inventories of legacy buckets (D2/F9); none configured leaves the item unresolved.
    pub inventories: Vec<Arc<dyn ObjectInventory>>,
    pub transcribers: Vec<Arc<dyn Transcriber>>,
    pub identity: Arc<dyn IdentityAdmin>,
    /// Billing's own store, for its erasure step 2 (D10), which opens its own transactions.
    pub billing: Arc<dyn BillingStore>,
    /// Erasure-only subscription termination per gateway and environment (D10).
    pub terminators: Vec<Arc<dyn SubscriptionTermination>>,
    pub exceptions: ExceptionPolicy,
}

/// An operator's explicit exception approval (C-11, D15). `operator` is the
/// verified identity of an administrator: the host takes it from its
/// authenticated session after its own admin check, never from the request
/// body. `rationale` records why the residual risk is accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperatorApproval {
    pub operator: String,
    pub rationale: String,
}

pub struct Privacy {
    s: PrivacyServices,
}

async fn finish<T>(uow: Box<dyn JobUow>, result: Result<T, JobsError>) -> Result<T, JobsError> {
    match result {
        Ok(v) => {
            uow.commit().await?;
            Ok(v)
        }
        Err(e) => {
            let _ = uow.rollback().await;
            Err(e)
        }
    }
}

impl Privacy {
    pub fn new(services: PrivacyServices) -> Privacy {
        Privacy { s: services }
    }

    fn now(&self) -> Micros {
        self.s.clock.now()
    }

    // ---- retention -------------------------------------------------------------

    /// PRV-023: the dry run. Same predicate and cutoff, nothing deleted, no audit row.
    pub async fn retention_plan(&self, days: RetentionDays) -> Result<RetentionPlan, JobsError> {
        let now = self.now();
        let cutoff = days.cutoff(now);
        let mut uow = self.s.store.begin().await?;
        let read = async {
            let candidates = uow.retention_candidates(cutoff).await?;
            let opted_out = uow.opted_out_count(cutoff).await?;
            Ok::<_, StoreError>((candidates, opted_out))
        }
        .await;
        let _ = uow.rollback().await;
        let (candidates, opted_out) = read?;
        Ok(retention::plan(days, now, &candidates, opted_out))
    }

    /// PRV-014..022: one run. A concurrent run writes `skipped`; more than the
    /// threshold writes `aborted` and deletes nothing; success commits the
    /// deletion and its audit row together; a failure is recorded after rollback.
    pub async fn retention_run(&self, days: RetentionDays, executed_by: &str) -> Result<RetentionOutcome, JobsError> {
        let started = self.now();
        let run = RetentionRun::started(days, started, executed_by);
        let Some(lock) = self.s.locks.try_lock(TRANSCRIPT_LOCK).await? else {
            self.record_run(&run.skipped(), true).await?;
            return Ok(RetentionOutcome::Skipped);
        };
        let outcome = self.retention_locked(run.clone(), days, started).await;
        let outcome = self.record_failure(&run, started, outcome).await;
        lock.release().await;
        outcome
    }

    async fn record_run(&self, run: &RetentionRun, insert: bool) -> Result<(), JobsError> {
        let mut uow = self.s.store.begin().await?;
        let r = if insert { uow.insert_retention_run(run).await } else { uow.save_retention_run(run).await };
        finish(uow, r.map_err(JobsError::from)).await
    }

    /// Under the job's lock, before any work: earlier `running` rows of the same job
    /// belong to runs that stopped before finishing, so they become honest failures
    /// (their deletion never committed); then this run's own row exists, so a crash
    /// now leaves it to the next run's recovery.
    async fn begin_locked_run(&self, run: &RetentionRun) -> Result<(), JobsError> {
        let mut uow = self.s.store.begin().await?;
        let r = async {
            uow.fail_interrupted_runs(&run.job_name, INTERRUPTED_MESSAGE, run.executed_at).await?;
            uow.insert_retention_run(run).await
        }
        .await;
        finish(uow, r.map_err(JobsError::from)).await
    }

    /// A failed locked run is recorded as `failure` after its rollback.
    async fn record_failure<T>(&self, run: &RetentionRun, started: Micros, outcome: Result<T, JobsError>) -> Result<T, JobsError> {
        if let Err(e) = &outcome {
            let elapsed = elapsed_ms(started, self.now());
            let _ = self.record_run(&run.clone().failed(&e.message(), elapsed), false).await;
        }
        outcome
    }

    async fn retention_locked(&self, run: RetentionRun, days: RetentionDays, started: Micros) -> Result<RetentionOutcome, JobsError> {
        self.begin_locked_run(&run).await?;
        let cutoff = days.cutoff(started);
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let mut candidates = uow.retention_candidates(cutoff).await?;
            // Account locks in ascending order, the order every job write uses. An
            // account under erasure is skipped: its erasure removes the lines, and
            // billing refuses its lock once the erasure opened.
            let mut accounts: Vec<AccountId> = candidates.iter().map(|c| c.account).collect();
            accounts.sort();
            accounts.dedup();
            let mut erasing: Vec<AccountId> = Vec::new();
            for account in &accounts {
                if uow.lock_barrier(*account).await?.is_some() {
                    erasing.push(*account);
                }
            }
            {
                let mut billing = uow.billing().await?;
                for account in accounts.iter().filter(|a| !erasing.contains(a)) {
                    match billing.lock_account(*account).await {
                        Ok(()) | Err(StoreError::AccountMissing(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            candidates.retain(|c| !erasing.contains(&c.account));
            let locked = uow.lock_retention_set(&candidates, cutoff).await?;
            let opted_out = uow.opted_out_count(cutoff).await?;
            let plan = retention::plan(days, started, &locked, opted_out);
            if plan.would_exceed_threshold {
                return Ok(Err((plan.estimated_deletions, opted_out)));
            }
            let deleted = uow.delete_lines_of(&plan.jobs).await?;
            // Line-only retention: completed AI artifacts stay, active AI work on these jobs is fenced.
            uow.ai().bump_source_revisions(&plan.jobs).await?;
            let finished = run.clone().succeeded(deleted, Some(opted_out), elapsed_ms(started, self.now()));
            uow.save_retention_run(&finished).await?;
            Ok::<_, JobsError>(Ok((deleted, opted_out)))
        }
        .await;
        match result {
            Ok(Ok((rows_deleted, users_opted_out))) => {
                uow.commit().await?;
                Ok(RetentionOutcome::Success { rows_deleted, users_opted_out })
            }
            Ok(Err((would_delete, opted_out))) => {
                let _ = uow.rollback().await;
                self.record_run(&run.aborted(would_delete, Some(opted_out)), false).await?;
                Ok(RetentionOutcome::Aborted { would_delete })
            }
            Err(e) => {
                let _ = uow.rollback().await;
                Err(e)
            }
        }
    }

    /// PRV-025/031, CRN-007: the scheduler's own finished run history older than
    /// `days` (+1 day) is removed, with the transcript run's checks: one run at a
    /// time under `cleanup_cron_job_history` (`skipped` otherwise), more than
    /// 100000 rows aborts with nothing deleted, deletion and its `success` audit
    /// row commit together, a failure is recorded after rollback. Pending and
    /// running claims are never history. The scheduled call passes
    /// `retention::HISTORY_RETENTION_DAYS` (90).
    pub async fn history_retention_run(&self, days: RetentionDays, executed_by: &str) -> Result<HistoryOutcome, JobsError> {
        let started = self.now();
        let run = RetentionRun::history_started(days, started, executed_by);
        let Some(lock) = self.s.locks.try_lock(HISTORY_LOCK).await? else {
            self.record_run(&run.skipped(), true).await?;
            return Ok(HistoryOutcome::Skipped);
        };
        let outcome = self.history_locked(run.clone(), started).await;
        let outcome = self.record_failure(&run, started, outcome).await;
        lock.release().await;
        outcome
    }

    async fn history_locked(&self, run: RetentionRun, started: Micros) -> Result<HistoryOutcome, JobsError> {
        self.begin_locked_run(&run).await?;
        let schedules: Vec<&str> = Schedule::ALL.iter().map(|s| s.name()).collect();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let would_delete = uow.history_count(&schedules, run.cutoff).await?;
            if would_delete > SAFETY_THRESHOLD {
                return Ok(Err(would_delete));
            }
            let deleted = uow.delete_history(&schedules, run.cutoff).await?;
            let finished = run.clone().succeeded(deleted, None, elapsed_ms(started, self.now()));
            uow.save_retention_run(&finished).await?;
            Ok::<_, JobsError>(Ok(deleted))
        }
        .await;
        match result {
            Ok(Ok(rows_deleted)) => {
                uow.commit().await?;
                Ok(HistoryOutcome::Success { rows_deleted })
            }
            Ok(Err(would_delete)) => {
                let _ = uow.rollback().await;
                self.record_run(&run.aborted(would_delete, None), false).await?;
                Ok(HistoryOutcome::Aborted { would_delete })
            }
            Err(e) => {
                let _ = uow.rollback().await;
                Err(e)
            }
        }
    }

    // ---- account erasure ---------------------------------------------------------

    /// PRV-001..010 intended (PRV-D01/D02): the authenticated owner requests
    /// erasure. One transaction writes the host barrier and opens billing's
    /// erasure (billing step 1): from then on every entry point refuses the
    /// account and ordinary billing cannot change it. The saga advances at once
    /// and in `erasure_pass`. A repeated request returns the existing state.
    pub async fn request_erasure(&self, account: AccountId) -> Result<ErasureView, JobsError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            if let Some(existing) = uow.lock_barrier(account).await? {
                return Ok::<_, JobsError>(existing.id);
            }
            uow.billing().await?.open_erasure(account, now).await?;
            let record = ErasureRecord {
                id: ErasureId::new(),
                account,
                requested_at: now,
                state: ErasureState::Requested,
                local_erased_at: None,
                identity_deleted_at: None,
                closed_at: None,
            };
            uow.insert_erasure(&record).await?;
            Ok(record.id)
        }
        .await;
        let id = match finish(uow, result).await {
            // A concurrent request wrote the barrier first (unique per account): its state is the answer.
            Err(JobsError::Store(StoreError::Conflict(_))) => return self.erasure_status(account).await,
            other => other?,
        };
        // Whatever cannot finish now stays runnable for the pass.
        let _ = self.erasure_step(id).await;
        self.erasure_status(account).await
    }

    /// What the owner (or the sign-in shell) sees; `NotFound` without a request.
    pub async fn erasure_status(&self, account: AccountId) -> Result<ErasureView, JobsError> {
        let mut uow = self.s.store.begin().await?;
        let read = async {
            let Some(record) = uow.lock_barrier(account).await? else { return Ok(None) };
            let items = uow.erasure_items(record.id).await?;
            let billing = if record.state == ErasureState::Requested {
                uow.billing().await?.erasure_status(account).await?
            } else {
                None
            };
            Ok::<_, StoreError>(Some(erasure::view(&record, &items, billing.as_ref())))
        }
        .await;
        let _ = uow.rollback().await;
        read?.ok_or(JobsError::NotFound)
    }

    /// Erasures with runnable work (scheduler pass).
    pub async fn erasure_pass(&self) -> Result<u32, JobsError> {
        let mut n = 0;
        for id in self.s.store.erasures_due(self.now(), 100).await? {
            if self.erasure_step(id).await.is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    async fn load_erasure(&self, id: ErasureId) -> Result<(ErasureRecord, Vec<ErasureItem>), JobsError> {
        let mut uow = self.s.store.begin().await?;
        let read = async {
            let record = uow.lock_erasure(id).await?;
            let items = uow.erasure_items(id).await?;
            Ok::<_, StoreError>((record, items))
        }
        .await;
        let _ = uow.rollback().await;
        let (record, items) = read?;
        Ok((record.ok_or(JobsError::NotFound)?, items))
    }

    /// Advances the saga: stop subscriptions and erase locally, then the remote
    /// items (legacy storage first, then identity, then the rest), closing only
    /// under the C-11 rule.
    pub async fn erasure_step(&self, id: ErasureId) -> Result<ErasureState, JobsError> {
        let (record, _) = self.load_erasure(id).await?;
        let state = match record.state {
            ErasureState::Requested => self.erase_locally(&record).await?,
            other => other,
        };
        if state != ErasureState::LocalErased {
            return Ok(state);
        }
        self.run_remote(id).await
    }

    /// Billing step 2 (D10, network outside any transaction), then one
    /// transaction: the manifest, billing step 3 first (D9, D5), the host cascade.
    async fn erase_locally(&self, record: &ErasureRecord) -> Result<ErasureState, JobsError> {
        let account = record.account;
        let now = self.now();
        let terminators: Vec<&dyn SubscriptionTermination> = self.s.terminators.iter().map(|t| t.as_ref()).collect();
        let status = terminate_subscriptions(self.s.billing.as_ref(), &terminators, account, now).await?;
        if !status.subscriptions_stopped() {
            return Ok(ErasureState::Requested);
        }
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut record) = uow.lock_erasure(record.id).await? else { return Err(JobsError::NotFound) };
            if record.state != ErasureState::Requested {
                return Ok(record.state);
            }
            let resources = uow.account_resources(account).await?;
            let subjects: Vec<SubjectRef> = resources.jobs.iter().map(|j| j.subject()).collect();
            {
                let mut billing = uow.billing().await?;
                match erase_account_billing(&mut *billing, account, &subjects, now).await {
                    Ok(_) => {}
                    Err(ErasureError::Store(e)) => return Err(e.into()),
                    // A subscription renewed or the owners moved since the check: retried by the pass.
                    Err(other) => return Err(JobsError::WrongState(other.to_string())),
                }
            }
            uow.erase_account_rows(account).await?;
            uow.insert_erasure_items(&erasure::build_items(record.id, &resources, now)).await?;
            record.state = ErasureState::LocalErased;
            record.local_erased_at = Some(now);
            uow.save_erasure(&record).await?;
            Ok(record.state)
        }
        .await;
        finish(uow, result).await
    }

    async fn run_remote(&self, id: ErasureId) -> Result<ErasureState, JobsError> {
        let (record, items) = self.load_erasure(id).await?;
        if record.state != ErasureState::LocalErased {
            return Ok(record.state);
        }
        let now = self.now();
        let mut updated: Vec<ErasureItem> = Vec::new();
        for item in erasure::runnable(&items, now) {
            let mut item = item.clone();
            item.attempts += 1;
            item.state = self.run_item(&item, record.account, now).await;
            updated.push(item);
        }
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut record) = uow.lock_erasure(id).await? else { return Err(JobsError::NotFound) };
            // Items ran outside this transaction. One whose stored attempts moved since
            // the load (an archive acknowledgement settled it meanwhile) keeps the newer
            // state; this sweep's outcome is stale and is not written back.
            let stored = uow.erasure_items(id).await?;
            for item in &updated {
                let unchanged = stored.iter().any(|s| s.id == item.id && s.attempts == item.attempts - 1);
                if !unchanged {
                    continue;
                }
                uow.save_erasure_item(item).await?;
                if matches!(item.target, ItemTarget::Identity) && item.state.is_done() && record.identity_deleted_at.is_none() {
                    record.identity_deleted_at = Some(now);
                }
            }
            let all = uow.erasure_items(id).await?;
            if let Some(state) = erasure::closure(&all) {
                record.state = state;
                record.closed_at = Some(now);
            }
            uow.save_erasure(&record).await?;
            Ok::<_, JobsError>(record.state)
        }
        .await;
        finish(uow, result).await
    }

    /// An operator records an approved exception (C-11): unsupported or
    /// retained-by-policy items, and (D15) an unresolved write whose end cannot
    /// be observed (an upload URL, an unacknowledged archive PUT). Other
    /// pending work, legacy inventory included, cannot be closed this way, and
    /// `ExceptionPolicy` never approves the latter. Accepting residual
    /// in-flight-write risk needs a nonempty rationale; the result is
    /// `ErasedWithExceptions`, never `FullyErased`.
    pub async fn approve_exception(&self, id: ErasureId, item: uuid::Uuid, approval: &OperatorApproval) -> Result<(), JobsError> {
        let operator = approval.operator.trim();
        if operator.is_empty() {
            return Err(JobsError::InvalidRequest("an approval needs the approving operator".into()));
        }
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut record) = uow.lock_erasure(id).await? else { return Err(JobsError::NotFound) };
            let mut items = uow.erasure_items(id).await?;
            let Some(target) = items.iter_mut().find(|i| i.id == item) else { return Err(JobsError::NotFound) };
            // D15/D9: writes or provider retention whose end cannot be observed; operator only.
            let unobservable_write = matches!(
                target.target,
                ItemTarget::UploadResweep { .. } | ItemTarget::ArchiveInFlight { .. } | ItemTarget::LlmDispatch { .. }
            ) && matches!(target.state, ItemState::Unresolved { .. });
            match target.state {
                ItemState::Unsupported { .. } | ItemState::BlockedUntil { .. } => {}
                _ if unobservable_write => {
                    if approval.rationale.trim().is_empty() {
                        return Err(JobsError::InvalidRequest("accepting an unobservable write needs a rationale".into()));
                    }
                }
                _ => return Err(JobsError::WrongState("only unsupported, retained or unobservable-write items can be approved".into())),
            }
            target.state = ItemState::ApprovedException { reason: approval.rationale.clone(), approved_by: operator.into(), at: now };
            uow.save_erasure_item(target).await?;
            if let Some(state) = erasure::closure(&items) {
                record.state = state;
                record.closed_at = Some(now);
                uow.save_erasure(&record).await?;
            }
            Ok(())
        }
        .await;
        finish(uow, result).await
    }

    /// The adapter bound to exactly this object's store and bucket (adapters refuse others).
    fn store_for(&self, object: &media_ports::ObjectRef) -> Option<&Arc<dyn ObjectStore>> {
        self.s.objects.iter().find(|o| o.kind() == object.store && o.bucket() == object.bucket)
    }

    fn transcriber(&self, provider: Provider) -> Option<&Arc<dyn Transcriber>> {
        let transport = provider.transport()?;
        self.s.transcribers.iter().find(|t| t.provider() == transport)
    }

    fn unsupported(&self, reason: &str, now: Micros) -> ItemState {
        if self.s.exceptions.approves(reason) {
            ItemState::ApprovedException { reason: reason.into(), approved_by: "policy".into(), at: now }
        } else {
            ItemState::Unsupported { reason: reason.into() }
        }
    }

    fn retry(&self, item: &ErasureItem, error: String, now: Micros) -> ItemState {
        ItemState::Retrying { attempts: item.attempts, last_error: error, next_at: erasure::next_retry(item.attempts, now) }
    }

    fn deletion(&self, item: &ErasureItem, outcome: Result<DeleteOutcome, media_ports::TransportError>, now: Micros) -> ItemState {
        match outcome {
            Ok(DeleteOutcome::Deleted) => ItemState::Confirmed { at: now },
            Ok(DeleteOutcome::Absent) => ItemState::Absent { at: now },
            Ok(DeleteOutcome::Retained { until, reason }) => {
                ItemState::BlockedUntil { until: until.map(crate::ids::from_media), reason }
            }
            Ok(DeleteOutcome::Unsupported { reason }) => self.unsupported(&reason, now),
            Err(e) => self.retry(item, e.to_string(), now),
        }
    }

    async fn run_item(&self, item: &ErasureItem, account: AccountId, now: Micros) -> ItemState {
        match &item.target {
            ItemTarget::LegacyStorageObject { object } | ItemTarget::StorageObject { object } => match self.store_for(object) {
                // D2: an unconfigured target stays an explicit unresolved item.
                None => ItemState::Unresolved {
                    reason: format!("object store {:?} is not configured", object.store),
                    next_at: erasure::next_retry(item.attempts, now),
                },
                Some(store) => self.deletion(item, store.delete(object).await, now),
            },
            // F1/F2: a write we cannot see the end of. A miss proves nothing; only
            // an object found and deleted resolves our own archive PUT, and an
            // upload sweep never closes by itself (`erasure::after_sweep`).
            ItemTarget::UploadResweep { object, .. } | ItemTarget::ArchiveInFlight { object } => match self.store_for(object) {
                None => ItemState::Unresolved {
                    reason: format!("object store {:?} is not configured", object.store),
                    next_at: erasure::next_retry(item.attempts, now),
                },
                Some(store) => match store.head(object).await {
                    Ok(None) => erasure::after_sweep(&item.target, false, item.attempts, now),
                    Ok(Some(_)) => match store.delete(object).await {
                        Ok(DeleteOutcome::Deleted | DeleteOutcome::Absent) => erasure::after_sweep(&item.target, true, item.attempts, now),
                        other => self.deletion(item, other, now),
                    },
                    Err(e) => self.retry(item, e.to_string(), now),
                },
            },
            // D2/F9: the legacy folder of the account. Complete only once an
            // authenticated listing comes back empty; every listed object is
            // deleted, and a failed or partial listing never counts as empty.
            ItemTarget::LegacyAccountInventory { bucket } => {
                let inventory = self.s.inventories.iter().find(|i| i.kind() == media_ports::StoreKind::Supabase && i.bucket() == bucket);
                let Some(inventory) = inventory else {
                    return ItemState::Unresolved {
                        reason: "legacy storage inventory is not configured".into(),
                        next_at: erasure::next_retry(item.attempts, now),
                    };
                };
                match inventory.inventory(&account.0.to_string()).await {
                    Err(e) => self.retry(item, e.to_string(), now),
                    Ok(objects) if objects.is_empty() => ItemState::Absent { at: now },
                    Ok(objects) => {
                        let mut failed = 0usize;
                        for object in &objects {
                            let deleted = match self.store_for(object) {
                                Some(store) => matches!(store.delete(object).await, Ok(DeleteOutcome::Deleted | DeleteOutcome::Absent)),
                                None => false,
                            };
                            failed += usize::from(!deleted);
                        }
                        let note = format!("{} legacy objects listed, {failed} not deleted; awaiting an empty listing", objects.len());
                        self.retry(item, note, now)
                    }
                }
            }
            // D3/D9: no provider deletion API exists and request flags prove nothing;
            // a possible dispatch stays unresolved until an operator approves it.
            ItemTarget::LlmDispatch { .. } => ItemState::Unresolved {
                reason: erasure::LLM_RETENTION_UNVERIFIED.into(),
                next_at: erasure::next_retry(item.attempts, now),
            },
            ItemTarget::Identity => match self.s.identity.delete_user(account).await {
                Ok(IdentityOutcome::Deleted) => ItemState::Confirmed { at: now },
                Ok(IdentityOutcome::Absent) => ItemState::Absent { at: now },
                Err(e) => self.retry(item, e, now),
            },
            ItemTarget::ProviderCopy { provider, remote } => match self.transcriber(*provider) {
                None => ItemState::Unresolved {
                    reason: format!("{provider} transport is not configured"),
                    next_at: erasure::next_retry(item.attempts, now),
                },
                Some(t) => match t.delete_remote(remote).await {
                    Ok(DeleteOutcome::Unsupported { .. }) => self.unsupported(NO_PROVIDER_DELETE_API, now),
                    other => self.deletion(item, other, now),
                },
            },
            ItemTarget::UnresolvedSubmission { provider, client_reference } => match self.transcriber(*provider) {
                None => ItemState::Unresolved {
                    reason: format!("{provider} transport is not configured"),
                    next_at: erasure::next_retry(item.attempts, now),
                },
                Some(t) if *provider == Provider::Deepgram => {
                    let _ = (t, client_reference);
                    self.unsupported(NO_PROVIDER_DELETE_API, now)
                }
                Some(t) => match t.find_by_reference(client_reference).await {
                    Err(e) => self.retry(item, e.to_string(), now),
                    Ok(records) if records.is_empty() => ItemState::Unresolved {
                        // A listing miss is not proof of nonacceptance; Soniox's own
                        // 30-day deletion is the backstop, so keep checking.
                        reason: "no matching provider record found yet".into(),
                        next_at: erasure::next_retry(item.attempts, now),
                    },
                    Ok(records) => {
                        let mut state = ItemState::Confirmed { at: now };
                        for r in records {
                            match self.deletion(item, t.delete_remote(&r.remote).await, now) {
                                ItemState::Confirmed { .. } | ItemState::Absent { .. } => {}
                                other => state = other,
                            }
                        }
                        state
                    }
                },
            },
        }
    }
}

fn elapsed_ms(start: Micros, end: Micros) -> u64 {
    ((end.0 - start.0).max(0) / 1000) as u64
}

#[cfg(test)]
mod tests;
