//! Ports the job, retention and erasure use cases call. `transgate/jobs-postgres`
//! implements them; the in-memory fake in `crate::jobs::testing` keeps the use
//! cases testable without a database.
//!
//! Transaction protocol (frozen seam 5): one `JobUow` is one READ COMMITTED
//! transaction. Lock order is the provider capacity lock (submission claims
//! only), then the billing account (`billing().lock_account`), then job rows in
//! ascending id, then revision compare-and-swap on save. The opener commits or
//! rolls back; no method does network I/O.

use billing_core::ports::{BillingTx, StoreError};
use media_ports::{ObjectRef, ObservationOrigin, ProviderObservation};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::account::ports::AccountTx;
use crate::ai::GenerationId;
use crate::ai::ports::AiTx;
use crate::ids::{AccountId, AttemptId, ErasureId, InboxId, JobId, LineId, Micros};
use crate::jobs::domain::{Attempt, Job, JobStatus, NewJob};
use crate::jobs::events::JobEvent;
use crate::languages::Language;
use crate::privacy::erasure::{AccountResources, ErasureItem, ErasureRecord};
use crate::privacy::retention::{RetentionCandidate, RetentionRun};
use crate::privacy::settings::UserSettings;
use crate::transcript::{Line, NewLine};

pub use billing_core::types::BoxFuture;

pub type StoreResult<'a, T> = BoxFuture<'a, Result<T, StoreError>>;

/// What the job list asks for (JOB-039, PRV-032).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobListQuery {
    pub account: AccountId,
    pub statuses: Vec<JobStatus>,
    /// `created >= since`; `None` when the user has `unlimited_history`.
    pub since: Option<Micros>,
    /// Keyset cursor: only ids below this one (id descending).
    pub before: Option<JobId>,
    pub limit: u32,
}

/// The `user_languages` and `user_translate_to_languages` views (UPL-021):
/// how often the user picked each language, most used first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageUsage {
    pub languages: Vec<(Language, u64)>,
    pub targets: Vec<(Language, u64)>,
}

/// A durably stored, authenticated provider observation (C-6, seam 6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewInboxEntry {
    pub attempt: AttemptId,
    pub job: JobId,
    pub account: AccountId,
    pub remote_id: String,
    pub origin: ObservationOrigin,
    /// As received: Soniox's reported duration stays exactly what the webhook
    /// carried (D6); Deepgram's body survives until application.
    pub observation: ProviderObservation,
    /// Stable digest of the payload, to tell a replay from a conflicting one.
    pub fingerprint: String,
    pub received_at: Micros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxState {
    Pending,
    Applied,
    /// Same attempt and remote id but a different payload: held, never applied.
    Conflict,
    /// Authenticated but not applicable (late result, erased account).
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InboxEntry {
    pub id: InboxId,
    pub entry: NewInboxEntry,
    pub state: InboxState,
    pub settled_at: Option<Micros>,
    pub note: Option<String>,
    /// Transient ingestion failures so far (retried; never a terminal failure by themselves).
    pub failures: i32,
}

/// Read-side and worker-selection queries that need no transaction.
pub trait JobQueries: Send + Sync {
    fn list_jobs<'a>(&'a self, query: &'a JobListQuery) -> StoreResult<'a, Vec<Job>>;
    /// Owner-scoped load by id, any status or age (PRV-012, UPL-033).
    fn owned_job(&self, account: AccountId, id: JobId) -> StoreResult<'_, Option<Job>>;
    /// Lines ordered by `start_time` text then id (EDT-011).
    fn lines(&self, job: JobId) -> StoreResult<'_, Vec<Line>>;
    fn language_usage(&self, account: AccountId) -> StoreResult<'_, LanguageUsage>;
    fn settings(&self, account: AccountId) -> StoreResult<'_, UserSettings>;
    /// Jobs a dispatch pass may submit: `not_started` or `pending`, due at `now`,
    /// with no live attempt. Pending before not_started, oldest `created` first
    /// (JOB-018/019), at most `limit`.
    fn dispatch_candidates(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Job>>;
    /// Running jobs whose `started_at` is before `started_before` (JOB-020).
    fn stuck_candidates(&self, started_before: Micros, limit: u32) -> StoreResult<'_, Vec<Job>>;
    /// Claimed attempts whose lease expired without an answer, and uncertain ones.
    fn unresolved_attempts(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<Attempt>>;
    /// Pending inbox entries, oldest first.
    fn pending_inbox(&self, limit: u32) -> StoreResult<'_, Vec<InboxEntry>>;
    /// Completed attempts whose provider copy still awaits deletion (TRX-041).
    fn remote_cleanup_due(&self, limit: u32) -> StoreResult<'_, Vec<Attempt>>;
    /// Erasures with runnable work.
    fn erasures_due(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<ErasureId>>;
    /// The attempt a callback names; the host selects the binding with it before verification.
    fn attempt(&self, id: AttemptId) -> StoreResult<'_, Option<Attempt>>;
    /// Whether an erasure barrier exists for the account (C-11), checked by every
    /// authenticated entry point before any work.
    fn barrier(&self, account: AccountId) -> StoreResult<'_, bool>;
    /// AI generations a worker may run: accepted, or running with an expired
    /// lease, oldest first; barrier accounts included (the worker interrupts them).
    fn runnable_generations(&self, now: Micros, limit: u32) -> StoreResult<'_, Vec<GenerationId>>;
    /// Completion summary requests without an outcome, oldest first.
    fn pending_summary_requests(&self, limit: u32) -> StoreResult<'_, Vec<(JobId, i64)>>;
    /// Accounts whose AI consent DELETE purge is still pending.
    fn pending_ai_purges(&self, limit: u32) -> StoreResult<'_, Vec<AccountId>>;
}

pub trait JobStore: JobQueries {
    fn begin(&self) -> StoreResult<'_, Box<dyn JobUow>>;
}

/// One host-owned transaction.
pub trait JobUow: Send {
    /// The AI view of this same transaction and connection (no allocation).
    fn ai(&mut self) -> &mut dyn AiTx;
    /// The account rows (feedback, support) of this same transaction (no allocation).
    fn account(&mut self) -> &mut dyn AccountTx;
    /// The API rows (P7 keys, idempotency, callback fan-out) of this same transaction.
    fn api(&mut self) -> &mut dyn crate::api::ports::ApiTx;
    /// Billing joined to this transaction (P2 `PgBillingTx::attach` seam). Drop
    /// it before the next job call; it borrows the same connection.
    fn billing(&mut self) -> BoxFuture<'_, Result<Box<dyn BillingTx + '_>, StoreError>>;

    /// Provider/project capacity lock; taken first, and only by submission claims.
    fn lock_capacity<'a>(&'a mut self, provider: media_ports::Provider, project: &'a str) -> StoreResult<'a, ()>;
    /// Claimed-and-unexpired, accepted and uncertain attempts of a provider/project
    /// plus running jobs it holds (JOB-018 counts running jobs).
    fn inflight<'a>(&'a mut self, provider: media_ports::Provider, project: &'a str, now: Micros) -> StoreResult<'a, u32>;

    /// Owned jobs among `ids`, `FOR UPDATE`, ascending id. Foreign ids are absent.
    fn lock_jobs<'a>(&'a mut self, account: AccountId, ids: &'a [JobId]) -> StoreResult<'a, Vec<Job>>;
    /// One job `FOR UPDATE` regardless of owner, for workers and callbacks.
    fn lock_job(&mut self, id: JobId) -> StoreResult<'_, Option<Job>>;
    /// Other jobs of the account in status `completed` (FRE-006), after the account lock.
    fn completed_job_count(&mut self, account: AccountId) -> StoreResult<'_, u64>;
    fn insert_job<'a>(&'a mut self, job: &'a NewJob) -> StoreResult<'a, Job>;
    /// Compare-and-swap on `expected_revision`; returns the new revision.
    /// `StoreError::Conflict` when the stored revision differs.
    fn save_job<'a>(&'a mut self, job: &'a Job, expected_revision: i64) -> StoreResult<'a, i64>;

    /// Records an issued upload capability so erasure can re-sweep after it expires.
    fn record_issued_upload<'a>(&'a mut self, upload: &'a IssuedUpload) -> StoreResult<'a, ()>;
    /// Records a payload archive intent so erasure can delete the object. Called
    /// inside the result transaction before the PUT; never depends on the job row.
    fn record_archived_payload<'a>(&'a mut self, payload: &'a ArchivedPayload) -> StoreResult<'a, ()>;
    /// F2: the acknowledged outcome of the account's in-flight archive intent for
    /// `object`. Only an `InFlight` row of that account settles; a settled intent
    /// or no row at all is left as is (not an error).
    fn settle_archived_payload<'a>(&'a mut self, account: AccountId, object: &'a ObjectRef, write: ArchiveWrite) -> StoreResult<'a, ()>;
    /// Removes the row and its lines (children first; the RESTRICT key stays).
    fn hard_delete_job(&mut self, id: JobId) -> StoreResult<'_, ()>;

    fn insert_attempt<'a>(&'a mut self, attempt: &'a Attempt) -> StoreResult<'a, ()>;
    fn save_attempt<'a>(&'a mut self, attempt: &'a Attempt) -> StoreResult<'a, ()>;
    fn lock_attempt(&mut self, id: AttemptId) -> StoreResult<'_, Option<Attempt>>;
    /// Every attempt of a job, current and superseded.
    fn attempts_of(&mut self, job: JobId) -> StoreResult<'_, Vec<Attempt>>;

    fn insert_lines<'a>(&'a mut self, job: &'a Job, lines: &'a [NewLine]) -> StoreResult<'a, u64>;
    fn lock_line(&mut self, id: LineId) -> StoreResult<'_, Option<Line>>;
    /// Update-only; never inserts (D7). Returns false when the line is gone.
    fn update_line_text<'a>(&'a mut self, id: LineId, text: &'a str) -> StoreResult<'a, bool>;
    fn update_line_speaker<'a>(&'a mut self, id: LineId, speaker: &'a str) -> StoreResult<'a, bool>;
    fn rename_speaker<'a>(&'a mut self, job: JobId, from: &'a str, to: &'a str) -> StoreResult<'a, u64>;

    /// Durable intake. Returns the stored entry and whether it was new; an
    /// existing entry for the same attempt and remote id is returned unchanged.
    fn insert_inbox<'a>(&'a mut self, entry: &'a NewInboxEntry) -> StoreResult<'a, (InboxEntry, bool)>;
    /// F6: keeps a delivery whose body differs from the stored entry `of` (same
    /// attempt and remote id) as its own entry in state `Conflict`, with its body
    /// and fingerprint, never applied and never pending. Idempotent per
    /// (`of`, fingerprint); erased with the account like every inbox entry.
    fn hold_inbox_conflict<'a>(&'a mut self, of: InboxId, entry: &'a NewInboxEntry) -> StoreResult<'a, ()>;
    fn lock_inbox(&mut self, id: InboxId) -> StoreResult<'_, Option<InboxEntry>>;
    /// Settles an entry; `Applied` and `Ignored` drop the raw body, keeping the fingerprint.
    fn settle_inbox<'a>(&'a mut self, id: InboxId, state: InboxState, note: Option<&'a str>, at: Micros) -> StoreResult<'a, ()>;
    fn record_inbox_failure<'a>(&'a mut self, id: InboxId, error: &'a str) -> StoreResult<'a, ()>;

    /// Queues an event in this transaction (outbox; delivered after commit).
    fn emit<'a>(&'a mut self, event: &'a JobEvent) -> StoreResult<'a, ()>;

    /// The settings row `FOR UPDATE`; the default row when absent (PRV-013).
    fn lock_settings(&mut self, account: AccountId) -> StoreResult<'_, UserSettings>;
    fn save_settings<'a>(&'a mut self, settings: &'a UserSettings) -> StoreResult<'a, ()>;

    /// Retention candidates at a fixed cutoff (PRV-014..016): completed jobs created
    /// at or before `cutoff` with at least one line, with their owner's preference.
    /// Read without locks; the apply step locks and rereads.
    fn retention_candidates(&mut self, cutoff: Micros) -> StoreResult<'_, Vec<RetentionCandidate>>;
    /// Locks the owners' settings rows in ascending account order, then the
    /// candidate jobs in ascending id, and rereads the same predicate.
    fn lock_retention_set<'a>(&'a mut self, candidates: &'a [RetentionCandidate], cutoff: Micros) -> StoreResult<'a, Vec<RetentionCandidate>>;
    /// Deletes the lines of these jobs; returns the deleted row count. Also
    /// clears the stored callback payloads of these jobs (P7 D10).
    fn delete_lines_of<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, u64>;
    /// Accounts that would have had lines deleted but opted out (PRV-022).
    fn opted_out_count(&mut self, cutoff: Micros) -> StoreResult<'_, u64>;
    fn insert_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()>;
    fn save_retention_run<'a>(&'a mut self, run: &'a RetentionRun) -> StoreResult<'a, ()>;
    /// Every audit row of `job_name` still `running` becomes `failure` with `message`
    /// (rows deleted 0); returns how many. Called while the job's advisory lock is
    /// held, so no such row belongs to a live run: each is an interrupted one.
    fn fail_interrupted_runs<'a>(&'a mut self, job_name: &'a str, message: &'a str, at: Micros) -> StoreResult<'a, u64>;
    /// PRV-025: finished runs of the named schedules (never pending or running
    /// claims) whose start is at or before `cutoff`.
    fn history_count<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64>;
    /// Deletes exactly the rows `history_count` counts; returns how many.
    fn delete_history<'a>(&'a mut self, schedules: &'a [&'a str], cutoff: Micros) -> StoreResult<'a, u64>;

    /// The erasure barrier `FOR UPDATE`, if one exists (C-11).
    fn lock_barrier(&mut self, account: AccountId) -> StoreResult<'_, Option<ErasureRecord>>;
    /// Writes the barrier record when the erasure is requested (no items yet).
    fn insert_erasure<'a>(&'a mut self, record: &'a ErasureRecord) -> StoreResult<'a, ()>;
    /// The resource manifest, captured in the cascade transaction.
    fn insert_erasure_items<'a>(&'a mut self, items: &'a [ErasureItem]) -> StoreResult<'a, ()>;
    fn lock_erasure(&mut self, id: ErasureId) -> StoreResult<'_, Option<ErasureRecord>>;
    fn save_erasure<'a>(&'a mut self, record: &'a ErasureRecord) -> StoreResult<'a, ()>;
    fn erasure_items(&mut self, id: ErasureId) -> StoreResult<'_, Vec<ErasureItem>>;
    fn save_erasure_item<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()>;
    /// Updates target, state and attempts of an existing LLM dispatch item; only
    /// for late dispatch identity under the erasure barrier (D9). Both the stored
    /// and the new target must be `LlmDispatch`; otherwise, or when the item is
    /// gone, `StoreError::Conflict`.
    fn save_erasure_item_target<'a>(&'a mut self, item: &'a ErasureItem) -> StoreResult<'a, ()>;
    /// Every resource of the account the manifest must capture before the cascade:
    /// job ids, stored objects, raw payloads, provider remote identities, unexpired
    /// upload capabilities, inbox bodies.
    fn account_resources(&mut self, account: AccountId) -> StoreResult<'_, AccountResources>;
    /// The host cascade: lines, attempts, inbox, jobs, settings and the customer
    /// directory row, children first. Runs after billing's
    /// `erase_account_billing` on `billing()` in the same transaction (D9).
    fn erase_account_rows(&mut self, account: AccountId) -> StoreResult<'_, ()>;

    /// A named savepoint, so a deferred debit or line insert failure can roll back
    /// alone while delivery facts still commit (seam 5).
    fn savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()>;
    fn rollback_to_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()>;
    fn release_savepoint<'a>(&'a mut self, name: &'a str) -> StoreResult<'a, ()>;

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>>;
    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>>;
}

/// A held advisory lock; dropping or releasing it frees the lock (media-postgres
/// dedicated-connection lock, adapted by the host).
pub trait LockGuard: Send {
    fn release(self: Box<Self>) -> BoxFuture<'static, ()>;
}

pub trait Locks: Send + Sync {
    /// `pg_try_advisory_lock` semantics: `None` when another holder has it.
    fn try_lock<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<Box<dyn LockGuard>>, StoreError>>;
}

/// A fixed window that starts at the first request; denied requests do not
/// count (RTL-005), shared by all replicas (IDN-D03 correction).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateDecision {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    pub reset_at: Micros,
}

pub trait RateLimiter: Send + Sync {
    fn hit<'a>(&'a self, key: &'a str, limit: u32, window_ms: i64, now: Micros) -> BoxFuture<'a, Result<RateDecision, StoreError>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityOutcome {
    Deleted,
    Absent,
}

/// The GoTrue admin user deletion (`DELETE /admin/users/{id}`), host-implemented.
pub trait IdentityAdmin: Send + Sync {
    fn delete_user(&self, account: AccountId) -> BoxFuture<'_, Result<IdentityOutcome, String>>;
}

/// Pauses between Deepgram queue submissions (JOB-018); the host supplies its runtime.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, millis: u64) -> BoxFuture<'_, ()>;
}

/// An unexpired presigned PUT the erasure must outlast (re-sweep after expiry).
/// `account` is the caller the URL was signed for (its folder), not the job's
/// owner: the job id in the path need not exist or be the caller's (as built).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IssuedUpload {
    pub account: AccountId,
    pub job: JobId,
    pub object: ObjectRef,
    pub expires_at: Micros,
}

/// A raw provider payload archive (JOB-044, F2). Recorded `InFlight` in the
/// result transaction before commit, ahead of the PUT, so erasure knows the
/// object whatever happens to the job row, the database or the PUT.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArchivedPayload {
    pub account: AccountId,
    pub job: JobId,
    pub object: ObjectRef,
    pub write: ArchiveWrite,
}

/// The durable outcome of one archive PUT (F2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveWrite {
    /// Recorded before the PUT; its outcome is not acknowledged yet.
    InFlight,
    /// The PUT completed: the object exists.
    Written,
    /// The PUT was refused or never sent: no object.
    NotWritten,
}

/// Free-shape details attached to an event or audit row.
pub type Details = Value;
