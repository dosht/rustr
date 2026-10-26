//! A test-only `JobStore` around the real `PgJobStore`. Every call passes through
//! unchanged, except two armed faults, neither known to production code:
//! - the first transaction that holds the armed generation's chat pair (its
//!   completing transaction) is committed for real, or rolled back, and then
//!   reported to the caller as a failed COMMIT, as when the database answer is lost;
//! - while `QuotaFault` is poisoned, every quota-counter operation of `AiTx`
//!   (`lock_counter`, `save_counter`, `counters`) fails with `StoreError::Unavailable`
//!   and is counted, so a test can also prove that a path never touched the store.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use billing_core::ports::{BillingTx, StoreError};
use media_ports::ObjectRef;
use transgate_core::account::ports::AccountTx;
use transgate_core::ai::GenerationId;
use transgate_core::ai::ports::{
    AiTx, ChatMessage, CompletionSummary, ConsentRecord, CounterKey, DispatchDisposition, Generation, GenerationKind, LlmDispatch,
    QuotaCounter, SummaryRequest,
};
use transgate_core::ids::{AccountId, AttemptId, ErasureId, InboxId, JobId, LineId, Micros};
use transgate_core::jobs::domain::{Attempt, Job, NewJob};
use transgate_core::jobs::events::JobEvent;
use transgate_core::jobs::ports::{
    ArchiveWrite, ArchivedPayload, BoxFuture, InboxEntry, InboxState, IssuedUpload, JobListQuery, JobQueries, JobStore,
    JobUow, LanguageUsage, NewInboxEntry, StoreResult,
};
use transgate_core::privacy::erasure::{AccountResources, ErasureItem, ErasureRecord};
use transgate_core::privacy::retention::{RetentionCandidate, RetentionRun};
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::{Line, NewLine};
use transgate_jobs_postgres::PgJobStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LostCommit {
    /// The completing transaction committed; only its answer is lost.
    Committed,
    /// The completing transaction did not commit, and the caller cannot tell.
    NotCommitted,
}

pub struct Arm {
    pub mode: LostCommit,
    pub generation: Mutex<Option<GenerationId>>,
    pub fired: AtomicBool,
}

impl Arm {
    pub fn new(mode: LostCommit) -> Arc<Arm> {
        Arc::new(Arm { mode, generation: Mutex::new(None), fired: AtomicBool::new(false) })
    }
}

/// Switch for the quota-store fault: while `poisoned`, counter operations fail.
/// `attempts` counts every counter operation tried, failed or not.
#[derive(Default)]
pub struct QuotaFault {
    pub poisoned: AtomicBool,
    pub attempts: AtomicUsize,
}

impl QuotaFault {
    fn hit(&self) -> bool {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        self.poisoned.load(Ordering::SeqCst)
    }
}

fn unavailable<'a, T: Send + 'a>() -> StoreResult<'a, T> {
    Box::pin(async { Err(StoreError::Unavailable("the quota store is unavailable".into())) })
}

#[derive(Clone)]
pub struct UncertainStore {
    pub inner: PgJobStore,
    pub arm: Arc<Arm>,
    pub quota: Arc<QuotaFault>,
}

impl JobQueries for UncertainStore {
    fn list_jobs<'a>(&'a self, query: &'a JobListQuery) -> StoreResult<'a, Vec<Job>> {
        self.inner.list_jobs(query)
    }
    fn owned_job(&self, account: AccountId, id: JobId) -> StoreResult<'_, Option<Job>> {
        self.inner.owned_job(account, id)
    }
    fn lines(&self, job: JobId) -> StoreResult<'_, Vec<Line>> {
        self.inner.lines(job)
    }
    fn language_usage(&self, account: AccountId) -> StoreResult<'_, LanguageUsage> {
        self.inner.language_usage(account)
    }
    fn settings(&self, account: AccountId) -> StoreResult<'_, UserSettings> {
        self.inner.settings(account)
    }
    fn dispatch_candidates(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Job>> {
        self.inner.dispatch_candidates(now, limit)
    }
    fn stuck_candidates(&self, started_before: Micros, limit: u32) -> StoreResult<'_, Vec<Job>> {
        self.inner.stuck_candidates(started_before, limit)
    }
    fn unresolved_attempts(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Attempt>> {
        self.inner.unresolved_attempts(now, limit)
    }
    fn pending_inbox(&self, limit: u32) -> StoreResult<'_, Vec<InboxEntry>> {
        self.inner.pending_inbox(limit)
    }
    fn remote_cleanup_due(&self, limit: u32) -> StoreResult<'_, Vec<Attempt>> {
        self.inner.remote_cleanup_due(limit)
    }
    fn erasures_due(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<ErasureId>> {
        self.inner.erasures_due(now, limit)
    }
    fn attempt(&self, id: AttemptId) -> StoreResult<'_, Option<Attempt>> {
        self.inner.attempt(id)
    }
    fn barrier(&self, account: AccountId) -> StoreResult<'_, bool> {
        self.inner.barrier(account)
    }
    fn runnable_generations(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<GenerationId>> {
        self.inner.runnable_generations(now, limit)
    }
    fn pending_summary_requests(&self, limit: u32) -> StoreResult<'_, Vec<(JobId, i64)>> {
        self.inner.pending_summary_requests(limit)
    }
    fn pending_ai_purges(&self, limit: u32) -> StoreResult<'_, Vec<AccountId>> {
        self.inner.pending_ai_purges(limit)
    }
}

impl JobStore for UncertainStore {
    fn begin(&self) -> StoreResult<'_, Box<dyn JobUow>> {
        Box::pin(async move {
            let inner = self.inner.begin().await?;
            Ok(Box::new(UncertainUow { inner, arm: self.arm.clone(), quota: self.quota.clone() }) as Box<dyn JobUow>)
        })
    }
}

pub struct UncertainUow {
    inner: Box<dyn JobUow>,
    arm: Arc<Arm>,
    quota: Arc<QuotaFault>,
}

impl JobUow for UncertainUow {
    fn ai(&mut self) -> &mut dyn AiTx {
        self
    }
    fn account(&mut self) -> &mut dyn AccountTx {
        self.inner.account()
    }
    fn billing(&mut self) -> BoxFuture<'_, Result<Box<dyn BillingTx + '_>, StoreError>> {
        self.inner.billing()
    }
    fn lock_capacity<'a>(&'a mut self, provider: media_ports::Provider, project: &'a str) -> StoreResult<'a, ()> {
        self.inner.lock_capacity(provider, project)
    }
    fn inflight<'a>(&'a mut self, provider: media_ports::Provider, project: &'a str, now: Micros) -> StoreResult<'a, u32> {
        self.inner.inflight(provider, project, now)
    }
    fn lock_jobs<'a>(&'a mut self, account: AccountId, ids: &'a [JobId]) -> StoreResult<'a, Vec<Job>> {
        self.inner.lock_jobs(account, ids)
    }
    fn lock_job(&mut self, id: JobId) -> StoreResult<'_, Option<Job>> {
        self.inner.lock_job(id)
    }
    fn completed_job_count(&mut self, account: AccountId) -> StoreResult<'_, u64> {
        self.inner.completed_job_count(account)
    }
    fn insert_job<'a>(&'a mut self, job: &'a NewJob) -> StoreResult<'a, Job> {
        self.inner.insert_job(job)
    }
    fn save_job<'a>(&'a mut self, job: &'a Job, expected_revision: i64) -> StoreResult<'a, i64> {
        self.inner.save_job(job, expected_revision)
    }
    fn record_issued_upload<'a>(&'a mut self, upload: &'a IssuedUpload) -> StoreResult<'a, ()> {
        self.inner.record_issued_upload(upload)
    }
    fn record_archived_payload<'a>(&'a mut self, payload: &'a ArchivedPayload) -> StoreResult<'a, ()> {
        self.inner.record_archived_payload(payload)
    }
    fn settle_archived_payload<'a>(&'a mut self, account: AccountId, object: &'a ObjectRef, write: ArchiveWrite) -> StoreResult<'a, ()> {
        self.inner.settle_archived_payload(account, object, write)
    }
    fn hard_delete_job(&mut self, id: JobId) -> StoreResult<'_, ()> {
        self.inner.hard_delete_job(id)
    }
    fn insert_attempt<'a>(&'a mut self, attempt: &'a Attempt) -> StoreResult<'a, ()> {
        self.inner.insert_attempt(attempt)
    }
    fn save_attempt<'a>(&'a mut self, attempt: &'a Attempt) -> StoreResult<'a, ()> {
        self.inner.save_attempt(attempt)
    }
    fn lock_attempt(&mut self, id: AttemptId) -> StoreResult<'_, Option<Attempt>> {
        self.inner.lock_attempt(id)
    }
    fn attempts_of(&mut self, job: JobId) -> StoreResult<'_, Vec<Attempt>> {
        self.inner.attempts_of(job)
    }
    fn insert_lines<'a>(&'a mut self, job: &'a Job, lines: &'a [NewLine]) -> StoreResult<'a, u64> {
        self.inner.insert_lines(job, lines)
    }
    fn lock_line(&mut self, id: LineId) -> StoreResult<'_, Option<Line>> {
        self.inner.lock_line(id)
    }
    fn update_line_text<'a>(&'a mut self, id: LineId, text: &'a str) -> StoreResult<'a, bool> {
        self.inner.update_line_text(id, text)
    }
    fn update_line_speaker<'a>(&'a mut self, id: LineId, speaker: &'a str) -> StoreResult<'a, bool> {
        self.inner.update_line_speaker(id, speaker)
    }
    fn rename_speaker<'a>(&'a mut self, job: JobId, from: &'a str, to: &'a str) -> StoreResult<'a, u64> {
        self.inner.rename_speaker(job, from, to)
    }
    fn insert_inbox<'a>(&'a mut self, entry: &'a NewInboxEntry) -> StoreResult<'a, (InboxEntry, bool)> {
        self.inner.insert_inbox(entry)
    }
    fn hold_inbox_conflict<'a>(&'a mut self, of: InboxId, entry: &'a NewInboxEntry) -> StoreResult<'a, ()> {
        self.inner.hold_inbox_conflict(of, entry)
    }
    fn lock_inbox(&mut self, id: InboxId) -> StoreResult<'_, Option<InboxEntry>> {
        self.inner.lock_inbox(id)
    }
    fn settle_inbox<'a>(&'a mut self, id: InboxId, state: InboxState, note: Option<&'a str>, at: Micros) -> StoreResult<'a, ()> {
        self.inner.settle_inbox(id, state, note, at)
    }
    fn record_inbox_failure<'a>(&'a mut self, id: InboxId, error: &'a str) -> StoreResult<'a, ()> {
        self.inner.record_inbox_failure(id, error)
    }
    fn emit<'a>(&'a mut self, event: &'a JobEvent) -> StoreResult<'a, ()> {
        self.inner.emit(event)
    }
    fn lock_settings(&mut self, account: AccountId) -> StoreResult<'_, UserSettings> {
        self.inner.lock_settings(account)
    }
    fn save_settings<'a>(&'a mut self, settings: &'a UserSettings) -> StoreResult<'a, ()> {
        self.inner.save_settings(settings)
    }
    fn retention_candidates(&mut self, cutoff: Micros) -> StoreResult<'_, Vec<RetentionCandidate>> {
        self.inner.retention_candidates(cutoff)
    }
    fn lock_retention_set<'a>(&'a mut self, candidates: &'a [RetentionCandidate], cutoff: Micros) -> StoreResult<'a, Vec<RetentionCandidate>> {
        self.inner.lock_retention_set(candidates, cutoff)
    }
    fn delete_lines_of<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, u64> {
        self.inner.delete_lines_of(jobs)
    }
    fn opted_out_count(&mut self, cutoff: Micros) -> StoreResult<'_, u64> {
        self.inner.opted_out_count(cutoff)
    }
    fn insert_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()> {
        self.inner.insert_retention_run(run)
    }
    fn save_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()> {
        self.inner.save_retention_run(run)
    }
    fn fail_interrupted_runs<'a>(&'a mut self, job_name: &'a str, message: &'a str, at: Micros) -> StoreResult<'a, u64> {
        self.inner.fail_interrupted_runs(job_name, message, at)
    }
    fn history_count<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64> {
        self.inner.history_count(schedules, cutoff)
    }
    fn delete_history<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64> {
        self.inner.delete_history(schedules, cutoff)
    }
    fn lock_barrier(&mut self, account: AccountId) -> StoreResult<'_, Option<ErasureRecord>> {
        self.inner.lock_barrier(account)
    }
    fn insert_erasure<'a>(&'a mut self, record: &'a ErasureRecord) -> StoreResult<'a, ()> {
        self.inner.insert_erasure(record)
    }
    fn insert_erasure_items<'a>(&'a mut self, items: &'a [ErasureItem]) -> StoreResult<'a, ()> {
        self.inner.insert_erasure_items(items)
    }
    fn lock_erasure(&mut self, id: ErasureId) -> StoreResult<'_, Option<ErasureRecord>> {
        self.inner.lock_erasure(id)
    }
    fn save_erasure<'a>(&'a mut self, record: &'a ErasureRecord) -> StoreResult<'a, ()> {
        self.inner.save_erasure(record)
    }
    fn erasure_items(&mut self, id: ErasureId) -> StoreResult<'_, Vec<ErasureItem>> {
        self.inner.erasure_items(id)
    }
    fn save_erasure_item<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()> {
        self.inner.save_erasure_item(item)
    }
    fn save_erasure_item_target<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()> {
        self.inner.save_erasure_item_target(item)
    }
    fn account_resources(&mut self, account: AccountId) -> StoreResult<'_, AccountResources> {
        self.inner.account_resources(account)
    }
    fn erase_account_rows(&mut self, account: AccountId) -> StoreResult<'_, ()> {
        self.inner.erase_account_rows(account)
    }
    fn savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        self.inner.savepoint(name)
    }
    fn rollback_to_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        self.inner.rollback_to_savepoint(name)
    }
    fn release_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        self.inner.release_savepoint(name)
    }
    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        self.inner.rollback()
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        let UncertainUow { mut inner, arm, .. } = *self;
        Box::pin(async move {
            let target = *arm.generation.lock().unwrap();
            let completing = match target {
                Some(id) if !arm.fired.load(Ordering::SeqCst) => inner.ai().chat_pair(id).await?.is_some(),
                _ => false,
            };
            if !completing {
                return inner.commit().await;
            }
            arm.fired.store(true, Ordering::SeqCst);
            match arm.mode {
                LostCommit::Committed => inner.commit().await?,
                LostCommit::NotCommitted => inner.rollback().await?,
            }
            Err(StoreError::Unavailable("the COMMIT answer was lost".into()))
        })
    }
}

impl AiTx for UncertainUow {
    fn consent(&mut self, account: AccountId) -> StoreResult<'_, Option<ConsentRecord>> {
        self.inner.ai().consent(account)
    }
    fn save_consent<'a>(&'a mut self, record: &'a ConsentRecord) -> StoreResult<'a, ()> {
        self.inner.ai().save_consent(record)
    }
    fn source_revision(&mut self, job: JobId) -> StoreResult<'_, i64> {
        self.inner.ai().source_revision(job)
    }
    fn bump_source_revision(&mut self, job: JobId) -> StoreResult<'_, i64> {
        self.inner.ai().bump_source_revision(job)
    }
    fn bump_source_revisions<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, ()> {
        self.inner.ai().bump_source_revisions(jobs)
    }
    fn generation_by_request(&mut self, account: AccountId, request_key: uuid::Uuid) -> StoreResult<'_, Option<Generation>> {
        self.inner.ai().generation_by_request(account, request_key)
    }
    fn active_metered(&mut self, account: AccountId) -> StoreResult<'_, Option<Generation>> {
        self.inner.ai().active_metered(account)
    }
    fn active_for_job(&mut self, account: AccountId, job: JobId, kind: GenerationKind) -> StoreResult<'_, Option<Generation>> {
        self.inner.ai().active_for_job(account, job, kind)
    }
    fn insert_generation<'a>(&'a mut self, generation: &'a Generation) -> StoreResult<'a, ()> {
        self.inner.ai().insert_generation(generation)
    }
    fn lock_generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>> {
        self.inner.ai().lock_generation(id)
    }
    fn save_generation<'a>(&'a mut self, generation: &'a Generation) -> StoreResult<'a, ()> {
        self.inner.ai().save_generation(generation)
    }
    fn generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>> {
        self.inner.ai().generation(id)
    }
    fn latest_analysis(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<Generation>> {
        self.inner.ai().latest_analysis(account, job)
    }
    fn lock_counter(&mut self, key: CounterKey) -> StoreResult<'_, Option<QuotaCounter>> {
        if self.quota.hit() {
            return unavailable();
        }
        self.inner.ai().lock_counter(key)
    }
    fn save_counter<'a>(&'a mut self, counter: &'a QuotaCounter) -> StoreResult<'a, ()> {
        if self.quota.hit() {
            return unavailable();
        }
        self.inner.ai().save_counter(counter)
    }
    fn counters(&mut self, account: AccountId, job: Option<JobId>) -> StoreResult<'_, Vec<QuotaCounter>> {
        if self.quota.hit() {
            return unavailable();
        }
        self.inner.ai().counters(account, job)
    }
    fn append_chat_pair<'a>(&'a mut self, question: &'a ChatMessage, answer: &'a ChatMessage) -> StoreResult<'a, [ChatMessage; 2]> {
        self.inner.ai().append_chat_pair(question, answer)
    }
    fn chat_pair(&mut self, generation: GenerationId) -> StoreResult<'_, Option<[ChatMessage; 2]>> {
        self.inner.ai().chat_pair(generation)
    }
    fn chat_history(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Vec<ChatMessage>> {
        self.inner.ai().chat_history(account, job)
    }
    fn save_completion_summary<'a>(&'a mut self, summary: &'a CompletionSummary) -> StoreResult<'a, ()> {
        self.inner.ai().save_completion_summary(summary)
    }
    fn completion_summary(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<CompletionSummary>> {
        self.inner.ai().completion_summary(account, job)
    }
    fn request_summary<'a>(&'a mut self, request: &'a SummaryRequest) -> StoreResult<'a, ()> {
        self.inner.ai().request_summary(request)
    }
    fn lock_summary_request(&mut self, job: JobId, source_revision: i64) -> StoreResult<'_, Option<SummaryRequest>> {
        self.inner.ai().lock_summary_request(job, source_revision)
    }
    fn save_summary_request<'a>(&'a mut self, request: &'a SummaryRequest) -> StoreResult<'a, ()> {
        self.inner.ai().save_summary_request(request)
    }
    fn summary_request(&mut self, job: JobId) -> StoreResult<'_, Option<SummaryRequest>> {
        self.inner.ai().summary_request(job)
    }
    fn purge_ai_artifacts(&mut self, account: AccountId, cutoff: i64) -> StoreResult<'_, u64> {
        self.inner.ai().purge_ai_artifacts(account, cutoff)
    }
    fn record_dispatch<'a>(&'a mut self, dispatch: &'a LlmDispatch) -> StoreResult<'a, ()> {
        self.inner.ai().record_dispatch(dispatch)
    }
    fn settle_dispatch<'a>(
        &'a mut self,
        account: AccountId,
        generation: GenerationId,
        disposition: DispatchDisposition,
        observed: Option<&'a str>,
    ) -> StoreResult<'a, bool> {
        self.inner.ai().settle_dispatch(account, generation, disposition, observed)
    }
}
