//! Transgate AI: specialized analysis, transcript chat, the general completion
//! summary, consent and chat limits (AI-001..026, FRE-014/024, D1..D6).
//!
//! Product policy lives here; the provider transport and the text-stream
//! reducer come from `llm-core`. Persistence implements `ports::AiTx` on the
//! same host transaction as jobs and billing. No network in a transaction, no
//! durable token log, no automatic provider retry.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ids::{JobId, Micros};

pub mod analysis;
pub mod app;
pub mod consent;
pub mod generation;
pub mod live;
pub mod ports;
pub mod presentation;
pub mod prompts;
pub mod quota;
pub mod sanitize;

pub use app::{Ai, AiConfig, AiServices, Bounds, ChatLimits, ChatRequest, HistoryTurn, ModelSettings, Watch, WatchReservation};
pub use ports::{
    AiTx, ChatMessage, ChatRole, ChatTurn, CompletionSummary, ConsentRecord, Draft, FinalContent, Generation,
    GenerationKind, GenerationStatus, Interruption, SummaryOutcome,
};

/// One generation (analysis, chat answer or completion summary).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GenerationId(pub Uuid);

impl GenerationId {
    pub fn new() -> GenerationId {
        GenerationId(Uuid::new_v4())
    }
}

impl Default for GenerationId {
    fn default() -> Self {
        GenerationId::new()
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

/// A refused chat admission (AI-015, FRE-024), with the source's codes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaRefusal {
    /// `TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED`, `USER_QUESTIONS_LIMIT_EXCEEDED`,
    /// `TRANSCRIPT_TOKENS_LIMIT_EXCEEDED` or `USER_TOKENS_LIMIT_EXCEEDED`.
    pub code: String,
    pub message: String,
    pub current: i64,
    pub max: i64,
    pub reset_at: Option<Micros>,
    pub period: ports::Period,
}

/// Errors a shell maps to HTTP; `http_status` is the intended mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    /// Missing or not owned job or generation: 404.
    NotFound,
    /// No valid (granted, not revoked) consent: 403.
    ConsentRequired,
    /// An analysis job has no transcript lines: 400.
    NoTranscript,
    /// `ENABLE_AI_FEATURES` is not exactly true: 503.
    Disabled,
    /// `ENABLE_AI_CHAT` is not exactly true: 503.
    ChatDisabled,
    /// No LLM transport configured: 503.
    NotConfigured,
    /// Request validation (AI-010/012, history roles, security patterns): 400.
    Invalid(String),
    /// A freemium chat cap refused the question: 429.
    Quota(QuotaRefusal),
    /// Another metered generation of the account, or an active analysis of
    /// the job, is running: 409.
    Busy(GenerationId),
    /// The request key was used for another operation or payload: 409.
    Conflict,
    /// The operation does not apply in the generation's state: 409.
    WrongState(String),
    /// The account is being erased: 410.
    Erased,
    /// The process already runs its maximum of dispatched generations: 503.
    Capacity,
    /// Storage failed or its outcome is unknown: 503.
    Unavailable(String),
}

impl AiError {
    pub fn http_status(&self) -> u16 {
        match self {
            AiError::NotFound => 404,
            AiError::ConsentRequired => 403,
            AiError::NoTranscript | AiError::Invalid(_) => 400,
            AiError::Quota(_) => 429,
            AiError::Busy(_) | AiError::Conflict | AiError::WrongState(_) => 409,
            AiError::Erased => 410,
            AiError::Disabled | AiError::ChatDisabled | AiError::NotConfigured | AiError::Capacity | AiError::Unavailable(_) => 503,
        }
    }

    /// The source's user-facing message where one exists.
    pub fn message(&self) -> String {
        match self {
            AiError::NotFound => "Job not found".into(),
            AiError::ConsentRequired => "AI consent required. Please enable AI features in your settings.".into(),
            AiError::NoTranscript => "No transcript content found for this job".into(),
            AiError::Disabled => "AI features are currently disabled".into(),
            AiError::ChatDisabled => "AI chat feature is currently disabled".into(),
            AiError::NotConfigured => "AI provider is not configured".into(),
            AiError::Invalid(m) | AiError::WrongState(m) | AiError::Unavailable(m) => m.clone(),
            AiError::Quota(q) => q.message.clone(),
            AiError::Busy(_) => "Another AI request is still running".into(),
            AiError::Conflict => "This request id was already used for a different request".into(),
            AiError::Erased => "The account is being erased".into(),
            AiError::Capacity => "AI is at capacity; try again shortly".into(),
        }
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for AiError {}

impl From<billing_core::ports::StoreError> for AiError {
    fn from(e: billing_core::ports::StoreError) -> Self {
        match e {
            billing_core::ports::StoreError::AccountErased(_) => AiError::Erased,
            other => AiError::Unavailable(other.to_string()),
        }
    }
}

/// A public generation snapshot: no prompt, transcript or original question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationView {
    pub id: GenerationId,
    pub job: JobId,
    pub kind: GenerationKind,
    pub status: GenerationStatus,
    pub sequence: i64,
    pub source_revision: i64,
    pub consent_epoch: i64,
    /// The sanitized cumulative checkpoint (replaced by `final_content` once completed).
    pub draft: Draft,
    /// Present only when `status` is completed: committed canonical content.
    pub final_content: Option<FinalContent>,
    /// True when the shown content is committed state, not a live provisional view.
    pub durable: bool,
    pub remote_outcome_unknown: bool,
    /// The execution that streams or streamed it: part of the live frame
    /// identity, so polled replacements match live frames.
    pub execution: Option<Uuid>,
    /// The source's completion metadata estimate (analysis `complete`, chat `done`).
    pub tokens_used: Option<i64>,
    pub interruption: Option<Interruption>,
    pub created_at: Micros,
    pub finished_at: Option<Micros>,
}

impl GenerationView {
    pub fn of(g: &Generation) -> GenerationView {
        let completed = g.status == GenerationStatus::Completed;
        GenerationView {
            id: g.id,
            job: g.job,
            kind: g.kind,
            status: g.status,
            sequence: g.sequence,
            source_revision: g.source_revision,
            consent_epoch: g.consent_epoch,
            draft: g.draft.clone(),
            final_content: if completed { g.staged.clone() } else { None },
            durable: true,
            remote_outcome_unknown: g.remote_outcome_unknown,
            execution: g.execution.map(|e| e.token),
            tokens_used: g.tokens_used,
            interruption: g.interruption.clone(),
            created_at: g.created_at,
            finished_at: g.finished_at,
        }
    }
}

/// The answer to a start or retry: `created` false is an exact replay of the
/// same request key (HTTP 200 rather than 202; never another provider call).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Started {
    pub generation: GenerationView,
    pub created: bool,
}

/// `GET /api/ai/consent` (AI-022), plus the D3 cleanup state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentView {
    pub has_consented: bool,
    pub consent_version: String,
    pub consent_date: Option<Micros>,
    pub revoked_date: Option<Micros>,
    pub privacy_policy_url: String,
    pub provider_name: String,
    pub can_revoke: bool,
    /// A DELETE whose artifact purge has not completed yet.
    pub cleanup_pending: bool,
}

/// One configured cap with its usage (API-BIL-011).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitUsage {
    pub current: i64,
    pub max: i64,
    pub exceeded: bool,
    pub reset_at: Option<Micros>,
    /// The configured reset period for user caps; `None` for transcript caps.
    pub reset_period: Option<ports::Period>,
}

/// `GET /api/ai/chat-limits?jobId=` (API-BIL-011, FRE-024).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsView {
    pub enabled: bool,
    pub has_limits: bool,
    pub questions_per_transcript: Option<LimitUsage>,
    pub questions_per_user: Option<LimitUsage>,
    pub tokens_per_transcript: Option<LimitUsage>,
    pub tokens_per_user: Option<LimitUsage>,
}

/// The general completion summary an owner reads (TRX-039).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryView {
    pub job: JobId,
    pub source_revision: i64,
    pub text: String,
    pub created_at: Micros,
}

/// What a worker pass did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiPassReport {
    pub examined: u32,
    pub completed: u32,
    pub interrupted: u32,
    pub reconciled: u32,
    pub errors: u32,
}
