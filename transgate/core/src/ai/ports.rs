//! What AI persistence implements: one `AiTx` view over the SAME host
//! transaction as `JobUow` (borrowed through `JobUow::ai`, no second pool or
//! connection), plus the records it stores.
//!
//! Lock order (seam 5, D2): the caller locks the billing account first
//! (`BillingTx::lock_account`), then re-reads the erasure barrier, then takes
//! AI rows (`lock_generation`, `lock_counter`). No method locks the account or
//! a job by itself, and no method opens network I/O. Compare-and-update
//! methods never recreate a missing owner, job or generation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::GenerationId;
use crate::ai::analysis::{AnalysisProgress, AnalysisResult};
use crate::ids::{AccountId, JobId, Micros};
use crate::jobs::ports::StoreResult;

/// The three kinds of generation (AI-001, AI-010, AI-024).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationKind {
    /// Specialized analysis: detected type, summary and typed sections.
    Analysis,
    /// One transcript question and its answer (metered for freemium callers).
    Chat,
    /// The separate, unmetered general summary driven by job completion.
    CompletionSummary,
}

/// A caller-supplied chat turn. Roles are user/assistant only (AI-D06).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: ChatRole,
    pub content: String,
}

/// What a generation was asked to do. Private: never in a public snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenerationInput {
    Analysis,
    Chat {
        /// The original accepted question, persisted as the user message (D5).
        question: String,
        /// Its sanitized copy: the provider input and the quota estimate.
        sanitized_question: String,
        /// The caller's history, trimmed as the source does, unsanitized; the
        /// worker sanitizes it into the provider request (AI-013).
        history: Vec<ChatTurn>,
        /// The response-language code for a non-English job (AI-013).
        language: Option<String>,
    },
    CompletionSummary {
        detailed: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    /// Admitted and durable; no worker holds it yet.
    Accepted,
    /// A worker holds the execution lease.
    Running,
    /// Final content, its artifact and (for metered chat) the settlement
    /// committed in one transaction.
    Completed,
    /// Ended without a durable final; valid partial content stays.
    Interrupted,
    /// The owner cancelled; local consumption stopped (never proof of remote
    /// cancellation).
    Cancelled,
}

impl GenerationStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, GenerationStatus::Completed | GenerationStatus::Interrupted | GenerationStatus::Cancelled)
    }
}

/// Why a generation ended without a durable final.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Interruption {
    /// The provider stream failed (transport, HTTP status, timeout).
    Provider { message: String },
    /// The provider finished without a successful stop (length, filter, tool calls).
    Unsuccessful { reason: String },
    /// The analysis framing or documents were malformed or incomplete.
    Malformed { message: String },
    /// An explicit D4 bound was reached; nothing was truncated.
    Limit { message: String },
    /// The 60-second total deadline passed.
    Deadline,
    /// Consent was refused or revoked after admission (epoch fence).
    Revoked,
    /// The transcript changed, lost its lines or its job was deleted.
    SourceChanged,
    /// The account entered erasure.
    Erased,
    /// The worker lost its lease without a durable final.
    WorkerLost,
    /// The final could not be saved; the answer may have been shown unsaved.
    NotSaved { message: String },
}

/// A sanitized, bounded, cumulative checkpoint. Never raw provider text.
///
/// The analysis variants are boxed: one allocation per analysis checkpoint or
/// final (which already own many heap strings and lists) keeps `Draft`,
/// `FinalContent` and every `Generation` small, which are moved and cloned on
/// each transaction and view. Serialized shapes are unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Draft {
    Empty,
    Text { text: String },
    Analysis { progress: Box<AnalysisProgress> },
}

/// The sanitized final content, staged before the completing transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinalContent {
    Text { text: String },
    Analysis { result: Box<AnalysisResult> },
}

/// The worker's execution claim: only its token may write the generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    pub token: Uuid,
    pub lease_until: Micros,
}

/// Quota counter identity (AI-015..018).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "scope", content = "job", rename_all = "snake_case")]
pub enum CounterScope {
    /// `*_per_transcript`, period `transcript`, never resets.
    Transcript(JobId),
    /// `*_per_user`, period daily/weekly/monthly.
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterKind {
    Questions,
    Tokens,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    /// The source default for an unset or unknown reset period (AI-017).
    #[default]
    Daily,
    Weekly,
    Monthly,
    Transcript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CounterKey {
    pub account: AccountId,
    pub scope: CounterScope,
    pub kind: CounterKind,
    pub period: Period,
}

/// One counter row. `count` includes amounts reserved by admitted, unsettled
/// generations of the period ending at `reset_at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaCounter {
    pub key: CounterKey,
    pub count: i64,
    /// The stored maximum (imported rows keep theirs); question checks use it (AI-018).
    pub max_limit: Option<i64>,
    /// End of the current period; `None` for transcript counters.
    pub reset_at: Option<Micros>,
}

/// One reserved amount, bound to the period it was admitted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservedBucket {
    pub key: CounterKey,
    /// The counter's `reset_at` at admission: settlement and release touch the
    /// counter only while it still is this period.
    pub period_end: Option<Micros>,
    pub amount: i64,
}

/// Frozen at admission (D2): entitlement, caps and periods never re-evaluated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaReservation {
    pub buckets: Vec<ReservedBucket>,
    /// `ceil(UTF16(sanitized question)/4)`, the reserved token estimate.
    pub question_tokens: i64,
}

/// One generation row: identity, fences, bounded current snapshot and the
/// staged final. No token or event log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generation {
    pub id: GenerationId,
    pub account: AccountId,
    pub job: JobId,
    pub kind: GenerationKind,
    /// The caller's idempotency key; unique per account.
    pub request_key: Uuid,
    /// Digest of kind, job and payload: the same key with another digest conflicts.
    pub request_digest: String,
    /// The job's content revision at admission (fence).
    pub source_revision: i64,
    /// The consent epoch at admission (fence; also the artifact's epoch).
    pub consent_epoch: i64,
    pub input: GenerationInput,
    pub status: GenerationStatus,
    /// Monotonic snapshot watermark.
    pub sequence: i64,
    pub draft: Draft,
    pub staged: Option<FinalContent>,
    pub execution: Option<Execution>,
    /// `Some` only for metered chat until settled or released.
    pub quota: Option<QuotaReservation>,
    pub interruption: Option<Interruption>,
    /// The provider may hold or bill work whose local outcome is unknown.
    pub remote_outcome_unknown: bool,
    /// The provider's completion id, when it sent one.
    pub remote_id: Option<String>,
    /// The source's completion metadata estimate: analysis
    /// `round(UTF16(raw canonical transcript)/4)`, chat
    /// `ceil(UTF16(sanitized question + answer)/4)`. Not a quota amount.
    pub tokens_used: Option<i64>,
    pub created_at: Micros,
    pub updated_at: Micros,
    pub finished_at: Option<Micros>,
}

/// One persisted chat message (AI-021). A pair shares its generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub account: AccountId,
    pub job: JobId,
    pub generation: GenerationId,
    /// Per-job order, assigned by `append_chat_pair`.
    pub order: i64,
    pub role: ChatRole,
    pub content: String,
    pub consent_epoch: i64,
    pub created_at: Micros,
}

/// The general completion summary (AI-024/025), one per job content revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionSummary {
    pub account: AccountId,
    pub job: JobId,
    pub source_revision: i64,
    pub generation: GenerationId,
    pub text: String,
    pub consent_epoch: i64,
    pub created_at: Micros,
}

/// The durable summary outcome that releases callback assembly (D5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SummaryOutcome {
    Saved { generation: GenerationId },
    /// AI features disabled, no transcript lines or no LLM configured.
    Absent { reason: String },
    /// No valid consent or the account is erasing.
    Denied { reason: String },
    /// The generation ended without a durable final.
    Failed { reason: String },
}

/// A completed job awaiting its summary outcome (the durable producer queue).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryRequest {
    pub account: AccountId,
    pub job: JobId,
    pub source_revision: i64,
    pub requested_at: Micros,
    /// The generation started for it, once one is.
    pub generation: Option<GenerationId>,
    /// `None` while pending: callback assembly waits.
    pub outcome: Option<SummaryOutcome>,
    pub decided_at: Option<Micros>,
}

/// At most this many distinct provider identifiers per dispatch are kept.
pub const MAX_DISPATCH_REMOTE_IDS: usize = 8;
/// A longer identifier is not kept (the dispatch stays possible).
pub const MAX_DISPATCH_REMOTE_ID_BYTES: usize = 256;

/// Whether a dispatch may have reached the provider (D3, D9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchDisposition {
    /// Recorded before provider I/O; the provider may hold the request.
    Possible,
    /// The transport reported that nothing was sent, and no provider id was observed.
    NotDispatched,
}

/// The payload-free durable intent of one LLM request (D3, D9): written in
/// the claim transaction before any provider I/O; it survives consent DELETE
/// purges, hard job deletion and transcript deletion, and is removed only by
/// account erasure after the manifest captured it. Never a prompt, question
/// or transcript.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmDispatch {
    pub account: AccountId,
    pub generation: GenerationId,
    /// The trusted configured provider (`ModelSettings::provider`).
    pub provider: String,
    pub model: String,
    pub disposition: DispatchDisposition,
    /// Distinct observed provider identifiers (request id, completion id),
    /// first seen first, bounded.
    pub remote_ids: Vec<String>,
    /// A nonempty provider id was observed but not kept (over 256 bytes, or
    /// the list was full): dispatch evidence that never expires.
    #[serde(default)]
    pub unkept_id_observed: bool,
    pub created_at: Micros,
}

impl LlmDispatch {
    /// Applies one settlement: a new observed id is appended when within the
    /// bounds, or else remembered as unkept evidence (never truncated or
    /// invented); `NotDispatched` holds only while no id was ever observed.
    pub fn settle(&mut self, disposition: DispatchDisposition, observed: Option<&str>) {
        if let Some(id) = observed.filter(|id| !id.is_empty())
            && !self.remote_ids.iter().any(|known| known == id)
        {
            if id.len() <= MAX_DISPATCH_REMOTE_ID_BYTES && self.remote_ids.len() < MAX_DISPATCH_REMOTE_IDS {
                self.remote_ids.push(id.to_owned());
            } else {
                self.unkept_id_observed = true;
            }
        }
        let evidence = !self.remote_ids.is_empty() || self.unkept_id_observed;
        self.disposition = if evidence { DispatchDisposition::Possible } else { disposition };
    }
}

/// The account's AI consent (AI-022, D3). No row reads as never consented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentRecord {
    pub account: AccountId,
    pub has_consented: bool,
    pub version: String,
    pub provider: String,
    pub policy_url: String,
    pub consent_date: Option<Micros>,
    pub revoked_date: Option<Micros>,
    /// Write epoch: advanced by every grant, refusal and revocation.
    pub epoch: i64,
    /// Every artifact with an epoch at or below this (or none, legacy) is
    /// purged and never readable again (DELETE, D3).
    pub purge_cutoff: Option<i64>,
    /// A DELETE whose purge has not completed yet.
    pub purge_pending: bool,
    pub updated_at: Micros,
}

/// The AI view of the host transaction. Every method runs on the same
/// connection and transaction as the `JobUow` that lent it.
pub trait AiTx: Send {
    // ---- consent ----------------------------------------------------------

    /// The consent row, if any. Callers hold the account lock for writes.
    fn consent(&mut self, account: AccountId) -> StoreResult<'_, Option<ConsentRecord>>;
    /// Upsert of the whole row (one per account).
    fn save_consent<'a>(&'a mut self, record: &'a ConsentRecord) -> StoreResult<'a, ()>;

    // ---- source revision --------------------------------------------------

    /// The job's content revision; 0 when it never changed.
    fn source_revision(&mut self, job: JobId) -> StoreResult<'_, i64>;
    /// Advances and returns the job's content revision by one: line edits,
    /// speaker renames, soft deletion.
    fn bump_source_revision(&mut self, job: JobId) -> StoreResult<'_, i64>;
    /// Advances the content revision of every listed job by one in one
    /// statement (transcript retention's line deletion).
    fn bump_source_revisions<'a>(&'a mut self, jobs: &'a [JobId]) -> StoreResult<'a, ()>;

    // ---- generations ------------------------------------------------------

    /// The account's generation for this idempotency key, any status.
    fn generation_by_request(&mut self, account: AccountId, request_key: Uuid) -> StoreResult<'_, Option<Generation>>;
    /// The account's non-terminal generation holding a quota reservation (at
    /// most one, D2).
    fn active_metered(&mut self, account: AccountId) -> StoreResult<'_, Option<Generation>>;
    /// The job's non-terminal generation of this kind (at most one).
    fn active_for_job(&mut self, account: AccountId, job: JobId, kind: GenerationKind) -> StoreResult<'_, Option<Generation>>;
    /// Inserts a new generation. `StoreError::Conflict` when the account
    /// already has this request key (the caller re-reads and compares digests).
    fn insert_generation<'a>(&'a mut self, generation: &'a Generation) -> StoreResult<'a, ()>;
    /// `FOR UPDATE` on the generation row.
    fn lock_generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>>;
    /// Updates every mutable field of an existing row; never inserts.
    /// `StoreError::Conflict` when the row is gone.
    fn save_generation<'a>(&'a mut self, generation: &'a Generation) -> StoreResult<'a, ()>;
    /// The generation, unlocked (reads and watch).
    fn generation(&mut self, id: GenerationId) -> StoreResult<'_, Option<Generation>>;
    /// The job's newest analysis generation (the current-analysis pointer).
    fn latest_analysis(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<Generation>>;

    // ---- quota ------------------------------------------------------------

    /// `FOR UPDATE` on the counter row, if it exists.
    fn lock_counter(&mut self, key: CounterKey) -> StoreResult<'_, Option<QuotaCounter>>;
    /// Upsert of the counter row.
    fn save_counter<'a>(&'a mut self, counter: &'a QuotaCounter) -> StoreResult<'a, ()>;
    /// Every counter of the account (user scope and the given job), unlocked.
    fn counters(&mut self, account: AccountId, job: Option<JobId>) -> StoreResult<'_, Vec<QuotaCounter>>;

    // ---- artifacts --------------------------------------------------------

    /// Appends the user/assistant pair of one generation with the next two
    /// per-job orders (the `order` fields are ignored) and returns the stored
    /// pair. `StoreError::Conflict` when the generation already has a pair.
    fn append_chat_pair<'a>(&'a mut self, question: &'a ChatMessage, answer: &'a ChatMessage) -> StoreResult<'a, [ChatMessage; 2]>;
    /// The stored pair of one generation (unknown-COMMIT reconciliation).
    fn chat_pair(&mut self, generation: GenerationId) -> StoreResult<'_, Option<[ChatMessage; 2]>>;
    /// The job's messages in order.
    fn chat_history(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Vec<ChatMessage>>;
    /// Saves the summary of `(job, source_revision)`; an existing one for the
    /// same key is left as is (idempotent).
    fn save_completion_summary<'a>(&'a mut self, summary: &'a CompletionSummary) -> StoreResult<'a, ()>;
    /// The job's newest completion summary.
    fn completion_summary(&mut self, account: AccountId, job: JobId) -> StoreResult<'_, Option<CompletionSummary>>;

    // ---- completion summary producer --------------------------------------

    /// Records that `(job, source_revision)` completed and needs a summary
    /// outcome; an existing request for the key is left as is.
    fn request_summary<'a>(&'a mut self, request: &'a SummaryRequest) -> StoreResult<'a, ()>;
    /// `FOR UPDATE` on the request row.
    fn lock_summary_request(&mut self, job: JobId, source_revision: i64) -> StoreResult<'_, Option<SummaryRequest>>;
    /// Updates `generation`, `outcome` and `decided_at` of an existing row.
    fn save_summary_request<'a>(&'a mut self, request: &'a SummaryRequest) -> StoreResult<'a, ()>;
    /// The job's newest request (callback assembly reads its outcome).
    fn summary_request(&mut self, job: JobId) -> StoreResult<'_, Option<SummaryRequest>>;

    // ---- purge ------------------------------------------------------------

    /// DELETE purge (D3): removes every AI artifact of the account whose
    /// consent epoch is at or below `cutoff` or absent (legacy): generations
    /// (drafts and staged finals included), chat messages and completion
    /// summaries. Imported legacy summaries/highlights (P9) and stored
    /// callback copies (P7) join this purge when those phases add them.
    /// Quota counters and summary requests (content-free) stay. Returns the
    /// rows removed. Also clears every stored callback payload of the account
    /// (P7 D10: a body may carry the summary); those are not counted.
    fn purge_ai_artifacts(&mut self, account: AccountId, cutoff: i64) -> StoreResult<'_, u64>;

    // ---- LLM dispatch intents (D3, D9) --------------------------------------

    /// Records the intent of one request before provider I/O. An identical row
    /// for the generation is left as is; a row with another account, provider
    /// or model is `StoreError::Conflict`.
    fn record_dispatch<'a>(&'a mut self, dispatch: &'a LlmDispatch) -> StoreResult<'a, ()>;
    /// Applies `LlmDispatch::settle` to the account's row for the generation;
    /// `false` when the row is gone (account erasure removed it).
    fn settle_dispatch<'a>(
        &'a mut self,
        account: AccountId,
        generation: GenerationId,
        disposition: DispatchDisposition,
        observed: Option<&'a str>,
    ) -> StoreResult<'a, bool>;
}

#[cfg(test)]
#[path = "ports/tests.rs"]
mod tests;
