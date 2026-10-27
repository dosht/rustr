//! In-memory doubles for the use-case tests. Each `begin` copies the state and
//! `commit` writes it back, so a rolled-back unit of work leaves nothing behind.
//! Concurrency and SQL semantics are proved against PostgreSQL in jobs-postgres.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use billing_core::ledger::{Package, PackageBalance};
use billing_core::model::NewUsage;
use billing_core::erasure::{
    CheckoutClosure, ErasureCounts, ErasureStatus, SubscriptionTermination, TerminationEvidence, TerminationTarget,
};
use billing_core::ports::{BillingStore, BillingTx, GatewayError, StoreError, StoreResult as BillingResult, UnitOfWork};
use billing_core::types::{BoxFuture, Clock, SubjectRef};
use media_ports::{
    AccessMethod, CallbackBinding, CallbackError, DeleteOutcome, Headers, ObjectMetadata, ObjectRef, ObjectStore,
    ObservationOrigin, ProviderObservation, PutObject, RemoteIdentity, RemoteRecord, SensitiveUrl, StoreKind, Submission,
    TemporaryAccess, Transcriber, TranscriptionRequest, TransportError,
};

use crate::account::ports::{AccountTx, ClaimedSupport, SupportOutbox};
use crate::account::{JobFeedback, SupportRequest};
use crate::ai::GenerationId;
use crate::ai::ports::{
    AiTx, ChatMessage, CompletionSummary, ConsentRecord, CounterKey, CounterScope, Generation, GenerationKind, QuotaCounter,
    DispatchDisposition, LlmDispatch, SummaryRequest,
};
use crate::ids::{AccountId, AttemptId, ErasureId, InboxId, JobId, LineId, Micros};
use crate::jobs::domain::{Attempt, AttemptState, Job, JobStatus, NewJob, RemoteCleanup};
use crate::jobs::events::JobEvent;
use crate::jobs::ports::*;
use crate::privacy::erasure::{AccountResources, ErasureItem, ErasureRecord, ItemTarget};
use crate::privacy::retention::{RetentionCandidate, RetentionRun, RunStatus};
use crate::privacy::settings::UserSettings;
use crate::transcript::{Line, NewLine};

#[derive(Clone, Default)]
pub struct Data {
    pub accounts: BTreeSet<AccountId>,
    pub jobs: BTreeMap<i64, Job>,
    pub next_job: i64,
    pub attempts: BTreeMap<AttemptId, Attempt>,
    pub lines: Vec<Line>,
    pub next_line: i64,
    pub inbox: BTreeMap<i64, InboxEntry>,
    pub next_inbox: i64,
    pub events: Vec<JobEvent>,
    pub settings: BTreeMap<AccountId, UserSettings>,
    pub runs: Vec<RetentionRun>,
    pub erasures: BTreeMap<ErasureId, ErasureRecord>,
    pub items: Vec<ErasureItem>,
    pub packages: Vec<Package>,
    pub usage: Vec<NewUsage>,
    pub uploads: Vec<IssuedUpload>,
    pub archived: Vec<ArchivedPayload>,
    pub billing_erased: Vec<AccountId>,
    pub billing_erasures: BTreeMap<AccountId, ErasureStatus>,
    /// Verified subscription bindings billing would find for an account.
    pub bound_subscriptions: Vec<(AccountId, TerminationTarget)>,
    pub termination_attempts: u32,
    /// Makes the next line insert fail with a constraint conflict.
    pub fail_line_insert: bool,
    /// The scheduler's own run history (PRV-025).
    pub schedule_runs: Vec<ScheduleRun>,
    pub ai: AiData,
    /// `job_feedback`, in insertion order.
    pub feedback: Vec<JobFeedback>,
    /// The support outbox, oldest first.
    pub support: Vec<SupportRow>,
    pub api: ApiData,
}

/// The P7 API rows of the in-memory store.
#[derive(Clone, Default)]
pub struct ApiData {
    pub keys: Vec<crate::api::keys::ApiKey>,
    pub idempotency: Vec<crate::api::idempotency::IdempotencyRecord>,
    pub deliveries: Vec<crate::api::callbacks::CallbackDelivery>,
    /// Delivery leases: claim token and lease end.
    pub leases: BTreeMap<crate::api::callbacks::DeliveryId, (uuid::Uuid, Micros)>,
    pub attempts: Vec<crate::api::ports::CallbackAttempt>,
    pub secrets: BTreeMap<AccountId, String>,
}

/// One undelivered support request and its delivery bookkeeping.
#[derive(Clone, Debug)]
pub struct SupportRow {
    pub request: SupportRequest,
    pub attempts: i32,
    /// Claimable from this instant (a lease or a retry delay).
    pub available_at: Micros,
    pub error: Option<String>,
}

/// The AI rows of the in-memory store.
#[derive(Clone, Default)]
pub struct AiData {
    pub consents: BTreeMap<AccountId, ConsentRecord>,
    pub revisions: BTreeMap<i64, i64>,
    /// In insertion order.
    pub generations: Vec<Generation>,
    pub counters: Vec<QuotaCounter>,
    pub chat: Vec<ChatMessage>,
    pub summaries: Vec<CompletionSummary>,
    pub summary_requests: Vec<SummaryRequest>,
    /// Makes the next purge fail (storage error).
    pub fail_purge: bool,
    /// While set, every quota-counter read and write fails (storage error).
    pub fail_counters: bool,
    /// While set, the billing plan (package) lookup fails (storage error).
    pub fail_packages: bool,
    /// Payload-free LLM dispatch intents; untouched by purges and hard deletes.
    pub dispatches: Vec<LlmDispatch>,
}

#[derive(Clone, Debug)]
pub struct ScheduleRun {
    pub schedule: String,
    pub started_at: Micros,
    pub finished: bool,
}

#[derive(Clone, Default)]
pub struct MemStore {
    pub data: Arc<Mutex<Data>>,
}

impl MemStore {
    pub fn with<R>(&self, f: impl FnOnce(&mut Data) -> R) -> R {
        f(&mut self.data.lock().unwrap())
    }
}

fn ok<'a, T: Send + 'a>(v: T) -> StoreResult<'a, T> {
    Box::pin(async move { Ok(v) })
}

fn balances(d: &Data, account: AccountId) -> Vec<PackageBalance> {
    d.packages
        .iter()
        .filter(|p| p.account == account)
        .map(|p| PackageBalance {
            package: p.clone(),
            consumed: d.usage.iter().filter(|u| u.package_id == p.id).map(|u| u.minutes).sum(),
        })
        .collect()
}

impl JobQueries for MemStore {
    fn list_jobs<'a>(&'a self, q: &'a JobListQuery) -> StoreResult<'a, Vec<Job>> {
        let mut jobs: Vec<Job> = self.with(|d| {
            d.jobs
                .values()
                .filter(|j| j.account == q.account && q.statuses.contains(&j.status))
                .filter(|j| q.since.is_none_or(|s| j.created_at.0 >= s.0))
                .filter(|j| q.before.is_none_or(|b| j.id.0 < b.0))
                .cloned()
                .collect()
        });
        jobs.sort_by_key(|job| std::cmp::Reverse(job.id));
        jobs.truncate(q.limit as usize);
        ok(jobs)
    }
    fn owned_job(&self, account: AccountId, id: JobId) -> StoreResult<'_, Option<Job>> {
        ok(self.with(|d| d.jobs.get(&id.0).filter(|j| j.account == account).cloned()))
    }
    fn lines(&self, job: JobId) -> StoreResult<'_, Vec<Line>> {
        let mut lines: Vec<Line> = self.with(|d| d.lines.iter().filter(|l| l.job == job).cloned().collect());
        crate::transcript::sort_lines(&mut lines);
        ok(lines)
    }
    fn language_usage(&self, _account: AccountId) -> StoreResult<'_, LanguageUsage> {
        ok(LanguageUsage::default())
    }
    fn settings(&self, account: AccountId) -> StoreResult<'_, UserSettings> {
        ok(self.with(|d| d.settings.get(&account).cloned().unwrap_or_else(|| UserSettings::defaults(account))))
    }
    fn dispatch_candidates(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Job>> {
        let mut jobs: Vec<Job> = self.with(|d| {
            d.jobs
                .values()
                .filter(|j| matches!(j.status, JobStatus::NotStarted | JobStatus::Pending))
                .filter(|j| j.next_dispatch_at.is_none_or(|t| t.0 <= now.0))
                .filter(|j| j.current_attempt.and_then(|a| d.attempts.get(&a)).is_none_or(|a| !a.is_live(now)))
                .cloned()
                .collect()
        });
        jobs.sort_by_key(|j| (j.status != JobStatus::Pending, j.created_at));
        jobs.truncate(limit as usize);
        ok(jobs)
    }
    fn stuck_candidates(&self, before: Micros, _limit: u32) -> StoreResult<'_, Vec<Job>> {
        ok(self.with(|d| {
            d.jobs
                .values()
                .filter(|j| j.status == JobStatus::Running && j.started_at.is_some_and(|s| s.0 < before.0))
                .cloned()
                .collect()
        }))
    }
    fn unresolved_attempts(&self, now: Micros, _limit: u32) -> StoreResult<'_, Vec<Attempt>> {
        ok(self.with(|d| {
            d.attempts
                .values()
                .filter(|a| (a.state == AttemptState::Claimed && a.lease_until.0 <= now.0) || a.state == AttemptState::Uncertain)
                .cloned()
                .collect()
        }))
    }
    fn pending_inbox(&self, _limit: u32) -> StoreResult<'_, Vec<InboxEntry>> {
        ok(self.with(|d| d.inbox.values().filter(|e| e.state == InboxState::Pending).cloned().collect()))
    }
    fn remote_cleanup_due(&self, _limit: u32) -> StoreResult<'_, Vec<Attempt>> {
        ok(self.with(|d| d.attempts.values().filter(|a| a.remote_cleanup == RemoteCleanup::Pending).cloned().collect()))
    }
    fn erasures_due(&self, _now: Micros, _limit: u32) -> StoreResult<'_, Vec<ErasureId>> {
        ok(self.with(|d| d.erasures.keys().copied().collect()))
    }
    fn attempt(&self, id: AttemptId) -> StoreResult<'_, Option<Attempt>> {
        ok(self.with(|d| d.attempts.get(&id).cloned()))
    }
    fn barrier(&self, account: AccountId) -> StoreResult<'_, bool> {
        ok(self.with(|d| d.erasures.values().any(|e| e.account == account)))
    }
    fn runnable_generations(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<GenerationId>> {
        ok(self.with(|d| {
            d.ai.generations
                .iter()
                .filter(|g| g.status == crate::ai::GenerationStatus::Accepted || crate::ai::generation::lease_expired(g, now))
                .take(limit as usize)
                .map(|g| g.id)
                .collect()
        }))
    }
    fn pending_summary_requests(&self, limit: u32) -> StoreResult<'_, Vec<(JobId, i64)>> {
        ok(self.with(|d| d.ai.summary_requests.iter().filter(|r| r.outcome.is_none()).take(limit as usize).map(|r| (r.job, r.source_revision)).collect()))
    }
    fn pending_ai_purges(&self, limit: u32) -> StoreResult<'_, Vec<AccountId>> {
        ok(self.with(|d| d.ai.consents.values().filter(|c| c.purge_pending).take(limit as usize).map(|c| c.account).collect()))
    }
}

impl JobStore for MemStore {
    fn begin(&self) -> StoreResult<'_, Box<dyn JobUow>> {
        let snapshot = self.with(|d| d.clone());
        ok(Box::new(MemUow { data: snapshot, store: self.data.clone(), savepoints: Vec::new() }) as Box<dyn JobUow>)
    }
}

pub struct MemUow {
    pub data: Data,
    store: Arc<Mutex<Data>>,
    savepoints: Vec<(String, Data)>,
}

fn conflict<'a, T: Send + 'a>(what: &str) -> StoreResult<'a, T> {
    let e = StoreError::Conflict(what.to_string());
    Box::pin(async move { Err(e) })
}

impl AccountTx for MemUow {
    fn insert_feedback<'a>(&'a mut self, feedback: &'a JobFeedback) -> StoreResult<'a, ()> {
        // Like the foreign key: feedback needs an existing job.
        if !self.data.jobs.contains_key(&feedback.job.0) {
            return conflict("job_feedback.job_id");
        }
        self.data.feedback.push(feedback.clone());
        ok(())
    }
    fn insert_support<'a>(&'a mut self, request: &'a SupportRequest) -> StoreResult<'a, ()> {
        self.data.support.push(SupportRow { request: request.clone(), attempts: 0, available_at: request.created_at, error: None });
        ok(())
    }
}

impl crate::api::ports::ApiTx for MemUow {
    fn insert_key<'a>(&'a mut self, key: &'a crate::api::keys::ApiKey) -> StoreResult<'a, ()> {
        if self.data.api.keys.iter().any(|k| k.id == key.id || (key.digest.is_some() && k.digest == key.digest)) {
            return conflict("api_keys");
        }
        self.data.api.keys.push(key.clone());
        ok(())
    }
    fn lock_key(&mut self, id: crate::api::KeyId) -> StoreResult<'_, Option<crate::api::keys::ApiKey>> {
        ok(self.data.api.keys.iter().find(|k| k.id == id).cloned())
    }
    fn save_key<'a>(&'a mut self, key: &'a crate::api::keys::ApiKey) -> StoreResult<'a, ()> {
        match self.data.api.keys.iter_mut().find(|k| k.id == key.id) {
            Some(row) => {
                *row = key.clone();
                ok(())
            }
            None => conflict("api_keys"),
        }
    }
    fn key_by_digest<'a>(&'a mut self, digest: &'a str) -> StoreResult<'a, Option<crate::api::keys::ApiKey>> {
        ok(self.data.api.keys.iter().find(|k| k.digest.as_deref() == Some(digest)).cloned())
    }
    fn legacy_candidates(&mut self, now: Micros) -> StoreResult<'_, Vec<crate::api::keys::ApiKey>> {
        ok(self
            .data
            .api
            .keys
            .iter()
            .filter(|k| k.active && k.digest.is_none() && k.legacy_bcrypt.is_some() && k.expires_at.is_none_or(|e| e >= now))
            .cloned()
            .collect())
    }
    fn list_keys(&mut self, account: AccountId) -> StoreResult<'_, Vec<crate::api::keys::ApiKey>> {
        let mut keys: Vec<_> = self.data.api.keys.iter().filter(|k| k.account == account).cloned().collect();
        keys.sort_by_key(|k| std::cmp::Reverse(k.created_at));
        ok(keys)
    }
    fn idempotency<'a>(&'a mut self, account: AccountId, key: &'a str, expired_before: Micros) -> StoreResult<'a, Option<crate::api::idempotency::IdempotencyRecord>> {
        ok(self.data.api.idempotency.iter().find(|r| r.account == account && r.key == key && r.created_at > expired_before).cloned())
    }
    fn insert_idempotency<'a>(&'a mut self, record: &'a crate::api::idempotency::IdempotencyRecord, expired_before: Micros) -> StoreResult<'a, ()> {
        let rows = &mut self.data.api.idempotency;
        if rows.iter().any(|r| r.account == record.account && r.key == record.key && r.created_at > expired_before) {
            return conflict("idempotency_keys");
        }
        rows.retain(|r| !(r.account == record.account && r.key == record.key));
        rows.push(record.clone());
        ok(())
    }
    fn save_idempotency_answer<'a>(&'a mut self, account: AccountId, key: &'a str, answer: &'a crate::jobs::app::StartAnswer) -> StoreResult<'a, ()> {
        if let Some(r) = self.data.api.idempotency.iter_mut().find(|r| r.account == account && r.key == key) {
            r.answer = Some(answer.clone());
        }
        ok(())
    }
    fn insert_delivery<'a>(&'a mut self, delivery: &'a crate::api::callbacks::CallbackDelivery) -> StoreResult<'a, bool> {
        if self.data.api.deliveries.iter().any(|d| d.event_id == delivery.event_id) {
            return ok(false);
        }
        self.data.api.deliveries.push(delivery.clone());
        ok(true)
    }
}

impl crate::api::ports::CallbackOutbox for MemStore {
    fn claim_deliveries(&self, limit: u32, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<crate::api::ports::ClaimedDelivery>> {
        use crate::api::callbacks::DeliveryState;
        ok(self.with(|d| {
            let api = &mut d.api;
            let mut due: Vec<_> = api
                .deliveries
                .iter()
                .filter(|x| x.state == DeliveryState::Pending && x.next_attempt_at <= now && api.leases.get(&x.id).is_none_or(|(_, until)| *until <= now))
                .cloned()
                .collect();
            due.sort_by_key(|x| (x.next_attempt_at, x.created_at));
            due.truncate(limit as usize);
            due.into_iter()
                .map(|delivery| {
                    let token = uuid::Uuid::new_v4();
                    api.leases.insert(delivery.id, (token, Micros(now.0 + lease_seconds * 1_000_000)));
                    crate::api::ports::ClaimedDelivery { delivery, token }
                })
                .collect()
        }))
    }
    fn postpone<'a>(&'a self, claim: &'a crate::api::ports::ClaimedDelivery, until: Micros) -> StoreResult<'a, bool> {
        ok(self.with(|d| {
            let api = &mut d.api;
            if api.leases.get(&claim.delivery.id).map(|(t, _)| *t) != Some(claim.token) {
                return false;
            }
            api.leases.remove(&claim.delivery.id);
            if let Some(x) = api.deliveries.iter_mut().find(|x| x.id == claim.delivery.id) {
                x.next_attempt_at = until;
            }
            true
        }))
    }
    fn begin_attempt<'a>(
        &'a self,
        claim: &'a crate::api::ports::ClaimedDelivery,
        attempt: &'a crate::api::ports::CallbackAttempt,
        payload: &'a serde_json::Value,
    ) -> StoreResult<'a, bool> {
        ok(self.with(|d| {
            let api = &mut d.api;
            if api.leases.get(&claim.delivery.id).map(|(t, _)| *t) != Some(claim.token) {
                return false;
            }
            let Some(x) = api.deliveries.iter_mut().find(|x| x.id == claim.delivery.id) else { return false };
            x.attempts = attempt.n;
            x.payload = Some(payload.clone());
            api.attempts.push(attempt.clone());
            true
        }))
    }
    fn finish_attempt<'a>(
        &'a self,
        claim: &'a crate::api::ports::ClaimedDelivery,
        attempt: &'a crate::api::ports::CallbackAttempt,
        settlement: &'a crate::api::callbacks::Settlement,
    ) -> StoreResult<'a, bool> {
        ok(self.with(|d| {
            let api = &mut d.api;
            if api.leases.get(&claim.delivery.id).map(|(t, _)| *t) != Some(claim.token) {
                return false;
            }
            api.leases.remove(&claim.delivery.id);
            if let Some(a) = api.attempts.iter_mut().find(|a| a.delivery == attempt.delivery && a.n == attempt.n) {
                *a = attempt.clone();
            }
            if let Some(x) = api.deliveries.iter_mut().find(|x| x.id == claim.delivery.id) {
                x.state = settlement.state;
                x.delivered_at = settlement.delivered_at;
                if let Some(next) = settlement.next_attempt_at {
                    x.next_attempt_at = next;
                }
            }
            true
        }))
    }
    fn abandon<'a>(&'a self, claim: &'a crate::api::ports::ClaimedDelivery, _reason: &'a str, _now: Micros) -> StoreResult<'a, bool> {
        ok(self.with(|d| {
            let api = &mut d.api;
            if api.leases.get(&claim.delivery.id).map(|(t, _)| *t) != Some(claim.token) {
                return false;
            }
            api.leases.remove(&claim.delivery.id);
            if let Some(x) = api.deliveries.iter_mut().find(|x| x.id == claim.delivery.id) {
                x.state = crate::api::callbacks::DeliveryState::Abandoned;
            }
            true
        }))
    }
    fn reopen(&self, id: crate::api::callbacks::DeliveryId, now: Micros) -> StoreResult<'_, crate::api::ports::Reopen> {
        use crate::api::callbacks::DeliveryState;
        use crate::api::ports::Reopen;
        ok(self.with(|d| {
            let Some(x) = d.api.deliveries.iter_mut().find(|x| x.id == id) else { return Reopen::Missing };
            if x.state == DeliveryState::Pending {
                return Reopen::StillPending;
            }
            x.state = DeliveryState::Pending;
            x.next_attempt_at = now;
            x.delivered_at = None;
            Reopen::Reopened
        }))
    }
    fn signing_secret<'a>(&'a self, account: AccountId, candidate: &'a str) -> StoreResult<'a, String> {
        ok(self.with(|d| d.api.secrets.entry(account).or_insert_with(|| candidate.to_string()).clone()))
    }
}

impl SupportOutbox for MemStore {
    fn claim_support(&self, limit: u32, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<ClaimedSupport>> {
        ok(self.with(|d| {
            d.support
                .iter_mut()
                .filter(|row| row.available_at <= now)
                .take(limit as usize)
                .map(|row| {
                    row.attempts += 1;
                    row.available_at = Micros(now.0 + lease_seconds * 1_000_000);
                    ClaimedSupport { request: row.request.clone(), attempts: row.attempts }
                })
                .collect()
        }))
    }
    fn ack_support<'a>(&'a self, claim: &'a ClaimedSupport) -> StoreResult<'a, bool> {
        let fenced = |row: &SupportRow| row.request.id == claim.request.id && row.attempts == claim.attempts;
        ok(self.with(|d| {
            let before = d.support.len();
            d.support.retain(|row| !fenced(row));
            d.support.len() < before
        }))
    }
    fn release_support<'a>(&'a self, claim: &'a ClaimedSupport, error: &'a str, retry_at: Micros) -> StoreResult<'a, bool> {
        ok(self.with(|d| match d.support.iter_mut().find(|row| row.request.id == claim.request.id && row.attempts == claim.attempts) {
            Some(row) => {
                row.available_at = retry_at;
                row.error = Some(error.to_string());
                true
            }
            None => false,
        }))
    }
}

impl AiTx for MemUow {
    fn consent(&mut self, account: AccountId) -> StoreResult<'_, Option<ConsentRecord>> {
        ok(self.data.ai.consents.get(&account).cloned())
    }
    fn save_consent<'a>(&'a mut self, record: &'a ConsentRecord) -> StoreResult<'a, ()> {
        self.data.ai.consents.insert(record.account, record.clone());
        ok(())
    }
    fn source_revision(&mut self, job: JobId) -> StoreResult<'_, i64> {
        ok(self.data.ai.revisions.get(&job.0).copied().unwrap_or(0))
    }
    fn bump_source_revisions<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, ()> {
        for job in jobs {
            *self.data.ai.revisions.entry(job.0).or_insert(0) += 1;
        }
        ok(())
    }
    fn bump_source_revision(&mut self, job: JobId) -> StoreResult<'_, i64> {
        let r = self.data.ai.revisions.entry(job.0).or_insert(0);
        *r += 1;
        ok(*r)
    }
    fn generation_by_request(&mut self, account: AccountId, request_key: uuid::Uuid) -> StoreResult<'_, Option<Generation>> {
        ok(self.data.ai.generations.iter().find(|g| g.account == account && g.request_key == request_key).cloned())
    }
    fn active_metered(&mut self, account: AccountId) -> StoreResult<'_, Option<Generation>> {
        ok(self.data.ai.generations.iter().find(|g| g.account == account && g.quota.is_some() && !g.status.is_terminal()).cloned())
    }
    fn active_for_job(&mut self, account: AccountId, job: JobId, kind: GenerationKind) -> StoreResult<'_, Option<Generation>> {
        ok(self.data.ai.generations.iter().find(|g| g.account == account && g.job == job && g.kind == kind && !g.status.is_terminal()).cloned())
    }
    fn insert_generation<'a>(&'a mut self, g: &'a Generation) -> StoreResult<'a, ()> {
        let ai = &mut self.data.ai;
        if ai.generations.iter().any(|x| x.account == g.account && x.request_key == g.request_key) {
            return conflict("duplicate request key");
        }
        if g.quota.is_some() && ai.generations.iter().any(|x| x.account == g.account && x.quota.is_some() && !x.status.is_terminal()) {
            return conflict("another metered generation is active");
        }
        ai.generations.push(g.clone());
        ok(())
    }
    fn lock_generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>> {
        ok(self.data.ai.generations.iter().find(|g| g.id == id).cloned())
    }
    fn save_generation<'a>(&'a mut self, g: &'a Generation) -> StoreResult<'a, ()> {
        match self.data.ai.generations.iter_mut().find(|x| x.id == g.id) {
            Some(row) => {
                *row = g.clone();
                ok(())
            }
            None => conflict("generation is gone"),
        }
    }
    fn generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>> {
        ok(self.data.ai.generations.iter().find(|g| g.id == id).cloned())
    }
    fn latest_analysis(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<Generation>> {
        ok(self.data.ai.generations.iter().rev().find(|g| g.account == account && g.job == job && g.kind == GenerationKind::Analysis).cloned())
    }
    fn lock_counter(&mut self, key: CounterKey) -> StoreResult<'_, Option<QuotaCounter>> {
        if self.data.ai.fail_counters {
            return Box::pin(async { Err(StoreError::Unavailable("quota store unavailable".into())) });
        }
        ok(self.data.ai.counters.iter().find(|c| c.key == key).copied())
    }
    fn save_counter<'a>(&'a mut self, counter: &'a QuotaCounter) -> StoreResult<'a, ()> {
        if self.data.ai.fail_counters {
            return Box::pin(async { Err(StoreError::Unavailable("quota store unavailable".into())) });
        }
        match self.data.ai.counters.iter_mut().find(|c| c.key == counter.key) {
            Some(row) => *row = *counter,
            None => self.data.ai.counters.push(*counter),
        }
        ok(())
    }
    fn counters(&mut self, account: AccountId, job: Option<JobId>) -> StoreResult<'_, Vec<QuotaCounter>> {
        if self.data.ai.fail_counters {
            return Box::pin(async { Err(StoreError::Unavailable("quota store unavailable".into())) });
        }
        ok(self
            .data
            .ai
            .counters
            .iter()
            .filter(|c| c.key.account == account && (c.key.scope == CounterScope::User || job.is_some_and(|j| c.key.scope == CounterScope::Transcript(j))))
            .copied()
            .collect())
    }
    fn append_chat_pair<'a>(&'a mut self, question: &'a ChatMessage, answer: &'a ChatMessage) -> StoreResult<'a, [ChatMessage; 2]> {
        let ai = &mut self.data.ai;
        if ai.chat.iter().any(|m| m.generation == question.generation) {
            return conflict("the generation already has a pair");
        }
        let next = ai.chat.iter().filter(|m| m.job == question.job).map(|m| m.order).max().map_or(1, |o| o + 1);
        let pair = [ChatMessage { order: next, ..question.clone() }, ChatMessage { order: next + 1, ..answer.clone() }];
        ai.chat.extend(pair.iter().cloned());
        ok(pair)
    }
    fn chat_pair(&mut self, generation: GenerationId) -> StoreResult<'_, Option<[ChatMessage; 2]>> {
        let rows: Vec<ChatMessage> = self.data.ai.chat.iter().filter(|m| m.generation == generation).cloned().collect();
        ok(<[ChatMessage; 2]>::try_from(rows).ok())
    }
    fn chat_history(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Vec<ChatMessage>> {
        let mut rows: Vec<ChatMessage> = self.data.ai.chat.iter().filter(|m| m.account == account && m.job == job).cloned().collect();
        rows.sort_by_key(|m| m.order);
        ok(rows)
    }
    fn save_completion_summary<'a>(&'a mut self, s: &'a CompletionSummary) -> StoreResult<'a, ()> {
        if !self.data.ai.summaries.iter().any(|x| x.job == s.job && x.source_revision == s.source_revision) {
            self.data.ai.summaries.push(s.clone());
        }
        ok(())
    }
    fn completion_summary(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<CompletionSummary>> {
        ok(self.data.ai.summaries.iter().rev().find(|s| s.account == account && s.job == job).cloned())
    }
    fn request_summary<'a>(&'a mut self, r: &'a SummaryRequest) -> StoreResult<'a, ()> {
        if !self.data.ai.summary_requests.iter().any(|x| x.job == r.job && x.source_revision == r.source_revision) {
            self.data.ai.summary_requests.push(r.clone());
        }
        ok(())
    }
    fn lock_summary_request(&mut self, job: JobId, source_revision: i64) -> StoreResult<'_, Option<SummaryRequest>> {
        ok(self.data.ai.summary_requests.iter().find(|r| r.job == job && r.source_revision == source_revision).cloned())
    }
    fn save_summary_request<'a>(&'a mut self, r: &'a SummaryRequest) -> StoreResult<'a, ()> {
        match self.data.ai.summary_requests.iter_mut().find(|x| x.job == r.job && x.source_revision == r.source_revision) {
            Some(row) => {
                *row = r.clone();
                ok(())
            }
            None => conflict("summary request is gone"),
        }
    }
    fn summary_request(&mut self, job: JobId) -> StoreResult<'_, Option<SummaryRequest>> {
        ok(self.data.ai.summary_requests.iter().rev().find(|r| r.job == job).cloned())
    }
    fn purge_ai_artifacts(&mut self, account: AccountId, cutoff: i64) -> StoreResult<'_, u64> {
        let ai = &mut self.data.ai;
        if ai.fail_purge {
            let e = StoreError::Unavailable("purge failed".into());
            return Box::pin(async move { Err(e) });
        }
        let before = ai.generations.len() + ai.chat.len() + ai.summaries.len();
        ai.generations.retain(|g| !(g.account == account && g.consent_epoch <= cutoff));
        ai.chat.retain(|m| !(m.account == account && m.consent_epoch <= cutoff));
        ai.summaries.retain(|s| !(s.account == account && s.consent_epoch <= cutoff));
        let removed = (before - ai.generations.len() - ai.chat.len() - ai.summaries.len()) as u64;
        // P7 D10: a stored callback body may carry the consent-gated summary.
        self.data.api.deliveries.iter_mut().filter(|d| d.account == account).for_each(|d| d.payload = None);
        ok(removed)
    }
    fn record_dispatch<'a>(&'a mut self, d: &'a LlmDispatch) -> StoreResult<'a, ()> {
        match self.data.ai.dispatches.iter().find(|x| x.generation == d.generation) {
            Some(x) if x.account != d.account || x.provider != d.provider || x.model != d.model => conflict("another dispatch for this generation"),
            Some(_) => ok(()),
            None => {
                self.data.ai.dispatches.push(d.clone());
                ok(())
            }
        }
    }
    fn settle_dispatch<'a>(
        &'a mut self,
        account: AccountId,
        generation: GenerationId,
        disposition: DispatchDisposition,
        observed: Option<&'a str>,
    ) -> StoreResult<'a, bool> {
        match self.data.ai.dispatches.iter_mut().find(|x| x.account == account && x.generation == generation) {
            Some(x) => {
                x.settle(disposition, observed);
                ok(true)
            }
            None => ok(false),
        }
    }
}

/// Billing joined to a job unit of work.
struct MemBilling<'a>(&'a mut Data);

impl MemBilling<'_> {
    fn d(&mut self) -> &mut Data {
        self.0
    }
}

/// A transaction billing opened itself (erasure step 2).
pub struct MemBillingUow {
    data: Data,
    store: Arc<Mutex<Data>>,
}

impl MemBillingUow {
    fn d(&mut self) -> &mut Data {
        &mut self.data
    }
}

macro_rules! unused {
    () => {
        unreachable!("not used by transcription tests")
    };
}

fn done<'a, T: Send + 'a>(v: Result<T, StoreError>) -> BillingResult<'a, T> {
    Box::pin(async move { v })
}

/// One body for both billing views: they differ only in where `Data` lives.
macro_rules! billing_tx {
    ($ty:ty) => {
        impl BillingTx for $ty {
            fn create_account(&mut self, account: AccountId, _at: Micros) -> BillingResult<'_, bool> {
                let d = self.d();
                if d.billing_erasures.contains_key(&account) {
                    return done(Err(StoreError::AccountErased(account)));
                }
                let new = d.accounts.insert(account);
                done(Ok(new))
            }
            fn lock_account(&mut self, account: AccountId) -> BillingResult<'_, ()> {
                let d = self.d();
                // Like the PostgreSQL adapter: an opened erasure fences ordinary billing.
                done(if d.billing_erasures.contains_key(&account) {
                    Err(StoreError::AccountErased(account))
                } else if d.accounts.contains(&account) {
                    Ok(())
                } else {
                    Err(StoreError::AccountMissing(account))
                })
            }
            fn account_created_at(&mut self, account: AccountId) -> BillingResult<'_, Option<Micros>> {
                let exists = self.d().accounts.contains(&account);
                done(Ok(exists.then_some(Micros(0))))
            }
            fn packages(&mut self, account: AccountId) -> BillingResult<'_, Vec<PackageBalance>> {
                if self.d().ai.fail_packages {
                    return done(Err(StoreError::Unavailable("plan lookup failed".into())));
                }
                let b = balances(self.d(), account);
                done(Ok(b))
            }
            fn insert_package<'a>(&'a mut self, _p: &'a billing_core::model::NewPackage) -> BillingResult<'a, i64> {
                unused!()
            }
            fn insert_usage<'a>(&'a mut self, rows: &'a [NewUsage]) -> BillingResult<'a, ()> {
                self.d().usage.extend_from_slice(rows);
                done(Ok(()))
            }
            fn release_subject(&mut self, account: AccountId, subject: SubjectRef) -> BillingResult<'_, u64> {
                let d = self.d();
                let before = d.usage.len();
                d.usage.retain(|u| !(u.account == account && u.subject == Some(subject)));
                let n = (before - d.usage.len()) as u64;
                done(Ok(n))
            }
            fn imported_release_owners(&mut self, account: AccountId, _subject: SubjectRef) -> BillingResult<'_, Vec<AccountId>> {
                done(Ok(vec![account]))
            }
            fn insert_payment<'a>(&'a mut self, _p: &'a billing_core::model::NewPayment) -> BillingResult<'a, billing_core::model::PaymentInsert> {
                unused!()
            }
            fn payment(&mut self, _id: i64, _lock: bool) -> BillingResult<'_, Option<billing_core::model::Payment>> {
                unused!()
            }
            fn payment_by_token<'a>(&'a mut self, _g: &'a str, _e: &'a str, _t: &'a str, _l: bool) -> BillingResult<'a, Option<billing_core::model::Payment>> {
                unused!()
            }
            fn payment_by_order_ref<'a>(&'a mut self, _g: &'a str, _e: &'a str, _o: &'a str, _l: bool) -> BillingResult<'a, Option<billing_core::model::Payment>> {
                unused!()
            }
            fn set_payment_status(
                &mut self,
                _id: i64,
                _from: billing_core::grants::PaymentStatus,
                _to: billing_core::grants::PaymentStatus,
            ) -> BillingResult<'_, bool> {
                unused!()
            }
            fn update_payment<'a>(&'a mut self, _id: i64, _u: &'a billing_core::model::PaymentUpdate) -> BillingResult<'a, ()> {
                unused!()
            }
            fn stamp_cancellation(&mut self, _id: i64, _at: Micros) -> BillingResult<'_, ()> {
                unused!()
            }
            fn completed_payments(&mut self, _a: AccountId) -> BillingResult<'_, Vec<billing_core::model::Payment>> {
                unused!()
            }
            fn insert_subscription<'a>(&'a mut self, _s: &'a billing_core::model::NewSubscription) -> BillingResult<'a, billing_core::model::Subscription> {
                unused!()
            }
            fn subscription<'a>(&'a mut self, _g: &'a str, _e: &'a str, _s: &'a str, _l: bool) -> BillingResult<'a, Option<billing_core::model::Subscription>> {
                unused!()
            }
            fn set_first_order_ref<'a>(&'a mut self, _id: i64, _o: &'a str, _a: &'a str, _r: &'a str) -> BillingResult<'a, bool> {
                unused!()
            }
            fn stamp_subscription_cancel<'a>(&'a mut self, _g: &'a str, _e: &'a str, _s: &'a str, _at: Micros) -> BillingResult<'a, ()> {
                unused!()
            }
            fn hold_notification<'a>(
                &'a mut self,
                _n: &'a billing_core::model::NewUnresolved,
            ) -> BillingResult<'a, (billing_core::model::UnresolvedNotification, bool)> {
                unused!()
            }
            fn unresolved(&mut self, _id: i64, _lock: bool) -> BillingResult<'_, Option<billing_core::model::UnresolvedNotification>> {
                unused!()
            }
            fn list_unresolved<'a>(&'a mut self, _s: Option<&'a str>) -> BillingResult<'a, Vec<billing_core::model::UnresolvedNotification>> {
                unused!()
            }
            fn mark_unresolved<'a>(&'a mut self, _id: i64, _s: &'a str, _p: Option<i64>, _at: Micros) -> BillingResult<'a, ()> {
                unused!()
            }
            fn coupon<'a>(&'a mut self, _id: &'a str) -> BillingResult<'a, Option<billing_core::pricing::Coupon>> {
                unused!()
            }
            fn latest_billing_info(&mut self, _a: AccountId) -> BillingResult<'_, Option<billing_core::model::BillingInfo>> {
                unused!()
            }
            fn emit<'a>(&'a mut self, _e: &'a billing_core::model::BillingEvent, _at: Micros) -> BillingResult<'a, ()> {
                unused!()
            }
            fn lock_account_for_erasure(&mut self, account: AccountId) -> BillingResult<'_, ()> {
                let present = self.d().accounts.contains(&account);
                done(if present { Ok(()) } else { Err(StoreError::AccountMissing(account)) })
            }
            fn open_erasure(&mut self, account: AccountId, at: Micros) -> BillingResult<'_, ()> {
                let d = self.d();
                if !d.billing_erasures.contains_key(&account) {
                    let pending = d.bound_subscriptions.iter().filter(|(a, _)| *a == account).map(|(_, t)| t.clone()).collect();
                    d.billing_erasures.insert(
                        account,
                        ErasureStatus { started_at: at, billing_erased_at: None, pending, confirmed: vec![], unbound_plan_payments: vec![] },
                    );
                    // The account row is the lock anchor, also for an unprovisioned account.
                    d.accounts.insert(account);
                }
                done(Ok(()))
            }
            fn erasure_status(&mut self, account: AccountId) -> BillingResult<'_, Option<ErasureStatus>> {
                let s = self.d().billing_erasures.get(&account).cloned();
                done(Ok(s))
            }
            fn record_termination<'a>(
                &'a mut self,
                account: AccountId,
                target: &'a TerminationTarget,
                confirmed: bool,
                _outcome: &'a str,
                _at: Micros,
            ) -> BillingResult<'a, ()> {
                let d = self.d();
                d.termination_attempts += 1;
                if confirmed && let Some(status) = d.billing_erasures.get_mut(&account)
                    && let Some(pos) = status.pending.iter().position(|t| t == target)
                {
                    let t = status.pending.remove(pos);
                    status.confirmed.push(t);
                }
                done(Ok(()))
            }
            fn adopt_erasure_target<'a>(&'a mut self, _account: AccountId, _target: &'a TerminationTarget, _at: Micros) -> BillingResult<'a, ()> {
                unused!()
            }
            fn resolve_erasure_checkout<'a>(
                &'a mut self,
                _account: AccountId,
                _payment_id: i64,
                _closure: &'a CheckoutClosure,
                _at: Micros,
            ) -> BillingResult<'a, bool> {
                unused!()
            }
            fn erasure_owners<'a>(&'a mut self, account: AccountId, _subjects: &'a [SubjectRef]) -> BillingResult<'a, Vec<AccountId>> {
                done(Ok(vec![account]))
            }
            fn erase_account_rows<'a>(&'a mut self, account: AccountId, subjects: &'a [SubjectRef], at: Micros) -> BillingResult<'a, ErasureCounts> {
                let d = self.d();
                let packages: Vec<i64> = d.packages.iter().filter(|p| p.account == account).map(|p| p.id).collect();
                let before = d.usage.len();
                d.usage.retain(|u| u.account != account && !u.subject.is_some_and(|s| subjects.contains(&s)) && !packages.contains(&u.package_id));
                let runtime_usage = (before - d.usage.len()) as u64;
                d.packages.retain(|p| p.account != account);
                d.accounts.remove(&account);
                if let Some(s) = d.billing_erasures.get_mut(&account) {
                    s.billing_erased_at = Some(at);
                }
                d.billing_erased.push(account);
                done(Ok(ErasureCounts { runtime_usage, packages: packages.len() as u64, ..ErasureCounts::default() }))
            }
        }
    };
}

billing_tx!(MemBilling<'_>);
billing_tx!(MemBillingUow);

impl UnitOfWork for MemBillingUow {
    fn tx(&mut self) -> &mut dyn BillingTx {
        self
    }
    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        let MemBillingUow { data, store } = *self;
        *store.lock().unwrap() = data;
        Box::pin(async { Ok(()) })
    }
    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Billing's own store over the same in-memory database.
#[derive(Clone)]
pub struct MemBillingStore {
    pub data: Arc<Mutex<Data>>,
}

impl BillingStore for MemBillingStore {
    fn begin(&self) -> BillingResult<'_, Box<dyn UnitOfWork>> {
        let data = self.data.lock().unwrap().clone();
        done(Ok(Box::new(MemBillingUow { data, store: self.data.clone() }) as Box<dyn UnitOfWork>))
    }
    fn ping(&self) -> BillingResult<'_, ()> {
        done(Ok(()))
    }
    fn claim_outbox(&self, _limit: i64, _lease: i64, _now: Micros) -> BillingResult<'_, Vec<billing_core::model::OutboxEvent>> {
        unused!()
    }
    fn delivered_targets(&self, _event: uuid::Uuid) -> BillingResult<'_, Vec<String>> {
        unused!()
    }
    fn record_delivery<'a>(&'a self, _event: uuid::Uuid, _target: &'a str, _at: Micros) -> BillingResult<'a, ()> {
        unused!()
    }
    fn ack_outbox<'a>(&'a self, _event: &'a billing_core::model::OutboxEvent, _at: Micros) -> BillingResult<'a, bool> {
        unused!()
    }
    fn release_outbox<'a>(&'a self, _event: &'a billing_core::model::OutboxEvent, _error: &'a str, _retry: Micros) -> BillingResult<'a, ()> {
        unused!()
    }
}

/// Erasure-only termination with a scripted answer.
pub struct FakeTerminator {
    pub evidence: Mutex<Result<TerminationEvidence, GatewayError>>,
}

impl SubscriptionTermination for FakeTerminator {
    fn gateway(&self) -> &str {
        "iyzico"
    }
    fn environment(&self) -> &str {
        "sandbox"
    }
    fn terminate<'a>(&'a self, _target: &'a TerminationTarget) -> BoxFuture<'a, Result<TerminationEvidence, GatewayError>> {
        let v = self.evidence.lock().unwrap().clone();
        Box::pin(async move { v })
    }
}

impl JobUow for MemUow {
    fn ai(&mut self) -> &mut dyn AiTx {
        self
    }
    fn account(&mut self) -> &mut dyn AccountTx {
        self
    }
    fn api(&mut self) -> &mut dyn crate::api::ports::ApiTx {
        self
    }
    fn billing(&mut self) -> BoxFuture<'_, Result<Box<dyn BillingTx + '_>, StoreError>> {
        let b: Box<dyn BillingTx + '_> = Box::new(MemBilling(&mut self.data));
        Box::pin(async move { Ok(b) })
    }
    fn lock_capacity<'a>(&'a mut self, _p: media_ports::Provider, _project: &'a str) -> StoreResult<'a, ()> {
        ok(())
    }
    fn inflight<'a>(&'a mut self, provider: media_ports::Provider, _project: &'a str, now: Micros) -> StoreResult<'a, u32> {
        let n = self
            .data
            .attempts
            .values()
            .filter(|a| a.provider.transport() == Some(provider) && a.is_live(now))
            .filter(|a| self.data.jobs.get(&a.job.0).is_some_and(|j| matches!(j.status, JobStatus::NotStarted | JobStatus::Pending | JobStatus::Running)))
            .count() as u32;
        ok(n)
    }
    fn lock_jobs<'a>(&'a mut self, account: AccountId, ids: &'a [JobId]) -> StoreResult<'a, Vec<Job>> {
        let mut ids: Vec<JobId> = ids.to_vec();
        ids.sort();
        ids.dedup();
        ok(ids.iter().filter_map(|id| self.data.jobs.get(&id.0)).filter(|j| j.account == account).cloned().collect())
    }
    fn lock_job(&mut self, id: JobId) -> StoreResult<'_, Option<Job>> {
        ok(self.data.jobs.get(&id.0).cloned())
    }
    fn completed_job_count(&mut self, account: AccountId) -> StoreResult<'_, u64> {
        ok(self.data.jobs.values().filter(|j| j.account == account && j.status == JobStatus::Completed).count() as u64)
    }
    fn insert_job<'a>(&'a mut self, new: &'a NewJob) -> StoreResult<'a, Job> {
        self.data.next_job += 1;
        let job = Job {
            id: JobId(self.data.next_job),
            account: new.account,
            revision: 1,
            status: new.status,
            name: new.name.clone(),
            source: new.source.clone(),
            language: new.language,
            translate_to: new.translate_to.clone(),
            speakers: new.speakers,
            fix_grammer: new.fix_grammer,
            duration_minutes: new.duration_minutes,
            size_bytes: new.size_bytes,
            provider: None,
            model: None,
            callback_url: new.callback_url.clone(),
            whisper_params: new.whisper_params.clone(),
            error: None,
            retry_count: 0,
            recovery_attempts: 0,
            soniox_transcription_id: None,
            started_at: None,
            created_at: new.created_at,
            updated_at: new.created_at,
            current_attempt: None,
            attempts_made: 0,
            deferred_charge: crate::jobs::domain::DeferredCharge::None,
            next_dispatch_at: None,
        };
        self.data.jobs.insert(job.id.0, job.clone());
        ok(job)
    }
    fn save_job<'a>(&'a mut self, job: &'a Job, expected: i64) -> StoreResult<'a, i64> {
        let current = self.data.jobs.get(&job.id.0).map(|j| j.revision);
        if current != Some(expected) {
            return Box::pin(async { Err(StoreError::Conflict("revision".into())) });
        }
        let mut saved = job.clone();
        saved.revision = expected + 1;
        self.data.jobs.insert(job.id.0, saved);
        ok(expected + 1)
    }
    fn record_issued_upload<'a>(&'a mut self, u: &'a IssuedUpload) -> StoreResult<'a, ()> {
        self.data.uploads.push(u.clone());
        ok(())
    }
    fn record_archived_payload<'a>(&'a mut self, p: &'a ArchivedPayload) -> StoreResult<'a, ()> {
        self.data.archived.push(p.clone());
        ok(())
    }
    fn settle_archived_payload<'a>(&'a mut self, account: AccountId, object: &'a ObjectRef, write: ArchiveWrite) -> StoreResult<'a, ()> {
        if let Some(p) = self.data.archived.iter_mut().find(|p| p.account == account && p.object == *object && p.write == ArchiveWrite::InFlight) {
            p.write = write;
        }
        ok(())
    }
    fn hard_delete_job(&mut self, id: JobId) -> StoreResult<'_, ()> {
        self.data.lines.retain(|l| l.job != id);
        self.data.jobs.remove(&id.0);
        let ai = &mut self.data.ai;
        ai.revisions.remove(&id.0);
        ai.generations.retain(|g| g.job != id);
        ai.counters.retain(|c| c.key.scope != CounterScope::Transcript(id));
        ai.chat.retain(|m| m.job != id);
        ai.summaries.retain(|s| s.job != id);
        ai.summary_requests.retain(|r| r.job != id);
        self.data.feedback.retain(|f| f.job != id);
        ok(())
    }
    fn insert_attempt<'a>(&'a mut self, a: &'a Attempt) -> StoreResult<'a, ()> {
        self.data.attempts.insert(a.id, a.clone());
        ok(())
    }
    fn save_attempt<'a>(&'a mut self, a: &'a Attempt) -> StoreResult<'a, ()> {
        self.data.attempts.insert(a.id, a.clone());
        ok(())
    }
    fn lock_attempt(&mut self, id: AttemptId) -> StoreResult<'_, Option<Attempt>> {
        ok(self.data.attempts.get(&id).cloned())
    }
    fn attempts_of(&mut self, job: JobId) -> StoreResult<'_, Vec<Attempt>> {
        ok(self.data.attempts.values().filter(|a| a.job == job).cloned().collect())
    }
    fn insert_lines<'a>(&'a mut self, job: &'a Job, lines: &'a [NewLine]) -> StoreResult<'a, u64> {
        if self.data.fail_line_insert {
            return Box::pin(async { Err(StoreError::Conflict("invalid input value for enum".into())) });
        }
        for l in lines {
            self.data.next_line += 1;
            self.data.lines.push(Line {
                id: LineId(self.data.next_line),
                job: job.id,
                account: job.account,
                start_time: l.start_time.clone(),
                end_time: l.end_time.clone(),
                line: l.line.clone(),
                speaker: l.speaker.clone(),
                corrected_text: l.corrected_text.clone(),
                split_on_speaker_change: l.split_on_speaker_change,
                language: l.language,
                translation_status: l.translation_status,
                created_at: Micros(0),
            });
        }
        ok(lines.len() as u64)
    }
    fn lock_line(&mut self, id: LineId) -> StoreResult<'_, Option<Line>> {
        ok(self.data.lines.iter().find(|l| l.id == id).cloned())
    }
    fn update_line_text<'a>(&'a mut self, id: LineId, text: &'a str) -> StoreResult<'a, bool> {
        let found = self.data.lines.iter_mut().find(|l| l.id == id).map(|l| l.line = text.to_string()).is_some();
        ok(found)
    }
    fn update_line_speaker<'a>(&'a mut self, id: LineId, speaker: &'a str) -> StoreResult<'a, bool> {
        let found = self.data.lines.iter_mut().find(|l| l.id == id).map(|l| l.speaker = Some(speaker.to_string())).is_some();
        ok(found)
    }
    fn rename_speaker<'a>(&'a mut self, job: JobId, from: &'a str, to: &'a str) -> StoreResult<'a, u64> {
        let mut n = 0;
        for l in self.data.lines.iter_mut().filter(|l| l.job == job && l.speaker.as_deref() == Some(from)) {
            l.speaker = Some(to.to_string());
            n += 1;
        }
        ok(n)
    }
    fn insert_inbox<'a>(&'a mut self, e: &'a NewInboxEntry) -> StoreResult<'a, (InboxEntry, bool)> {
        if let Some(existing) = self.data.inbox.values().find(|x| x.entry.attempt == e.attempt && x.entry.remote_id == e.remote_id) {
            return ok((existing.clone(), false));
        }
        self.data.next_inbox += 1;
        let stored = InboxEntry { id: InboxId(self.data.next_inbox), entry: e.clone(), state: InboxState::Pending, settled_at: None, note: None, failures: 0 };
        self.data.inbox.insert(stored.id.0, stored.clone());
        ok((stored, true))
    }
    fn hold_inbox_conflict<'a>(&'a mut self, of: InboxId, e: &'a NewInboxEntry) -> StoreResult<'a, ()> {
        let note = format!("conflicts with inbox {}", of.0);
        let held = self.data.inbox.values().any(|x| {
            x.state == InboxState::Conflict && x.note.as_deref() == Some(note.as_str()) && x.entry.fingerprint == e.fingerprint
        });
        if !held {
            self.data.next_inbox += 1;
            let id = InboxId(self.data.next_inbox);
            let entry = InboxEntry { id, entry: e.clone(), state: InboxState::Conflict, settled_at: Some(e.received_at), note: Some(note), failures: 0 };
            self.data.inbox.insert(id.0, entry);
        }
        ok(())
    }
    fn lock_inbox(&mut self, id: InboxId) -> StoreResult<'_, Option<InboxEntry>> {
        ok(self.data.inbox.get(&id.0).cloned())
    }
    fn settle_inbox<'a>(&'a mut self, id: InboxId, state: InboxState, note: Option<&'a str>, at: Micros) -> StoreResult<'a, ()> {
        if let Some(e) = self.data.inbox.get_mut(&id.0) {
            e.state = state;
            e.note = note.map(str::to_string);
            e.settled_at = Some(at);
        }
        ok(())
    }
    fn record_inbox_failure<'a>(&'a mut self, id: InboxId, _e: &'a str) -> StoreResult<'a, ()> {
        if let Some(e) = self.data.inbox.get_mut(&id.0) {
            e.failures += 1;
        }
        ok(())
    }
    fn emit<'a>(&'a mut self, event: &'a JobEvent) -> StoreResult<'a, ()> {
        self.data.events.push(event.clone());
        ok(())
    }
    fn lock_settings(&mut self, account: AccountId) -> StoreResult<'_, UserSettings> {
        ok(self.data.settings.get(&account).cloned().unwrap_or_else(|| UserSettings::defaults(account)))
    }
    fn save_settings<'a>(&'a mut self, s: &'a UserSettings) -> StoreResult<'a, ()> {
        self.data.settings.insert(s.account, s.clone());
        ok(())
    }
    fn retention_candidates(&mut self, cutoff: Micros) -> StoreResult<'_, Vec<RetentionCandidate>> {
        let d = &self.data;
        let c = d
            .jobs
            .values()
            .filter(|j| j.status == JobStatus::Completed && j.created_at.0 <= cutoff.0)
            .map(|j| RetentionCandidate {
                account: j.account,
                job: j.id,
                // NOT NULL DEFAULT true (PRV-013): a missing settings row behaves as true.
                auto_delete: d.settings.get(&j.account).map(|s| s.auto_delete_transcripts).unwrap_or(true),
                line_count: d.lines.iter().filter(|l| l.job == j.id).count() as u64,
            })
            .filter(|c| c.line_count > 0)
            .collect();
        ok(c)
    }
    fn lock_retention_set<'a>(&'a mut self, candidates: &'a [RetentionCandidate], cutoff: Micros) -> StoreResult<'a, Vec<RetentionCandidate>> {
        let ids: BTreeSet<JobId> = candidates.iter().map(|c| c.job).collect();
        let d = &self.data;
        let c = d
            .jobs
            .values()
            .filter(|j| ids.contains(&j.id) && j.status == JobStatus::Completed && j.created_at.0 <= cutoff.0)
            .map(|j| RetentionCandidate {
                account: j.account,
                job: j.id,
                auto_delete: d.settings.get(&j.account).map(|s| s.auto_delete_transcripts).unwrap_or(true),
                line_count: d.lines.iter().filter(|l| l.job == j.id).count() as u64,
            })
            .collect();
        ok(c)
    }
    fn delete_lines_of<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, u64> {
        let before = self.data.lines.len();
        self.data.lines.retain(|l| !jobs.contains(&l.job));
        // P7 D10: callback bodies copied the lines.
        self.data.api.deliveries.iter_mut().filter(|d| jobs.contains(&d.job)).for_each(|d| d.payload = None);
        ok((before - self.data.lines.len()) as u64)
    }
    fn opted_out_count(&mut self, cutoff: Micros) -> StoreResult<'_, u64> {
        let d = &self.data;
        let accounts: BTreeSet<AccountId> = d
            .jobs
            .values()
            .filter(|j| j.status == JobStatus::Completed && j.created_at.0 <= cutoff.0)
            .filter(|j| d.lines.iter().any(|l| l.job == j.id))
            .filter(|j| !d.settings.get(&j.account).map(|s| s.auto_delete_transcripts).unwrap_or(true))
            .map(|j| j.account)
            .collect();
        ok(accounts.len() as u64)
    }
    fn insert_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()> {
        self.data.runs.push(run.clone());
        ok(())
    }
    fn save_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()> {
        match self.data.runs.iter_mut().find(|r| r.id == run.id) {
            Some(r) => *r = run.clone(),
            None => self.data.runs.push(run.clone()),
        }
        ok(())
    }
    fn fail_interrupted_runs<'a>(&'a mut self, job_name: &'a str, message: &'a str, _at: Micros) -> StoreResult<'a, u64> {
        let mut n = 0;
        for r in self.data.runs.iter_mut().filter(|r| r.job_name == job_name && r.status == RunStatus::Running) {
            r.status = RunStatus::Failure;
            r.rows_deleted = 0;
            r.error_message = Some(message.to_string());
            n += 1;
        }
        ok(n)
    }
    fn history_count<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64> {
        let n = self.data.schedule_runs.iter().filter(|r| r.finished && schedules.contains(&r.schedule.as_str()) && r.started_at.0 <= cutoff.0).count();
        ok(n as u64)
    }
    fn delete_history<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64> {
        let before = self.data.schedule_runs.len();
        self.data
            .schedule_runs
            .retain(|r| !(r.finished && schedules.contains(&r.schedule.as_str()) && r.started_at.0 <= cutoff.0));
        ok((before - self.data.schedule_runs.len()) as u64)
    }
    fn lock_barrier(&mut self, account: AccountId) -> StoreResult<'_, Option<ErasureRecord>> {
        ok(self.data.erasures.values().find(|e| e.account == account).cloned())
    }
    fn insert_erasure<'a>(&'a mut self, r: &'a ErasureRecord) -> StoreResult<'a, ()> {
        self.data.erasures.insert(r.id, r.clone());
        ok(())
    }
    fn insert_erasure_items<'a>(&'a mut self, items: &'a [ErasureItem]) -> StoreResult<'a, ()> {
        self.data.items.extend_from_slice(items);
        ok(())
    }
    fn lock_erasure(&mut self, id: ErasureId) -> StoreResult<'_, Option<ErasureRecord>> {
        ok(self.data.erasures.get(&id).cloned())
    }
    fn save_erasure<'a>(&'a mut self, r: &'a ErasureRecord) -> StoreResult<'a, ()> {
        self.data.erasures.insert(r.id, r.clone());
        ok(())
    }
    fn erasure_items(&mut self, id: ErasureId) -> StoreResult<'_, Vec<ErasureItem>> {
        ok(self.data.items.iter().filter(|i| i.erasure == id).cloned().collect())
    }
    fn save_erasure_item<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()> {
        // Like the adapter: only state and attempts change; the target is immutable here.
        if let Some(i) = self.data.items.iter_mut().find(|i| i.id == item.id) {
            i.state = item.state.clone();
            i.attempts = item.attempts;
        }
        ok(())
    }
    fn save_erasure_item_target<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()> {
        let llm = |t: &ItemTarget| matches!(t, ItemTarget::LlmDispatch { .. });
        match self.data.items.iter_mut().find(|i| i.id == item.id) {
            Some(i) if llm(&i.target) && llm(&item.target) => {
                *i = item.clone();
                ok(())
            }
            _ => conflict("not an llm dispatch item"),
        }
    }
    fn account_resources(&mut self, account: AccountId) -> StoreResult<'_, AccountResources> {
        let d = &self.data;
        let jobs: Vec<JobId> = d.jobs.values().filter(|j| j.account == account).map(|j| j.id).collect();
        let media = d
            .jobs
            .values()
            .filter(|j| j.account == account)
            .filter_map(|j| match &j.source {
                crate::jobs::domain::JobSource::Upload { object: Some(o), .. } => Some(o.clone()),
                _ => None,
            })
            .collect();
        let provider_copies = d.attempts.values().filter(|a| a.account == account).filter_map(|a| Some((a.provider, a.remote()?))).collect();
        ok(AccountResources {
            jobs,
            media,
            archived_payloads: d.archived.iter().filter(|p| p.account == account).cloned().collect(),
            provider_copies,
            unresolved_submissions: vec![],
            issued_uploads: d.uploads.iter().filter(|u| u.account == account).cloned().collect(),
            llm_dispatches: d.ai.dispatches.iter().filter(|x| x.account == account).cloned().collect(),
        })
    }
    fn erase_account_rows(&mut self, account: AccountId) -> StoreResult<'_, ()> {
        let ids: Vec<JobId> = self.data.jobs.values().filter(|j| j.account == account).map(|j| j.id).collect();
        self.data.lines.retain(|l| !ids.contains(&l.job));
        self.data.attempts.retain(|_, a| a.account != account);
        self.data.inbox.retain(|_, e| e.entry.account != account);
        self.data.jobs.retain(|_, j| j.account != account);
        self.data.settings.remove(&account);
        let ai = &mut self.data.ai;
        ai.consents.remove(&account);
        ai.revisions.retain(|job, _| !ids.iter().any(|j| j.0 == *job));
        ai.generations.retain(|g| g.account != account);
        ai.counters.retain(|c| c.key.account != account);
        ai.chat.retain(|m| m.account != account);
        ai.summaries.retain(|s| s.account != account);
        ai.summary_requests.retain(|r| r.account != account);
        ai.dispatches.retain(|x| x.account != account);
        self.data.feedback.retain(|f| f.account != account);
        self.data.support.retain(|s| s.request.account != account);
        let api = &mut self.data.api;
        api.keys.retain(|k| k.account != account);
        api.idempotency.retain(|r| r.account != account);
        api.deliveries.retain(|d| d.account != account);
        ok(())
    }
    fn savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        self.savepoints.push((name.to_string(), self.data.clone()));
        ok(())
    }
    fn rollback_to_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        if let Some(pos) = self.savepoints.iter().rposition(|(n, _)| n == name) {
            self.data = self.savepoints[pos].1.clone();
            self.savepoints.truncate(pos);
        }
        ok(())
    }
    fn release_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()> {
        if let Some(pos) = self.savepoints.iter().rposition(|(n, _)| n == name) {
            self.savepoints.truncate(pos);
        }
        ok(())
    }
    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        let MemUow { data, store, .. } = *self;
        *store.lock().unwrap() = data;
        Box::pin(async { Ok(()) })
    }
    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        Box::pin(async { Ok(()) })
    }
}

pub struct TestClock(pub Mutex<Micros>);

impl TestClock {
    pub fn advance_minutes(&self, minutes: i64) {
        let mut t = self.0.lock().unwrap();
        t.0 += minutes * crate::ids::MICROS_PER_MINUTE;
    }
}

impl Clock for TestClock {
    fn now(&self) -> Micros {
        *self.0.lock().unwrap()
    }
}

/// A scripted transcriber: `submissions` answers each submit in order.
pub struct FakeTranscriber {
    pub provider: media_ports::Provider,
    pub submissions: Mutex<VecDeque<Result<Submission, TransportError>>>,
    pub submitted: Mutex<Vec<TranscriptionRequest>>,
    pub status: Mutex<Option<ProviderObservation>>,
    pub result: Mutex<Option<ProviderObservation>>,
    pub completed: Mutex<Option<ProviderObservation>>,
    pub records: Mutex<Vec<RemoteRecord>>,
    /// When set, `find_by_reference` fails with it (the listing is down).
    pub listing_error: Mutex<Option<TransportError>>,
    pub deleted: Mutex<Vec<RemoteIdentity>>,
    pub delete_outcome: Mutex<DeleteOutcome>,
}

impl FakeTranscriber {
    pub fn new(provider: media_ports::Provider) -> FakeTranscriber {
        FakeTranscriber {
            provider,
            submissions: Mutex::new(VecDeque::new()),
            submitted: Mutex::new(vec![]),
            status: Mutex::new(None),
            result: Mutex::new(None),
            completed: Mutex::new(None),
            records: Mutex::new(vec![]),
            listing_error: Mutex::new(None),
            deleted: Mutex::new(vec![]),
            delete_outcome: Mutex::new(DeleteOutcome::Deleted),
        }
    }

    pub fn answer(&self, r: Result<Submission, TransportError>) {
        self.submissions.lock().unwrap().push_back(r);
    }

    pub fn accept(&self, id: &str) {
        self.answer(Ok(Submission::Accepted {
            remote: RemoteIdentity { provider: self.provider, project: "project".into(), id: id.into() },
        }));
    }
}

impl Transcriber for FakeTranscriber {
    fn provider(&self) -> media_ports::Provider {
        self.provider
    }
    fn project(&self) -> &str {
        "project"
    }
    fn submit<'a>(&'a self, request: &'a TranscriptionRequest) -> BoxFuture<'a, Result<Submission, TransportError>> {
        self.submitted.lock().unwrap().push(request.clone());
        let next = self.submissions.lock().unwrap().pop_front().unwrap_or(Err(TransportError::Unavailable { status: None, operation: "unscripted".into() }));
        Box::pin(async move { next })
    }
    fn status<'a>(&'a self, _r: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        let v = self.status.lock().unwrap().clone().ok_or(TransportError::Unsupported("status".into()));
        Box::pin(async move { v })
    }
    fn result<'a>(&'a self, _r: &'a RemoteIdentity) -> BoxFuture<'a, Result<ProviderObservation, TransportError>> {
        let v = self.result.lock().unwrap().clone().ok_or(TransportError::Unsupported("result".into()));
        Box::pin(async move { v })
    }
    fn delete_remote<'a>(&'a self, r: &'a RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        self.deleted.lock().unwrap().push(r.clone());
        let v = self.delete_outcome.lock().unwrap().clone();
        Box::pin(async move { Ok(v) })
    }
    fn find_by_reference<'a>(&'a self, _reference: &'a str) -> BoxFuture<'a, Result<Vec<RemoteRecord>, TransportError>> {
        let v = match self.listing_error.lock().unwrap().clone() {
            Some(e) => Err(e),
            None => Ok(self.records.lock().unwrap().clone()),
        };
        Box::pin(async move { v })
    }
    fn verify_callback(&self, _b: &CallbackBinding, _h: &Headers, _body: &[u8]) -> Result<ProviderObservation, CallbackError> {
        Err(CallbackError::Malformed)
    }
    fn complete_callback(&self, observation: ProviderObservation) -> BoxFuture<'_, Result<ProviderObservation, TransportError>> {
        let v = self.completed.lock().unwrap().clone().map(|mut c| {
            // Like the real adapter: the reported webhook part stays verbatim (D6).
            if let (media_ports::ProviderPayload::Soniox { reported, .. }, media_ports::ProviderPayload::Soniox { reported: original, .. }) =
                (&mut c.payload, &observation.payload)
            {
                *reported = original.clone();
            }
            c.origin = ObservationOrigin::Webhook;
            c
        });
        Box::pin(async move { v.ok_or(TransportError::Unavailable { status: None, operation: "transcript".into() }) })
    }
}

#[derive(Default)]
pub struct FakeObjects {
    pub kind: Option<StoreKind>,
    /// `bucket` when unset.
    pub bucket_name: Option<&'static str>,
    pub present: Mutex<BTreeSet<String>>,
    pub deleted: Mutex<Vec<String>>,
    pub delete_outcome: Mutex<Option<DeleteOutcome>>,
}

impl ObjectStore for FakeObjects {
    fn kind(&self) -> StoreKind {
        self.kind.unwrap_or(StoreKind::Gcs)
    }
    fn bucket(&self) -> &str {
        self.bucket_name.unwrap_or("bucket")
    }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: media_ports::Micros, ttl: u32)
        -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        let access = TemporaryAccess {
            object: object.clone(),
            method,
            url: SensitiveUrl::new(format!("https://storage.test/{}", object.key)),
            expires_at: media_ports::Micros(now.0 + i64::from(ttl) * 1_000_000),
            headers: vec![],
        };
        Box::pin(async move { Ok(access) })
    }
    fn head<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        let present = self.present.lock().unwrap().contains(&object.key);
        let meta = present.then(|| ObjectMetadata { object: object.clone(), size: 1, content_type: None, etag: None });
        Box::pin(async move { Ok(meta) })
    }
    fn put(&self, request: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        self.present.lock().unwrap().insert(request.object.key.clone());
        let meta = ObjectMetadata { object: request.object, size: request.content_length, content_type: Some(request.content_type), etag: None };
        Box::pin(async move { Ok(meta) })
    }
    fn delete<'a>(&'a self, object: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        self.deleted.lock().unwrap().push(object.key.clone());
        let outcome = self.delete_outcome.lock().unwrap().clone().unwrap_or(DeleteOutcome::Deleted);
        self.present.lock().unwrap().remove(&object.key);
        Box::pin(async move { Ok(outcome) })
    }
}

/// A legacy prefix inventory over the objects present in `objects` (D2/F9): it lists
/// every present key under `{owner}/`, so deletions show up in the next listing.
pub struct FakeInventory {
    pub objects: Arc<FakeObjects>,
    /// When set, `inventory` fails with it instead of returning a shorter list.
    pub error: Mutex<Option<TransportError>>,
    pub owners: Mutex<Vec<String>>,
}

impl FakeInventory {
    pub fn over(objects: Arc<FakeObjects>) -> FakeInventory {
        FakeInventory { objects, error: Mutex::new(None), owners: Mutex::new(vec![]) }
    }
}

impl media_ports::ObjectInventory for FakeInventory {
    fn kind(&self) -> StoreKind {
        self.objects.kind()
    }
    fn bucket(&self) -> &str {
        self.objects.bucket()
    }
    fn inventory<'a>(&'a self, owner: &'a str) -> BoxFuture<'a, Result<Vec<ObjectRef>, TransportError>> {
        self.owners.lock().unwrap().push(owner.to_string());
        let answer = match self.error.lock().unwrap().clone() {
            Some(e) => Err(e),
            None => {
                let prefix = format!("{owner}/");
                let present = self.objects.present.lock().unwrap();
                Ok(present
                    .iter()
                    .filter(|k| k.starts_with(&prefix))
                    .map(|k| ObjectRef { store: self.kind(), bucket: self.bucket().to_string(), key: k.clone(), version: None })
                    .collect())
            }
        };
        Box::pin(async move { answer })
    }
}

pub struct NoSleep;

impl Sleeper for NoSleep {
    fn sleep(&self, _millis: u64) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

pub struct CountingLimiter {
    pub hits: Mutex<BTreeMap<String, u32>>,
}

impl RateLimiter for CountingLimiter {
    fn hit<'a>(&'a self, key: &'a str, limit: u32, window_ms: i64, now: Micros) -> BoxFuture<'a, Result<RateDecision, StoreError>> {
        let mut hits = self.hits.lock().unwrap();
        let count = hits.entry(key.to_string()).or_insert(0);
        let allowed = *count < limit;
        if allowed {
            *count += 1;
        }
        let decision = RateDecision { allowed, limit, remaining: limit - *count, reset_at: Micros(now.0 + window_ms * 1000) };
        Box::pin(async move { Ok(decision) })
    }
}

pub struct TestLocks {
    pub held: Arc<Mutex<BTreeSet<String>>>,
}

struct TestGuard(Arc<Mutex<BTreeSet<String>>>, String);

impl LockGuard for TestGuard {
    fn release(self: Box<Self>) -> BoxFuture<'static, ()> {
        self.0.lock().unwrap().remove(&self.1);
        Box::pin(async {})
    }
}

impl Locks for TestLocks {
    fn try_lock<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<Box<dyn LockGuard>>, StoreError>> {
        let acquired = self.held.lock().unwrap().insert(name.to_string());
        let guard: Option<Box<dyn LockGuard>> = acquired.then(|| Box::new(TestGuard(self.held.clone(), name.to_string())) as Box<dyn LockGuard>);
        Box::pin(async move { Ok(guard) })
    }
}

pub struct FakeIdentity {
    pub outcome: Mutex<Result<IdentityOutcome, String>>,
    pub calls: Mutex<u32>,
}

impl IdentityAdmin for FakeIdentity {
    fn delete_user(&self, _account: AccountId) -> BoxFuture<'_, Result<IdentityOutcome, String>> {
        *self.calls.lock().unwrap() += 1;
        let v = self.outcome.lock().unwrap().clone();
        Box::pin(async move { v })
    }
}

pub fn token(text: &str, start: f64, end: f64, speaker: Option<i64>) -> media_ports::SonioxToken {
    media_ports::SonioxToken {
        text: text.into(),
        start_ms: Some(start),
        end_ms: Some(end),
        speaker: speaker.map(|s| serde_json::json!(s)),
        language: None,
        translation_status: None,
        is_audio_event: None,
        extra: Default::default(),
    }
}

pub fn translated(text: &str, start: f64, end: f64, language: &str, status: &str) -> media_ports::SonioxToken {
    let mut t = token(text, start, end, None);
    t.language = Some(language.into());
    t.translation_status = Some(status.into());
    t
}

pub fn reported(id: &str, status: &str, duration_ms: Option<f64>) -> media_ports::SonioxReported {
    media_ports::SonioxReported {
        id: id.into(),
        status: status.into(),
        audio_duration_ms: duration_ms,
        error: None,
        error_type: None,
        error_message: None,
        extra: Default::default(),
    }
}

pub fn soniox(id: &str, origin: ObservationOrigin, status: &str, duration_ms: Option<f64>, tokens: Option<Vec<media_ports::SonioxToken>>) -> ProviderObservation {
    ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Soniox, project: "project".into(), id: id.into() },
        origin,
        payload: media_ports::ProviderPayload::Soniox {
            reported: reported(id, status, duration_ms),
            transcript: tokens.map(|tokens| media_ports::SonioxTranscript {
                id: id.into(),
                tokens,
                text: None,
                metadata: serde_json::Value::Null,
                extra: Default::default(),
            }),
        },
    }
}

pub fn deepgram(request_id: &str, duration: Option<f64>, sentences: &[(&str, f64, f64, Option<i64>)]) -> ProviderObservation {
    let paragraphs = sentences
        .iter()
        .map(|(text, start, end, speaker)| media_ports::DeepgramParagraph {
            sentences: vec![media_ports::DeepgramSentence { text: (*text).into(), start: *start, end: *end, extra: Default::default() }],
            speaker: *speaker,
            extra: Default::default(),
        })
        .collect();
    ProviderObservation {
        remote: RemoteIdentity { provider: media_ports::Provider::Deepgram, project: "project".into(), id: request_id.into() },
        origin: ObservationOrigin::Webhook,
        payload: media_ports::ProviderPayload::Deepgram(media_ports::DeepgramTranscript {
            metadata: media_ports::DeepgramMetadata { request_id: request_id.into(), duration, extra: Default::default() },
            results: media_ports::DeepgramResults {
                channels: vec![media_ports::DeepgramChannel {
                    alternatives: vec![media_ports::DeepgramAlternative {
                        transcript: String::new(),
                        words: vec![],
                        paragraphs: Some(media_ports::DeepgramParagraphs { paragraphs, extra: Default::default() }),
                        extra: Default::default(),
                    }],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
            extra: Default::default(),
        }),
    }
}
