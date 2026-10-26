//! The AI use cases: admission, reads, consent and limits, cancellation, the
//! worker that streams one generation, the completion-summary producer and
//! the DELETE purge.
//!
//! Every write runs in one host transaction: erasure barrier, billing account
//! lock, then AI rows, with consent, content revision and execution fences
//! re-read under the lock. Provider I/O never runs inside a transaction.

use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use billing_core::freemium::is_freemium_app2;
use billing_core::ports::StoreError;
use billing_core::types::Clock;
use llm_core::{
    ApplicationFrame, CompletionEvent, CompletionRequest, ErrorKind, Llm, LlmError, Message, Persistence, PublicError,
    RemoteOutcome, ResponseFormat, Role, StreamIdentity, StreamStatus, TextEvent,
};
use uuid::Uuid;

use crate::ai::analysis::{AnalysisParser, AnalysisProgress};
use crate::ai::generation::{self, Fences, NewGeneration};
use crate::ai::live::{LiveHub, Subscription};
use crate::ai::presentation::public_error;
pub use crate::ai::quota::ChatLimits;
use crate::ai::ports::{
    ChatMessage, ChatRole, ChatTurn, CompletionSummary, ConsentRecord, DispatchDisposition, Draft, FinalContent, Generation,
    GenerationInput, GenerationKind, GenerationStatus, Interruption, LlmDispatch, QuotaReservation, SummaryOutcome,
    SummaryRequest,
};
use crate::ai::{
    AiError, AiPassReport, ConsentView, GenerationId, GenerationView, LimitUsage, LimitsView, Started, SummaryView,
    consent, prompts, quota, sanitize,
};
use crate::ids::{AccountId, JobId, Micros};
use crate::jobs::domain::{Job, JobStatus};
use crate::jobs::ports::{JobStore, JobUow, Sleeper};
use crate::privacy::erasure::{self, ErasureState, ItemState, ItemTarget};
use crate::transcript::Line;

/// Model request settings (AI-005).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelSettings {
    /// The trusted provider the host composed (recorded in dispatch intents).
    pub provider: String,
    pub model: String,
    pub temperature: f64,
    pub max_output_tokens: u32,
    /// Send `response_format=json_object` for analysis. The source did
    /// (AI-004), which contradicts its multi-document prompt (AI-D01); live
    /// analysis omits it (D1 framing correction), the E4 baseline keeps it.
    pub analysis_json_mode: bool,
}

impl ModelSettings {
    /// Live settings: the captured `deepseek-chat`, temperature 0.3 and 1000
    /// output tokens, with the D1 framing correction (no JSON mode for analysis).
    pub fn source() -> ModelSettings {
        ModelSettings { provider: "deepseek".into(), model: "deepseek-chat".into(), temperature: 0.3, max_output_tokens: 1000, analysis_json_mode: false }
    }

    /// The exact source request, for the E4 baseline comparison only.
    pub fn source_baseline() -> ModelSettings {
        ModelSettings { analysis_json_mode: true, ..ModelSettings::source() }
    }
}

/// D4 safety restrictions. Exceeding one refuses or interrupts; nothing is truncated.
#[derive(Clone, Debug, PartialEq)]
pub struct Bounds {
    /// Decoded input (history included), generated output, SSE record and JSON depth.
    pub llm: llm_core::Limits,
    /// Queued raw deltas per live attachment.
    pub subscriber_delta_bytes: usize,
    /// Dispatched generations per process.
    pub max_dispatch: usize,
    /// Total deadline of every completion kind.
    pub deadline: Duration,
    /// Worker lease, renewed at each checkpoint; longer than the deadline.
    pub lease: Duration,
    /// Raw prose growth between two sanitized checkpoints.
    pub checkpoint_bytes: usize,
    /// While the provider is quiet: save unsaved prose or re-read the fences
    /// this often (milliseconds), so other attachments see partial text and a
    /// refusal or cancellation stops a silent stream promptly.
    pub quiet_millis: u64,
    /// Live watch attachments per process and per account (D4); refused at once beyond.
    pub watches_per_process: usize,
    pub watches_per_account: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds {
            llm: llm_core::Limits::default(),
            subscriber_delta_bytes: 64 * 1024,
            max_dispatch: 8,
            deadline: Duration::from_secs(60),
            lease: Duration::from_secs(90),
            checkpoint_bytes: 4 * 1024,
            quiet_millis: 250,
            watches_per_process: 32,
            watches_per_account: 4,
        }
    }
}

/// Deployment configuration. Flags follow the source's exact-`true` checks.
#[derive(Clone, Debug, PartialEq)]
pub struct AiConfig {
    /// `ENABLE_AI_FEATURES === 'true'`.
    pub features_enabled: bool,
    /// `ENABLE_AI_CHAT === 'true'`.
    pub chat_enabled: bool,
    /// `AI_CHAT_VALIDATION_ENABLED !== 'false'`.
    pub chat_validation: bool,
    pub limits: ChatLimits,
    pub model: ModelSettings,
    pub bounds: Bounds,
}

#[derive(Clone)]
pub struct AiServices {
    pub store: Arc<dyn JobStore>,
    pub clock: Arc<dyn Clock>,
    /// `None`: generation requests answer `NotConfigured`; completion summaries record `Absent`.
    pub llm: Option<Arc<dyn Llm>>,
    pub live: Arc<LiveHub>,
    /// The host's timer for the quiet-provider cadence.
    pub sleeper: Arc<dyn Sleeper>,
}

/// One caller-supplied history turn before validation (AI-010, AI-D06).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryTurn {
    pub role: String,
    pub content: String,
}

/// `POST /api/v1/ai/chat` body after authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRequest {
    pub message: String,
    pub history: Vec<HistoryTurn>,
}

/// An attachment: the durable snapshot first, then live frames when the
/// generation runs in this process (text kinds only).
pub struct Watch {
    pub snapshot: GenerationView,
    pub frames: Option<Subscription>,
    /// Held for the attachment's lifetime; dropping the watch (disconnect) frees it.
    _slot: WatchSlot,
}

pub struct Ai {
    s: AiServices,
    config: AiConfig,
    dispatched: Arc<AtomicUsize>,
    watches: Arc<std::sync::Mutex<Watches>>,
}

/// A dispatch slot of this process (D4); released on drop.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Live watch attachments of this process (D4): totals per process and account.
#[derive(Default)]
struct Watches {
    total: usize,
    per_account: std::collections::HashMap<AccountId, usize>,
}

/// A held watch slot not yet attached (see `Ai::reserve_watch`).
pub struct WatchReservation {
    slot: WatchSlot,
    account: AccountId,
}

struct WatchSlot {
    watches: Arc<std::sync::Mutex<Watches>>,
    account: AccountId,
}

impl Drop for WatchSlot {
    fn drop(&mut self) {
        let mut w = self.watches.lock().unwrap_or_else(|p| p.into_inner());
        w.total = w.total.saturating_sub(1);
        if let Some(n) = w.per_account.get_mut(&self.account) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                w.per_account.remove(&self.account);
            }
        }
    }
}

enum Wait<T> {
    Ready(T),
    Tick,
}

/// Polls `work` first, then `tick`; `work` stays alive across ticks, so a
/// pending provider read is never dropped and restarted.
async fn race<F: Future + ?Sized>(mut work: Pin<&mut F>, tick: impl Future<Output = ()>) -> Wait<F::Output> {
    let mut tick = std::pin::pin!(tick);
    std::future::poll_fn(move |cx| {
        if let Poll::Ready(v) = work.as_mut().poll(cx) {
            return Poll::Ready(Wait::Ready(v));
        }
        if tick.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wait::Tick);
        }
        Poll::Pending
    })
    .await
}

async fn finish<T>(uow: Box<dyn JobUow>, result: Result<T, AiError>) -> Result<T, AiError> {
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

/// Barrier first, then the billing account row (the existing account-lock domain).
async fn lock_account(uow: &mut dyn JobUow, account: AccountId) -> Result<(), AiError> {
    if uow.lock_barrier(account).await?.is_some() {
        return Err(AiError::Erased);
    }
    uow.billing().await?.lock_account(account).await?;
    Ok(())
}

fn micros(d: Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

fn identity(g: &Generation, token: Uuid) -> StreamIdentity {
    StreamIdentity {
        generation: g.id.to_string(),
        content_revision: u64::try_from(g.source_revision).unwrap_or(0),
        consent_epoch: u64::try_from(g.consent_epoch).unwrap_or(0),
        execution: token.to_string(),
    }
}

fn frame(id: &StreamIdentity, seq: i64, event: TextEvent) -> ApplicationFrame {
    ApplicationFrame { identity: id.clone(), seq: u64::try_from(seq).unwrap_or(0), event }
}

fn llm_interruption(e: &LlmError) -> (Interruption, bool) {
    let unknown = matches!(e.remote, RemoteOutcome::Unknown | RemoteOutcome::Finished);
    let reason = match &e.kind {
        ErrorKind::Timeout => Interruption::Deadline,
        ErrorKind::Limit => Interruption::Limit { message: "The AI response exceeded a size limit".into() },
        ErrorKind::Protocol => Interruption::Malformed { message: "The AI response could not be read".into() },
        other => Interruption::Provider { message: format!("{other:?}") },
    };
    (reason, unknown)
}

/// The worker's answer when a fenced step says stop, `None` to go on.
fn stopping(outcome: Result<ControlFlow<GenerationView>, AiError>) -> Option<Result<GenerationView, AiError>> {
    match outcome {
        Ok(ControlFlow::Continue(())) => None,
        Ok(ControlFlow::Break(view)) => Some(Ok(view)),
        Err(e) => Some(Err(e)),
    }
}

/// D4: the decoded content of a snapshot or final: prose text bytes, or the
/// serialized typed analysis. The outer JSON envelope and escaping are not counted.
fn content_len(draft: &Draft) -> usize {
    match draft {
        Draft::Empty => 0,
        Draft::Text { text } => text.len(),
        Draft::Analysis { progress } => serde_json::to_vec(progress).map_or(usize::MAX, |v| v.len()),
    }
}

fn final_len(content: &FinalContent) -> usize {
    match content {
        FinalContent::Text { text } => text.len(),
        FinalContent::Analysis { result } => serde_json::to_vec(result).map_or(usize::MAX, |v| v.len()),
    }
}

impl Ai {
    pub fn new(services: AiServices, config: AiConfig) -> Ai {
        Ai { s: services, config, dispatched: Arc::new(AtomicUsize::new(0)), watches: Arc::default() }
    }

    fn now(&self) -> Micros {
        self.s.clock.now()
    }

    fn try_slot(&self) -> Option<Slot> {
        let max = self.config.bounds.max_dispatch;
        self.dispatched
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < max).then_some(n + 1))
            .ok()
            .map(|_| Slot(Arc::clone(&self.dispatched)))
    }

    fn has_capacity(&self) -> bool {
        self.dispatched.load(Ordering::Acquire) < self.config.bounds.max_dispatch
    }

    /// A visible job of the account (not soft deleted).
    async fn visible_job(&self, account: AccountId, job: JobId) -> Result<Job, AiError> {
        let job = self.s.store.owned_job(account, job).await?.ok_or(AiError::NotFound)?;
        if job.status == JobStatus::Deleted {
            return Err(AiError::NotFound);
        }
        Ok(job)
    }

    /// One generation, read in its own transaction (no lock).
    async fn stored_generation(&self, id: GenerationId) -> Result<Option<Generation>, AiError> {
        let mut uow = self.s.store.begin().await?;
        let r = uow.ai().generation(id).await;
        let _ = uow.rollback().await;
        Ok(r?)
    }

    /// The account's consent, read in its own transaction (no lock).
    async fn stored_consent(&self, account: AccountId) -> Result<Option<ConsentRecord>, AiError> {
        let mut uow = self.s.store.begin().await?;
        let r = uow.ai().consent(account).await;
        let _ = uow.rollback().await;
        Ok(r?)
    }

    /// The job's newest summary request, read in its own transaction.
    async fn stored_summary_request(&self, job: JobId) -> Result<Option<SummaryRequest>, AiError> {
        let mut uow = self.s.store.begin().await?;
        let r = uow.ai().summary_request(job).await;
        let _ = uow.rollback().await;
        Ok(r?)
    }

    /// The account's generation for a request key, read in its own transaction.
    async fn stored_request(&self, account: AccountId, request_key: Uuid) -> Result<Option<Generation>, AiError> {
        let mut uow = self.s.store.begin().await?;
        let r = uow.ai().generation_by_request(account, request_key).await;
        let _ = uow.rollback().await;
        Ok(r?)
    }

    /// App definition 2 (FRE-014) inside the admission transaction. A failed
    /// plan query counts as freemium only when its savepoint leaves the
    /// transaction usable (D2); otherwise admission is unavailable.
    async fn is_freemium(uow: &mut dyn JobUow, account: AccountId, now: Micros) -> Result<bool, AiError> {
        uow.savepoint("ai_plan").await?;
        let packages = match uow.billing().await {
            Ok(mut b) => b.packages(account).await,
            Err(e) => Err(e),
        };
        match packages {
            Ok(rows) => {
                uow.release_savepoint("ai_plan").await?;
                let all: Vec<_> = rows.into_iter().map(|p| p.package).collect();
                Ok(is_freemium_app2(Some(&all), now))
            }
            Err(_) => {
                uow.rollback_to_savepoint("ai_plan").await.map_err(|e| AiError::Unavailable(e.to_string()))?;
                Ok(true)
            }
        }
    }

    // ---- consent and limits (no prior consent needed) ---------------------------

    /// `GET /api/ai/consent`.
    pub async fn consent(&self, account: AccountId) -> Result<ConsentView, AiError> {
        if self.s.store.barrier(account).await? {
            return Err(AiError::Erased);
        }
        let record = self.stored_consent(account).await?;
        Ok(consent::view(record.as_ref()))
    }

    /// `POST /api/ai/consent {hasConsented}` (AI-022): grant or refusal.
    /// Refusal fences in-flight work and keeps completed artifacts.
    pub async fn record_consent(&self, account: AccountId, has_consented: bool) -> Result<ConsentView, AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            lock_account(uow.as_mut(), account).await?;
            let previous = uow.ai().consent(account).await?;
            let next = consent::decide(previous, account, has_consented, now);
            uow.ai().save_consent(&next).await?;
            Ok(consent::view(Some(&next)))
        }
        .await;
        finish(uow, result).await
    }

    /// `DELETE /api/ai/consent` (AI-023, D3): commits refusal, a new epoch
    /// and the purge cutoff first, then purges. A failed purge stays pending
    /// (`cleanup_pending`) for `purge_pass`; permission is already withdrawn.
    pub async fn revoke_consent(&self, account: AccountId) -> Result<ConsentView, AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            lock_account(uow.as_mut(), account).await?;
            let previous = uow.ai().consent(account).await?;
            let next = consent::revoke(previous, account, now);
            uow.ai().save_consent(&next).await?;
            Ok(next)
        }
        .await;
        let revoked = finish(uow, result).await?;
        match self.purge(account).await {
            Ok(Some(done)) => Ok(consent::view(Some(&done))),
            _ => Ok(consent::view(Some(&revoked))),
        }
    }

    /// Purges every artifact at or below the cutoff; releases the reservation
    /// of an active metered generation it removes.
    async fn purge(&self, account: AccountId) -> Result<Option<ConsentRecord>, AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            lock_account(uow.as_mut(), account).await?;
            let Some(mut record) = uow.ai().consent(account).await? else { return Ok(None) };
            let Some(cutoff) = record.purge_cutoff.filter(|_| record.purge_pending) else { return Ok(Some(record)) };
            if let Some(active) = uow.ai().active_metered(account).await?
                && active.consent_epoch <= cutoff
                && let Some(reservation) = &active.quota
            {
                Self::release(uow.as_mut(), reservation).await?;
            }
            uow.ai().purge_ai_artifacts(account, cutoff).await?;
            record.purge_pending = false;
            record.updated_at = now;
            uow.ai().save_consent(&record).await?;
            Ok(Some(record))
        }
        .await;
        finish(uow, result).await
    }

    /// DELETE purges left pending by a failure.
    pub async fn purge_pass(&self) -> Result<AiPassReport, AiError> {
        let mut report = AiPassReport::default();
        for account in self.s.store.pending_ai_purges(100).await? {
            report.examined += 1;
            match self.purge(account).await {
                Ok(_) => report.completed += 1,
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    /// `GET /api/ai/chat-limits?jobId=` (API-BIL-011): read-only usage of the
    /// configured caps; paid callers have no limits; nothing is created or reset.
    pub async fn limits(&self, account: AccountId, job: JobId) -> Result<LimitsView, AiError> {
        if !self.config.chat_enabled {
            return Err(AiError::ChatDisabled);
        }
        self.visible_job(account, job).await?;
        let now = self.now();
        let limits = self.config.limits;
        let mut uow = self.s.store.begin().await?;
        let read = async {
            let freemium = Self::is_freemium(uow.as_mut(), account, now).await?;
            let rows = if freemium { uow.ai().counters(account, Some(job)).await? } else { Vec::new() };
            Ok::<_, AiError>((freemium, rows))
        }
        .await;
        let _ = uow.rollback().await;
        let (freemium, rows) = read?;
        if !freemium {
            return Ok(LimitsView { enabled: true, has_limits: false, ..LimitsView::default() });
        }
        let mut view = LimitsView { enabled: true, has_limits: limits.any(), ..LimitsView::default() };
        for (key, max) in limits.enabled(account, job) {
            let c = quota::current(rows.iter().find(|r| r.key == key), key, max, now);
            let transcript = matches!(key.scope, crate::ai::ports::CounterScope::Transcript(_));
            let questions = key.kind == crate::ai::ports::CounterKind::Questions;
            // The source compares the configured cap for transcript questions and
            // the stored maximum everywhere else.
            let exceeded = if transcript && questions { c.count >= max } else { c.max_limit.is_some_and(|m| c.count >= m) };
            let usage = LimitUsage {
                current: c.count,
                max,
                exceeded,
                reset_at: if transcript { None } else { c.reset_at },
                reset_period: (!transcript).then_some(limits.reset_period),
            };
            match (transcript, questions) {
                (true, true) => view.questions_per_transcript = Some(usage),
                (false, true) => view.questions_per_user = Some(usage),
                (true, false) => view.tokens_per_transcript = Some(usage),
                (false, false) => view.tokens_per_user = Some(usage),
            }
        }
        Ok(view)
    }

    // ---- admission --------------------------------------------------------------

    /// `POST /api/v1/ai/analyze` (AI-001): owned visible job, valid consent,
    /// transcript rows, features enabled, in the source's response order.
    pub async fn start_analysis(&self, account: AccountId, job: JobId, request_key: Uuid) -> Result<Started, AiError> {
        self.visible_job(account, job).await?;
        let consented = consent::is_valid(self.stored_consent(account).await?.as_ref());
        if !consented {
            return Err(AiError::ConsentRequired);
        }
        if self.s.store.lines(job).await?.is_empty() {
            return Err(AiError::NoTranscript);
        }
        if !self.config.features_enabled {
            return Err(AiError::Disabled);
        }
        self.admit(account, job, request_key, GenerationKind::Analysis, GenerationInput::Analysis, None).await
    }

    /// `POST /api/v1/ai/chat` (AI-010..016): route checks, consent, context,
    /// freemium caps, then the optional question validator, in source order.
    pub async fn start_chat(&self, account: AccountId, job: JobId, request_key: Uuid, chat: ChatRequest) -> Result<Started, AiError> {
        sanitize::validate_raw_question(&chat.message).map_err(|e| AiError::Invalid(e.message().into()))?;
        let mut history = Vec::with_capacity(chat.history.len());
        for turn in &chat.history {
            let role = match turn.role.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Assistant,
                _ => return Err(AiError::Invalid("Invalid chat history format".into())),
            };
            let content = sanitize::js_trim(&turn.content);
            if content.is_empty() {
                return Err(AiError::Invalid("Invalid chat history format".into()));
            }
            history.push(ChatTurn { role, content: content.to_owned() });
        }
        let job_row = self.visible_job(account, job).await?;
        let lines = self.s.store.lines(job).await?;
        if lines.is_empty() {
            return Err(AiError::Invalid("No transcript content found".into()));
        }
        if !self.config.features_enabled {
            return Err(AiError::Disabled);
        }
        if !self.config.chat_enabled {
            return Err(AiError::ChatDisabled);
        }
        // D5: the user message keeps the original accepted text as typed; the
        // provider and quota use the sanitized copy of its trimmed form (source route).
        let question = chat.message.clone();
        let sanitized_question = sanitize::input(sanitize::js_trim(&question));
        sanitize::validate_input_security(&sanitized_question)
            .map_err(|_| AiError::Invalid("Message validation failed: Content too long".into()))?;
        let context = sanitize::input(&sanitize::transcript_text(&lines, true));
        if sanitize::validate_chat_context(&context).is_err() {
            return Err(AiError::Invalid("No transcript content available for chat".into()));
        }
        let raw_context = sanitize::transcript_text(&lines, true);
        let validator = self.config.chat_validation.then(|| (sanitized_question.clone(), raw_context));
        let input = GenerationInput::Chat {
            question,
            sanitized_question,
            history,
            language: job_row.language.map(|l| l.code().to_owned()),
        };
        self.admit(account, job, request_key, GenerationKind::Chat, input, validator).await
    }

    /// A new generation repeating a terminal one's request (AI-009, D5):
    /// analysis starts over; chat resends the same question with the same
    /// history and appends another pair. New metered work, a new request key.
    pub async fn retry(&self, account: AccountId, id: GenerationId, request_key: Uuid) -> Result<Started, AiError> {
        let g = self.stored_generation(id).await?.filter(|g| g.account == account).ok_or(AiError::NotFound)?;
        if !g.status.is_terminal() {
            return Err(AiError::WrongState("Only a finished request can be retried".into()));
        }
        match g.input {
            GenerationInput::Analysis => self.start_analysis(account, g.job, request_key).await,
            GenerationInput::Chat { question, history, .. } => {
                let history = history
                    .into_iter()
                    .map(|t| HistoryTurn { role: if t.role == ChatRole::User { "user" } else { "assistant" }.into(), content: t.content })
                    .collect();
                self.start_chat(account, g.job, request_key, ChatRequest { message: question, history }).await
            }
            GenerationInput::CompletionSummary { .. } => Err(AiError::WrongState("A completion summary is not retried by request".into())),
        }
    }

    async fn admit(
        &self,
        account: AccountId,
        job: JobId,
        request_key: Uuid,
        kind: GenerationKind,
        input: GenerationInput,
        validator: Option<(String, String)>,
    ) -> Result<Started, AiError> {
        if self.s.llm.is_none() {
            return Err(AiError::NotConfigured);
        }
        if !self.has_capacity() {
            return Err(AiError::Capacity);
        }
        let now = self.now();
        let digest = generation::digest(kind, job, &input);
        let mut uow = self.s.store.begin().await?;
        let result: Result<Result<Started, AiError>, AiError> = async {
            lock_account(uow.as_mut(), account).await?;
            let record = uow.ai().consent(account).await?;
            if !consent::is_valid(record.as_ref()) {
                return Err(AiError::ConsentRequired);
            }
            if let Some(existing) = uow.ai().generation_by_request(account, request_key).await? {
                return if existing.request_digest == digest {
                    Ok(Ok(Started { generation: GenerationView::of(&existing), created: false }))
                } else {
                    Err(AiError::Conflict)
                };
            }
            if kind == GenerationKind::Analysis
                && let Some(active) = uow.ai().active_for_job(account, job, kind).await?
            {
                return Err(AiError::Busy(active.id));
            }
            let mut reservation: Option<QuotaReservation> = None;
            if let GenerationInput::Chat { sanitized_question, .. } = &input {
                let limits = self.config.limits;
                if limits.any() && Self::is_freemium(uow.as_mut(), account, now).await? {
                    if let Some(active) = uow.ai().active_metered(account).await? {
                        return Err(AiError::Busy(active.id));
                    }
                    let caps = limits.enabled(account, job);
                    let mut rows = Vec::with_capacity(caps.len());
                    for (key, _) in &caps {
                        rows.push(uow.ai().lock_counter(*key).await?);
                    }
                    let admission = quota::admit(&limits, &caps, rows, sanitized_question, now);
                    for c in &admission.counters {
                        uow.ai().save_counter(c).await?;
                    }
                    match admission.outcome {
                        // Counter creation and resets persist with the refusal, as the source's lookup does.
                        Err(refusal) => return Ok(Err(AiError::Quota(refusal))),
                        Ok(r) => reservation = Some(r),
                    }
                }
            }
            if let Some((question, raw_context)) = &validator {
                sanitize::validate_question(question, raw_context).map_err(|e| AiError::Invalid(e.message().into()))?;
            }
            let source_revision = uow.ai().source_revision(job).await?;
            let mut g = generation::accepted(
                NewGeneration { account, job, kind, request_key, source_revision, consent_epoch: consent::epoch(record.as_ref()), input: input.clone() },
                now,
            );
            g.quota = reservation;
            if let Err(e) = uow.ai().insert_generation(&g).await {
                return Err(match e {
                    // A unique key refused the insert (request key, or one active metered
                    // generation): decided after rollback, in a fresh transaction.
                    StoreError::Conflict(_) => AiError::Conflict,
                    other => other.into(),
                });
            }
            Ok(Ok(Started { generation: GenerationView::of(&g), created: true }))
        }
        .await;
        match result {
            Ok(answer) => match uow.commit().await {
                Ok(()) => answer,
                // A lost COMMIT answer is not rollback evidence: look the request up again.
                Err(e) => match self.stored_request(account, request_key).await {
                    Ok(Some(g)) if g.request_digest == digest => Ok(Started { generation: GenerationView::of(&g), created: true }),
                    _ => Err(AiError::Unavailable(e.to_string())),
                },
            },
            Err(AiError::Conflict) => {
                // Never read on the aborted transaction: roll back, then a fresh one.
                let _ = uow.rollback().await;
                let mut read = self.s.store.begin().await?;
                let found = async { Ok::<_, StoreError>((read.ai().generation_by_request(account, request_key).await?, read.ai().active_metered(account).await?)) }.await;
                let _ = read.rollback().await;
                let (existing, active) = found?;
                match (existing, active) {
                    (Some(g), _) if g.request_digest == digest => Ok(Started { generation: GenerationView::of(&g), created: false }),
                    (Some(_), _) => Err(AiError::Conflict),
                    (None, Some(active)) => Err(AiError::Busy(active.id)),
                    (None, None) => Err(AiError::Unavailable("the request could not be admitted; retry".into())),
                }
            }
            Err(e) => {
                let _ = uow.rollback().await;
                Err(e)
            }
        }
    }

    // ---- reads ------------------------------------------------------------------

    async fn readable(&self, account: AccountId, id: GenerationId) -> Result<Generation, AiError> {
        if self.s.store.barrier(account).await? {
            return Err(AiError::Erased);
        }
        let mut uow = self.s.store.begin().await?;
        let read = async { Ok::<_, StoreError>((uow.ai().consent(account).await?, uow.ai().generation(id).await?)) }.await;
        let _ = uow.rollback().await;
        let (record, g) = read?;
        let g = g.filter(|g| g.account == account).ok_or(AiError::NotFound)?;
        if !consent::may_read(record.as_ref(), g.consent_epoch) {
            return Err(AiError::ConsentRequired);
        }
        Ok(g)
    }

    /// One generation's public snapshot.
    pub async fn generation(&self, account: AccountId, id: GenerationId) -> Result<GenerationView, AiError> {
        Ok(GenerationView::of(&self.readable(account, id).await?))
    }

    /// Attaches to a generation: its durable snapshot, plus live frames when it
    /// runs in this process. Detaching never cancels or regenerates.
    pub async fn watch(&self, account: AccountId, id: GenerationId) -> Result<Watch, AiError> {
        let reservation = self.reserve_watch(account)?;
        self.attach(reservation, account, id).await
    }

    /// A watch slot taken before starting a generation that will be watched at
    /// once (the legacy chat and analysis POSTs), so a full registry refuses
    /// before admission instead of after it. Dropping it frees the slot.
    pub fn reserve_watch(&self, account: AccountId) -> Result<WatchReservation, AiError> {
        Ok(WatchReservation { slot: self.watch_slot(account)?, account })
    }

    /// Attaches with a slot reserved by `reserve_watch` for the same account.
    pub async fn attach(&self, reservation: WatchReservation, account: AccountId, id: GenerationId) -> Result<Watch, AiError> {
        if reservation.account != account {
            return Err(AiError::NotFound);
        }
        let g = self.readable(account, id).await?;
        let frames = (g.kind != GenerationKind::Analysis && !g.status.is_terminal()).then(|| self.s.live.subscribe(id)).flatten();
        Ok(Watch { snapshot: GenerationView::of(&g), frames, _slot: reservation.slot })
    }

    /// D4: at most `watches_per_process` and `watches_per_account` live
    /// attachments; beyond, an immediate refusal (no queue).
    fn watch_slot(&self, account: AccountId) -> Result<WatchSlot, AiError> {
        let b = &self.config.bounds;
        let mut w = self.watches.lock().unwrap_or_else(|p| p.into_inner());
        let mine = w.per_account.get(&account).copied().unwrap_or(0);
        if w.total >= b.watches_per_process || mine >= b.watches_per_account {
            return Err(AiError::Capacity);
        }
        w.total += 1;
        w.per_account.insert(account, mine + 1);
        Ok(WatchSlot { watches: Arc::clone(&self.watches), account })
    }

    /// The job's current analysis (AI-007): the newest analysis generation,
    /// pending/partial/interrupted included, unless a DELETE cutoff covers it.
    pub async fn analysis(&self, account: AccountId, job: JobId) -> Result<Option<GenerationView>, AiError> {
        self.visible_job(account, job).await?;
        let mut uow = self.s.store.begin().await?;
        let read = async { Ok::<_, StoreError>((uow.ai().consent(account).await?, uow.ai().latest_analysis(account, job).await?)) }.await;
        let _ = uow.rollback().await;
        let (record, g) = read?;
        if !consent::is_valid(record.as_ref()) {
            return Err(AiError::ConsentRequired);
        }
        Ok(g.filter(|g| !consent::covered(record.as_ref(), g.consent_epoch)).map(|g| GenerationView::of(&g)))
    }

    /// The job's persisted chat (AI-021), in order.
    pub async fn chat_history(&self, account: AccountId, job: JobId) -> Result<Vec<ChatMessage>, AiError> {
        self.visible_job(account, job).await?;
        let mut uow = self.s.store.begin().await?;
        let read = async { Ok::<_, StoreError>((uow.ai().consent(account).await?, uow.ai().chat_history(account, job).await?)) }.await;
        let _ = uow.rollback().await;
        let (record, messages) = read?;
        if !consent::is_valid(record.as_ref()) {
            return Err(AiError::ConsentRequired);
        }
        Ok(messages.into_iter().filter(|m| !consent::covered(record.as_ref(), m.consent_epoch)).collect())
    }

    /// The general completion summary an owner reads (TRX-039).
    pub async fn completion_summary(&self, account: AccountId, job: JobId) -> Result<Option<SummaryView>, AiError> {
        self.visible_job(account, job).await?;
        let mut uow = self.s.store.begin().await?;
        let read = async { Ok::<_, StoreError>((uow.ai().consent(account).await?, uow.ai().completion_summary(account, job).await?)) }.await;
        let _ = uow.rollback().await;
        let (record, summary) = read?;
        if !consent::is_valid(record.as_ref()) {
            return Ok(None);
        }
        Ok(summary
            .filter(|s| !consent::covered(record.as_ref(), s.consent_epoch))
            .map(|s| SummaryView { job: s.job, source_revision: s.source_revision, text: s.text, created_at: s.created_at }))
    }

    /// The callback ordering boundary (JOB-033, D5): `None` while the job's
    /// newest summary request has no outcome (callback assembly waits); a job
    /// without any request reads as `Absent`. Never changes the job.
    pub async fn summary_outcome(&self, job: JobId) -> Result<Option<SummaryOutcome>, AiError> {
        let request = self.stored_summary_request(job).await?;
        Ok(match request {
            None => Some(SummaryOutcome::Absent { reason: "no summary was requested".into() }),
            Some(r) => r.outcome,
        })
    }

    // ---- cancellation -----------------------------------------------------------

    /// The owner cancels (idempotent). Local consumption stops at the worker's
    /// next fence; nothing proves remote cancellation. Completed stays completed.
    pub async fn cancel(&self, account: AccountId, id: GenerationId) -> Result<GenerationView, AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            lock_account(uow.as_mut(), account).await?;
            let mut g = uow.ai().lock_generation(id).await?.filter(|g| g.account == account).ok_or(AiError::NotFound)?;
            let reservation = g.quota.clone();
            if generation::cancel(&mut g, now) {
                if let Some(r) = &reservation {
                    Self::release(uow.as_mut(), r).await?;
                }
                if g.kind == GenerationKind::CompletionSummary {
                    Self::decide_summary(uow.as_mut(), &g, SummaryOutcome::Failed { reason: "cancelled".into() }, now).await?;
                }
                uow.ai().save_generation(&g).await?;
            }
            Ok(GenerationView::of(&g))
        }
        .await;
        finish(uow, result).await
    }

    // ---- quota settlement -------------------------------------------------------

    async fn release(uow: &mut dyn JobUow, reservation: &QuotaReservation) -> Result<(), AiError> {
        for bucket in &reservation.buckets {
            if let Some(c) = uow.ai().lock_counter(bucket.key).await?
                && let Some(next) = quota::release(c, bucket)
            {
                uow.ai().save_counter(&next).await?;
            }
        }
        Ok(())
    }

    async fn settle(uow: &mut dyn JobUow, reservation: &QuotaReservation, sanitized_question: &str, answer: &str) -> Result<(), AiError> {
        for bucket in &reservation.buckets {
            if let Some(c) = uow.ai().lock_counter(bucket.key).await?
                && let Some(next) = quota::settle(c, bucket, quota::actual(bucket, sanitized_question, answer))
            {
                uow.ai().save_counter(&next).await?;
            }
        }
        Ok(())
    }

    // ---- worker -----------------------------------------------------------------

    async fn fences(&self, uow: &mut dyn JobUow, g: &Generation) -> Result<Fences, AiError> {
        let record = uow.ai().consent(g.account).await?;
        let job = self.s.store.owned_job(g.account, g.job).await?;
        let revision = uow.ai().source_revision(g.job).await?;
        Ok(Fences {
            erasing: false,
            source_present: job.is_some_and(|j| j.status != JobStatus::Deleted),
            source_revision: revision,
            consent_current: consent::may_write(record.as_ref(), g.consent_epoch),
        })
    }

    /// Runs one generation: claims it, streams the provider outside any
    /// transaction, checkpoints sanitized snapshots, stages the sanitized final
    /// and completes atomically. A running generation whose lease expired is
    /// recovered instead: a staged final completes without any provider call;
    /// otherwise it is interrupted with its remote outcome unknown.
    pub async fn run(&self, id: GenerationId) -> Result<GenerationView, AiError> {
        let now = self.now();
        let current = self.stored_generation(id).await?.ok_or(AiError::NotFound)?;
        if current.status.is_terminal() {
            return Ok(GenerationView::of(&current));
        }
        if current.status == GenerationStatus::Running {
            if !generation::lease_expired(&current, now) {
                return Ok(GenerationView::of(&current));
            }
            return self.recover(id).await;
        }
        let Some(llm) = self.s.llm.clone() else { return Err(AiError::NotConfigured) };
        let Some(slot) = self.try_slot() else { return Ok(GenerationView::of(&current)) };
        let token = Uuid::new_v4();
        let lease = micros(self.config.bounds.lease);
        // ---- claim ----
        let mut uow = self.s.store.begin().await?;
        let claimed: Result<Result<Generation, Generation>, AiError> = async {
            if uow.lock_barrier(current.account).await?.is_some() {
                return Err(AiError::Erased);
            }
            uow.billing().await?.lock_account(current.account).await?;
            let Some(mut g) = uow.ai().lock_generation(id).await? else { return Err(AiError::NotFound) };
            if g.status != GenerationStatus::Accepted {
                return Ok(Err(g));
            }
            let f = self.fences(uow.as_mut(), &g).await?;
            if let Some(reason) = generation::fenced(&g, &f) {
                self.end(uow.as_mut(), &mut g, reason, false, now).await?;
                return Ok(Err(g));
            }
            generation::claim(&mut g, token, Micros(now.0 + lease), now).map_err(|e| AiError::WrongState(format!("{e:?}")))?;
            uow.ai().save_generation(&g).await?;
            // D3/D9: the payload-free intent is durable before any provider I/O.
            let dispatch = LlmDispatch {
                account: g.account,
                generation: g.id,
                provider: self.config.model.provider.clone(),
                model: self.config.model.model.clone(),
                disposition: DispatchDisposition::Possible,
                remote_ids: Vec::new(),
                unkept_id_observed: false,
                created_at: now,
            };
            uow.ai().record_dispatch(&dispatch).await?;
            Ok(Ok(g))
        }
        .await;
        let g = match finish(uow, claimed).await {
            Ok(Ok(g)) => g,
            Ok(Err(other)) => return Ok(GenerationView::of(&other)),
            Err(AiError::Erased) => return Err(AiError::Erased),
            Err(e) => return Err(e),
        };
        let outcome = self.stream(llm.as_ref(), g, token).await;
        drop(slot);
        outcome
    }

    async fn stream(&self, llm: &dyn Llm, mut g: Generation, token: Uuid) -> Result<GenerationView, AiError> {
        let lines = self.s.store.lines(g.job).await?;
        if lines.is_empty() {
            let _ = self.observe(g.account, g.id, DispatchDisposition::NotDispatched, None).await;
            return self.interrupt_owned(g.id, token, Interruption::SourceChanged, false, None).await;
        }
        let request = match self.request(&g, &lines) {
            Ok(r) => r,
            Err(reason) => {
                let _ = self.observe(g.account, g.id, DispatchDisposition::NotDispatched, None).await;
                return self.interrupt_owned(g.id, token, reason, false, None).await;
            }
        };
        let stream_id = identity(&g, token);
        let publisher = (g.kind != GenerationKind::Analysis).then(|| self.s.live.open(g.id, stream_id.clone()));
        // D4: the fence cadence also runs while the provider's headers are pending.
        let opened = {
            let mut open = llm.open(request);
            loop {
                match race(open.as_mut(), self.s.sleeper.sleep(self.config.bounds.quiet_millis)).await {
                    Wait::Ready(opened) => break opened,
                    Wait::Tick => {
                        if let Some(stop) = stopping(self.probe(&g, token, publisher.as_ref().map(|p| (p, &stream_id))).await) {
                            return stop;
                        }
                    }
                }
            }
        };
        let mut stream = match opened {
            Ok(s) => s,
            Err(e) => {
                let disposition = if e.remote == RemoteOutcome::NotDispatched { DispatchDisposition::NotDispatched } else { DispatchDisposition::Possible };
                let _ = self.observe(g.account, g.id, disposition, e.remote_id.as_deref()).await;
                let (reason, unknown) = llm_interruption(&e);
                return self.interrupt_owned(g.id, token, reason, unknown, publisher.as_ref().map(|p| (p, &stream_id))).await;
            }
        };
        let output_limit = self.config.bounds.llm.output_bytes;
        let mut raw = String::new();
        let mut parser = (g.kind == GenerationKind::Analysis).then(AnalysisParser::new);
        let mut seq = g.sequence;
        let mut checkpointed_len = 0usize;
        let mut checkpointed_sections = 0usize;
        let finish_reason = loop {
            let event = {
                let mut next = stream.next();
                loop {
                    match race(next.as_mut(), self.s.sleeper.sleep(self.config.bounds.quiet_millis)).await {
                        Wait::Ready(event) => break event,
                        Wait::Tick => {
                            // A quiet provider: save unsaved prose as a sanitized
                            // replacement, or re-read the fences, without waiting
                            // for the next token.
                            let live = publisher.as_ref().map(|p| (p, &stream_id));
                            let outcome = if parser.is_none() && raw.len() > checkpointed_len {
                                checkpointed_len = raw.len();
                                let text = sanitize::output(&raw);
                                let outcome = self.checkpoint(&mut g, token, Draft::Text { text }, seq, live).await;
                                seq = g.sequence;
                                outcome
                            } else {
                                self.probe(&g, token, live).await
                            };
                            if let Some(stop) = stopping(outcome) {
                                drop(next);
                                stream.cancel();
                                return stop;
                            }
                        }
                    }
                }
            };
            match event {
                Ok(Some(CompletionEvent::Delta(text))) => {
                    if raw.len() + text.len() > output_limit {
                        stream.cancel();
                        let reason = Interruption::Limit { message: "The AI response exceeded 1 MiB".into() };
                        return self.interrupt_owned(g.id, token, reason, true, publisher.as_ref().map(|p| (p, &stream_id))).await;
                    }
                    raw.push_str(&text);
                    if let Some(p) = parser.as_mut() {
                        if let Err(e) = p.push(&text) {
                            stream.cancel();
                            let partial = p.progress().clone();
                            return self.interrupt_with_partial(g.id, token, Interruption::Malformed { message: format!("{e:?}") }, partial).await;
                        }
                        let progress = p.progress();
                        let sections = progress.content_sections + usize::from(progress.detected.is_some());
                        if sections > checkpointed_sections {
                            checkpointed_sections = sections;
                            let draft = Draft::Analysis { progress: Box::new(progress.clone()) };
                            if let Some(stop) = stopping(self.checkpoint(&mut g, token, draft, seq, None).await) {
                                stream.cancel();
                                return stop;
                            }
                            seq = g.sequence;
                        }
                    } else {
                        seq += 1;
                        if let Some(p) = &publisher {
                            p.send(&frame(&stream_id, seq, TextEvent::Delta(text)));
                        }
                        if raw.len() - checkpointed_len >= self.config.bounds.checkpoint_bytes {
                            checkpointed_len = raw.len();
                            let text = sanitize::output(&raw);
                            let live = publisher.as_ref().map(|p| (p, &stream_id));
                            if let Some(stop) = stopping(self.checkpoint(&mut g, token, Draft::Text { text }, seq, live).await) {
                                stream.cancel();
                                return stop;
                            }
                            seq = g.sequence;
                        }
                    }
                }
                Ok(Some(CompletionEvent::Metadata { remote_id, .. })) => {
                    if let Some(id) = remote_id {
                        // Durable at once: it survives the cascade on the erasure item if needed.
                        let _ = self.observe(g.account, g.id, DispatchDisposition::Possible, Some(&id)).await;
                        g.remote_id = Some(id);
                    }
                }
                Ok(Some(CompletionEvent::Finished(done))) => {
                    if let Some(id) = &done.remote_id {
                        let _ = self.observe(g.account, g.id, DispatchDisposition::Possible, Some(id)).await;
                        g.remote_id = Some(id.clone());
                    }
                    break done.reason;
                }
                Ok(None) => {
                    let reason = Interruption::Provider { message: "The AI stream ended without a finish".into() };
                    return self.interrupt_owned(g.id, token, reason, true, publisher.as_ref().map(|p| (p, &stream_id))).await;
                }
                Err(e) => {
                    let (reason, unknown) = llm_interruption(&e);
                    return self.interrupt_owned(g.id, token, reason, unknown, publisher.as_ref().map(|p| (p, &stream_id))).await;
                }
            }
        };
        let success = finish_reason.is_success();
        let content = match parser {
            Some(mut p) => match p.finish(success) {
                Ok(result) => FinalContent::Analysis { result: Box::new(result) },
                Err(error) => {
                    let reason = if success {
                        Interruption::Malformed { message: format!("{error:?}") }
                    } else {
                        Interruption::Unsuccessful { reason: format!("{finish_reason:?}") }
                    };
                    // The parser keeps the validated partial after a failed finish.
                    return self.interrupt_with_partial(g.id, token, reason, p.into_progress()).await;
                }
            },
            None if !success => {
                let reason = Interruption::Unsuccessful { reason: format!("{finish_reason:?}") };
                return self.interrupt_owned(g.id, token, reason, false, publisher.as_ref().map(|p| (p, &stream_id))).await;
            }
            None => FinalContent::Text { text: sanitize::output(&raw) },
        };
        if final_len(&content) > output_limit {
            let reason = Interruption::Limit { message: "The AI result exceeded 1 MiB".into() };
            return self.interrupt_owned(g.id, token, reason, false, publisher.as_ref().map(|p| (p, &stream_id))).await;
        }
        let tokens_used = match (&g.input, &content) {
            // D8: the UTF-16 estimate of the canonical transcript actually submitted.
            (GenerationInput::Analysis, _) => Some(i64::try_from(prompts::analysis_input_estimated_tokens(&lines)).unwrap_or(i64::MAX)),
            (GenerationInput::Chat { sanitized_question, .. }, FinalContent::Text { text }) => {
                Some(quota::estimate_tokens(&format!("{sanitized_question}{text}")))
            }
            _ => None,
        };
        // ---- stage, then complete atomically ----
        let staged = self.stage(g.id, token, content, g.remote_id.clone(), tokens_used).await;
        let staged = match staged {
            Ok(Ok(g)) => g,
            Ok(Err(view)) => return Ok(view),
            Err(_) => {
                // The storage cause stays private: it may carry row or payload details.
                let reason = Interruption::NotSaved { message: "The answer could not be saved. Reconnect to see its saved state.".into() };
                return self.interrupt_owned(g.id, token, reason, false, publisher.as_ref().map(|p| (p, &stream_id))).await;
            }
        };
        let view = self.complete(staged.id, Some(token), seq).await?;
        if let (Some(p), Some(FinalContent::Text { text })) = (&publisher, &view.final_content) {
            let persistence = if view.status == GenerationStatus::Completed { Persistence::Saved } else { Persistence::Unsaved };
            p.send(&frame(&stream_id, view.sequence, TextEvent::Final { text: text.clone(), persistence }));
        }
        Ok(view)
    }

    fn request(&self, g: &Generation, lines: &[Line]) -> Result<CompletionRequest, Interruption> {
        let m = &self.config.model;
        let (messages, format) = match &g.input {
            GenerationInput::Analysis => {
                let p = prompts::analysis_messages(lines);
                let format = m.analysis_json_mode.then_some(ResponseFormat::JsonObject);
                (vec![Message { role: Role::System, content: p.system }, Message { role: Role::User, content: p.user }], format)
            }
            GenerationInput::CompletionSummary { detailed } => {
                let p = prompts::summary_messages(lines, *detailed);
                (vec![Message { role: Role::System, content: p.system }, Message { role: Role::User, content: p.user }], None)
            }
            GenerationInput::Chat { sanitized_question, history, language, .. } => {
                let context = sanitize::input(&sanitize::transcript_text(lines, true));
                let mut messages = Vec::with_capacity(history.len() + 2);
                messages.push(Message { role: Role::System, content: prompts::chat_system_message(&context, language.as_deref()) });
                for turn in history {
                    let role = if turn.role == ChatRole::User { Role::User } else { Role::Assistant };
                    messages.push(Message { role, content: sanitize::input(&turn.content) });
                }
                messages.push(Message { role: Role::User, content: sanitized_question.clone() });
                (messages, None)
            }
        };
        let input_bytes: usize = messages.iter().map(|m| m.content.len()).sum();
        if input_bytes > self.config.bounds.llm.input_bytes {
            return Err(Interruption::Limit { message: "The AI request exceeded 16 MiB".into() });
        }
        let mut request = CompletionRequest::new(m.model.clone(), messages);
        request.temperature = m.temperature;
        request.max_output_tokens = m.max_output_tokens;
        request.response_format = format;
        request.timeout = self.config.bounds.deadline;
        request.limits = self.config.bounds.llm;
        Ok(request)
    }

    /// A fenced checkpoint transaction. `Break` carries the generation's view
    /// when it must stop (fenced, cancelled or lost); `Err` is a real failure.
    async fn checkpoint(
        &self,
        g: &mut Generation,
        token: Uuid,
        draft: Draft,
        seq_floor: i64,
        live: Option<(&crate::ai::live::Publisher, &StreamIdentity)>,
    ) -> Result<ControlFlow<GenerationView>, AiError> {
        if content_len(&draft) > self.config.bounds.llm.output_bytes {
            let reason = Interruption::Limit { message: "The AI progress exceeded 1 MiB".into() };
            return self.interrupt_owned(g.id, token, reason, true, live).await.map(ControlFlow::Break);
        }
        let now = self.now();
        let lease = Micros(now.0 + micros(self.config.bounds.lease));
        let mut uow = self.s.store.begin().await?;
        let result: Result<Result<Generation, Generation>, AiError> = async {
            if uow.lock_barrier(g.account).await?.is_some() {
                return Err(AiError::Erased);
            }
            uow.billing().await?.lock_account(g.account).await?;
            let Some(mut row) = uow.ai().lock_generation(g.id).await? else { return Err(AiError::NotFound) };
            if generation::owns(&row, token).is_err() {
                return Ok(Err(row));
            }
            let f = self.fences(uow.as_mut(), &row).await?;
            if let Some(reason) = generation::fenced(&row, &f) {
                self.end(uow.as_mut(), &mut row, reason, true, now).await?;
                return Ok(Err(row));
            }
            generation::checkpoint(&mut row, token, draft.clone(), seq_floor, lease, now).map_err(|e| AiError::WrongState(format!("{e:?}")))?;
            uow.ai().save_generation(&row).await?;
            Ok(Ok(row))
        }
        .await;
        match finish(uow, result).await? {
            Ok(row) => {
                if let (Some((p, id)), Draft::Text { text }) = (live, &row.draft) {
                    p.send(&frame(id, row.sequence, TextEvent::Snapshot { text: text.clone(), status: StreamStatus::Streaming, persistence: Persistence::Provisional }));
                }
                *g = row;
                Ok(ControlFlow::Continue(()))
            }
            Err(stopped) => {
                if let Some((p, id)) = live {
                    let event = match &stopped.interruption {
                        Some(Interruption::Revoked) => TextEvent::Revoked,
                        Some(reason) => TextEvent::Interrupted(public_error(reason)),
                        None => TextEvent::Interrupted(PublicError { code: "cancelled".into(), message: "Cancelled".into() }),
                    };
                    p.send(&frame(id, stopped.sequence, event));
                }
                Ok(ControlFlow::Break(GenerationView::of(&stopped)))
            }
        }
    }

    /// D3/D9: records a dispatch disposition or an observed provider id. Before
    /// the account's cascade the intent row takes it (account lock, or barrier
    /// lock once erasure began); after it, only the matching erasure item's
    /// identity changes (attempts bumped, a closed erasure reopened when new
    /// dispatch evidence appears). Nothing local is ever recreated.
    async fn observe(&self, account: AccountId, generation: GenerationId, disposition: DispatchDisposition, observed: Option<&str>) -> Result<(), AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut record) = uow.lock_barrier(account).await? else {
                uow.billing().await?.lock_account(account).await?;
                uow.ai().settle_dispatch(account, generation, disposition, observed).await?;
                return Ok(());
            };
            if uow.ai().settle_dispatch(account, generation, disposition, observed).await? {
                return Ok(());
            }
            let items = uow.erasure_items(record.id).await?;
            let Some(mut item) = items
                .into_iter()
                .find(|i| matches!(&i.target, ItemTarget::LlmDispatch { generation: g, .. } if *g == generation.0))
            else {
                return Ok(());
            };
            let ItemTarget::LlmDispatch { generation: g, provider, remote_ids, unkept_id_observed } = item.target.clone() else { return Ok(()) };
            let was_absent = matches!(item.state, ItemState::Absent { .. });
            let mut d = LlmDispatch {
                account,
                generation,
                provider: provider.clone(),
                model: String::new(),
                disposition: if was_absent { DispatchDisposition::NotDispatched } else { DispatchDisposition::Possible },
                remote_ids: remote_ids.clone(),
                unkept_id_observed,
                created_at: now,
            };
            d.settle(disposition, observed);
            let state = match (d.disposition, &item.state) {
                // `settle` yields NotDispatched only without any kept or unkept evidence.
                (DispatchDisposition::NotDispatched, ItemState::Unresolved { .. }) => ItemState::Absent { at: now },
                (DispatchDisposition::Possible, ItemState::Absent { .. }) => {
                    ItemState::Unresolved { reason: erasure::LLM_RETENTION_UNVERIFIED.into(), next_at: now }
                }
                (_, s) => s.clone(),
            };
            if d.remote_ids == remote_ids && d.unkept_id_observed == unkept_id_observed && state == item.state {
                return Ok(());
            }
            let reopened = was_absent && !matches!(state, ItemState::Absent { .. });
            item.target = ItemTarget::LlmDispatch { generation: g, provider, remote_ids: d.remote_ids, unkept_id_observed: d.unkept_id_observed };
            item.state = state;
            item.attempts += 1;
            uow.save_erasure_item_target(&item).await?;
            if reopened && matches!(record.state, ErasureState::FullyErased | ErasureState::ErasedWithExceptions) {
                record.state = ErasureState::LocalErased;
                record.closed_at = None;
                uow.save_erasure(&record).await?;
            }
            Ok(())
        }
        .await;
        finish(uow, result).await
    }

    /// A quiet-provider fence check without a write: `Break` with the view
    /// when the worker lost its claim (cancelled, recovered) or a fence fails.
    async fn probe(&self, g: &Generation, token: Uuid, live: Option<(&crate::ai::live::Publisher, &StreamIdentity)>) -> Result<ControlFlow<GenerationView>, AiError> {
        let erasing = self.s.store.barrier(g.account).await?;
        let mut uow = self.s.store.begin().await?;
        let checked = async {
            let Some(row) = uow.ai().generation(g.id).await? else { return Err(AiError::NotFound) };
            if generation::owns(&row, token).is_err() {
                return Ok(Err(row));
            }
            let f = self.fences(uow.as_mut(), &row).await?;
            Ok(Ok(generation::fenced(&row, &Fences { erasing, ..f })))
        }
        .await;
        let _ = uow.rollback().await;
        match checked? {
            Ok(None) => Ok(ControlFlow::Continue(())),
            Ok(Some(reason)) => self.interrupt_owned(g.id, token, reason, true, live).await.map(ControlFlow::Break),
            Err(row) => Ok(ControlFlow::Break(GenerationView::of(&row))),
        }
    }

    async fn stage(
        &self,
        id: GenerationId,
        token: Uuid,
        content: FinalContent,
        remote_id: Option<String>,
        tokens_used: Option<i64>,
    ) -> Result<Result<Generation, GenerationView>, AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(peek) = uow.ai().generation(id).await? else { return Err(AiError::NotFound) };
            lock_account(uow.as_mut(), peek.account).await?;
            let Some(mut row) = uow.ai().lock_generation(id).await? else { return Err(AiError::NotFound) };
            if generation::owns(&row, token).is_err() {
                return Ok(Err(GenerationView::of(&row)));
            }
            let f = self.fences(uow.as_mut(), &row).await?;
            if let Some(reason) = generation::fenced(&row, &f) {
                self.end(uow.as_mut(), &mut row, reason, true, now).await?;
                return Ok(Err(GenerationView::of(&row)));
            }
            row.remote_id = remote_id;
            row.tokens_used = tokens_used;
            generation::stage(&mut row, token, content, now).map_err(|e| AiError::WrongState(format!("{e:?}")))?;
            uow.ai().save_generation(&row).await?;
            Ok(Ok(row))
        }
        .await;
        finish(uow, result).await
    }

    /// The completing transaction: artifact, settlement and completion at once.
    /// `token` is `None` for recovery of a lost worker's staged final. An
    /// unknown COMMIT is reconciled by reading the generation back; a still
    /// staged final is completed again, never re-requested from the provider.
    async fn complete(&self, id: GenerationId, token: Option<Uuid>, seq_floor: i64) -> Result<GenerationView, AiError> {
        for _attempt in 0..2 {
            let now = self.now();
            let mut uow = self.s.store.begin().await?;
            let result = async {
                let Some(peek) = uow.ai().generation(id).await? else { return Err(AiError::NotFound) };
                lock_account(uow.as_mut(), peek.account).await?;
                let Some(mut row) = uow.ai().lock_generation(id).await? else { return Err(AiError::NotFound) };
                if row.status.is_terminal() || row.staged.is_none() {
                    return Ok(row);
                }
                if let Some(t) = token
                    && generation::owns(&row, t).is_err()
                {
                    return Ok(row);
                }
                let f = self.fences(uow.as_mut(), &row).await?;
                if let Some(reason) = generation::fenced(&row, &f) {
                    self.end(uow.as_mut(), &mut row, reason, true, now).await?;
                    return Ok(row);
                }
                self.save_artifact(uow.as_mut(), &row, now).await?;
                row.sequence = row.sequence.max(seq_floor);
                generation::complete(&mut row, now).map_err(|e| AiError::WrongState(format!("{e:?}")))?;
                uow.ai().save_generation(&row).await?;
                Ok(row)
            }
            .await;
            match result {
                Ok(row) => match uow.commit().await {
                    Ok(()) => return Ok(GenerationView::of(&row)),
                    Err(_) => {
                        let back = self.stored_generation(id).await?;
                        match back {
                            Some(g) if g.status.is_terminal() => return Ok(GenerationView::of(&g)),
                            Some(_) => continue,
                            None => return Err(AiError::NotFound),
                        }
                    }
                },
                Err(e) => {
                    let _ = uow.rollback().await;
                    return Err(e);
                }
            }
        }
        // Still staged: the final is durable; recovery completes it after the lease.
        let g = self.stored_generation(id).await?.ok_or(AiError::NotFound)?;
        Ok(GenerationView::of(&g))
    }

    async fn save_artifact(&self, uow: &mut dyn JobUow, g: &Generation, now: Micros) -> Result<(), AiError> {
        match (&g.input, &g.staged) {
            (GenerationInput::Chat { question, sanitized_question, .. }, Some(FinalContent::Text { text })) => {
                if uow.ai().chat_pair(g.id).await?.is_none() {
                    let message = |role, content: &str| ChatMessage {
                        id: Uuid::new_v4(),
                        account: g.account,
                        job: g.job,
                        generation: g.id,
                        order: 0,
                        role,
                        content: content.to_owned(),
                        consent_epoch: g.consent_epoch,
                        created_at: now,
                    };
                    uow.ai().append_chat_pair(&message(ChatRole::User, question), &message(ChatRole::Assistant, text)).await?;
                }
                if let Some(reservation) = &g.quota {
                    Self::settle(uow, reservation, sanitized_question, text).await?;
                }
            }
            (GenerationInput::CompletionSummary { .. }, Some(FinalContent::Text { text })) => {
                let summary = CompletionSummary {
                    account: g.account,
                    job: g.job,
                    source_revision: g.source_revision,
                    generation: g.id,
                    text: text.clone(),
                    consent_epoch: g.consent_epoch,
                    created_at: now,
                };
                uow.ai().save_completion_summary(&summary).await?;
                Self::decide_summary(uow, g, SummaryOutcome::Saved { generation: g.id }, now).await?;
            }
            (GenerationInput::Analysis, Some(FinalContent::Analysis { .. })) => {}
            _ => return Err(AiError::WrongState("staged content does not match the request".into())),
        }
        Ok(())
    }

    /// Ends a generation in the caller's transaction: releases its reservation
    /// and settles a completion summary request as failed.
    async fn end(&self, uow: &mut dyn JobUow, g: &mut Generation, reason: Interruption, dispatched: bool, now: Micros) -> Result<(), AiError> {
        let reservation = g.quota.clone();
        let summary_reason = format!("{reason:?}");
        if generation::interrupt(g, reason, dispatched, now) {
            if let Some(r) = &reservation {
                Self::release(uow, r).await?;
            }
            if g.kind == GenerationKind::CompletionSummary {
                Self::decide_summary(uow, g, SummaryOutcome::Failed { reason: summary_reason }, now).await?;
            }
            uow.ai().save_generation(g).await?;
        }
        Ok(())
    }

    async fn interrupt_owned(
        &self,
        id: GenerationId,
        token: Uuid,
        reason: Interruption,
        remote_unknown: bool,
        live: Option<(&crate::ai::live::Publisher, &StreamIdentity)>,
    ) -> Result<GenerationView, AiError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(peek) = uow.ai().generation(id).await? else { return Err(AiError::NotFound) };
            if uow.lock_barrier(peek.account).await?.is_some() {
                return Err(AiError::Erased);
            }
            uow.billing().await?.lock_account(peek.account).await?;
            let Some(mut row) = uow.ai().lock_generation(id).await? else { return Err(AiError::NotFound) };
            if generation::owns(&row, token).is_ok() {
                self.end(uow.as_mut(), &mut row, reason, remote_unknown, now).await?;
            }
            Ok(row)
        }
        .await;
        let row = finish(uow, result).await?;
        if let (Some((p, sid)), Some(reason)) = (live, &row.interruption) {
            p.send(&frame(sid, row.sequence, TextEvent::Interrupted(public_error(reason))));
        }
        Ok(GenerationView::of(&row))
    }

    async fn interrupt_with_partial(&self, id: GenerationId, token: Uuid, reason: Interruption, partial: AnalysisProgress) -> Result<GenerationView, AiError> {
        let now = self.now();
        let draft = Draft::Analysis { progress: Box::new(partial) };
        let keep = content_len(&draft) <= self.config.bounds.llm.output_bytes;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(peek) = uow.ai().generation(id).await? else { return Err(AiError::NotFound) };
            lock_account(uow.as_mut(), peek.account).await?;
            let Some(mut row) = uow.ai().lock_generation(id).await? else { return Err(AiError::NotFound) };
            if generation::owns(&row, token).is_ok() {
                if keep {
                    row.draft = draft;
                }
                self.end(uow.as_mut(), &mut row, reason, false, now).await?;
            }
            Ok(row)
        }
        .await;
        Ok(GenerationView::of(&finish(uow, result).await?))
    }

    /// A running generation whose worker lease expired: complete its staged
    /// final locally, or interrupt it with the remote outcome unknown.
    async fn recover(&self, id: GenerationId) -> Result<GenerationView, AiError> {
        let now = self.now();
        let staged = self.stored_generation(id).await?.ok_or(AiError::NotFound)?;
        if staged.staged.is_some() {
            return self.complete(id, None, staged.sequence).await;
        }
        let mut uow = self.s.store.begin().await?;
        let result = async {
            lock_account(uow.as_mut(), staged.account).await?;
            let Some(mut row) = uow.ai().lock_generation(id).await? else { return Err(AiError::NotFound) };
            if generation::lease_expired(&row, now) && row.staged.is_none() {
                self.end(uow.as_mut(), &mut row, Interruption::WorkerLost, true, now).await?;
            }
            Ok(row)
        }
        .await;
        let row = finish(uow, result).await?;
        if row.staged.is_some() && !row.status.is_terminal() {
            return self.complete(id, None, row.sequence).await;
        }
        Ok(GenerationView::of(&row))
    }

    /// Accepted generations nobody runs and running ones whose lease expired.
    pub async fn run_pass(&self) -> Result<AiPassReport, AiError> {
        let mut report = AiPassReport::default();
        for id in self.s.store.runnable_generations(self.now(), 100).await? {
            report.examined += 1;
            match self.run(id).await {
                Ok(v) if v.status == GenerationStatus::Completed => report.completed += 1,
                Ok(v) if v.status == GenerationStatus::Interrupted => report.interrupted += 1,
                Ok(_) => {}
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    // ---- completion summary producer -----------------------------------------------

    async fn decide_summary(uow: &mut dyn JobUow, g: &Generation, outcome: SummaryOutcome, now: Micros) -> Result<(), AiError> {
        if let Some(mut request) = uow.ai().lock_summary_request(g.job, g.source_revision).await?
            && request.outcome.is_none()
        {
            request.outcome = Some(outcome);
            request.decided_at = Some(now);
            uow.ai().save_summary_request(&request).await?;
        }
        Ok(())
    }

    /// Settles every pending summary request (D5, JOB-033): disabled or
    /// unconfigured AI and jobs without lines are `Absent`, no valid consent or
    /// erasure is `Denied`, an existing summary of the job is reused, otherwise
    /// an unmetered summary generation runs and its end decides the outcome.
    pub async fn summary_pass(&self) -> Result<AiPassReport, AiError> {
        let mut report = AiPassReport::default();
        for (job, revision) in self.s.store.pending_summary_requests(100).await? {
            report.examined += 1;
            match self.summarize(job, revision).await {
                Ok(Some(id)) => match self.run(id).await {
                    Ok(v) if v.status == GenerationStatus::Completed => report.completed += 1,
                    Ok(v) if v.status == GenerationStatus::Interrupted => report.interrupted += 1,
                    Ok(_) => {}
                    Err(_) => report.errors += 1,
                },
                Ok(None) => report.reconciled += 1,
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    async fn summarize(&self, job: JobId, revision: i64) -> Result<Option<GenerationId>, AiError> {
        let now = self.now();
        let pending = self.stored_summary_request(job).await?;
        let Some(pending) = pending.filter(|r| r.source_revision == revision && r.outcome.is_none()) else { return Ok(None) };
        let account = pending.account;
        let has_lines = !self.s.store.lines(job).await?.is_empty();
        let mut uow = self.s.store.begin().await?;
        let result: Result<Option<GenerationId>, AiError> = async {
            if uow.lock_barrier(account).await?.is_some() {
                let mut r = pending.clone();
                r.outcome = Some(SummaryOutcome::Denied { reason: "the account is being erased".into() });
                r.decided_at = Some(now);
                uow.ai().save_summary_request(&r).await?;
                return Ok(None);
            }
            uow.billing().await?.lock_account(account).await?;
            let Some(mut request) = uow.ai().lock_summary_request(job, revision).await? else { return Ok(None) };
            if request.outcome.is_some() {
                return Ok(None);
            }
            if let Some(id) = request.generation {
                let g = uow.ai().generation(id).await?;
                return match g {
                    Some(g) if !g.status.is_terminal() => Ok(Some(id)),
                    Some(g) => {
                        let outcome = if g.status == GenerationStatus::Completed {
                            SummaryOutcome::Saved { generation: id }
                        } else {
                            SummaryOutcome::Failed { reason: format!("{:?}", g.interruption) }
                        };
                        request.outcome = Some(outcome);
                        request.decided_at = Some(now);
                        uow.ai().save_summary_request(&request).await?;
                        Ok(None)
                    }
                    None => {
                        request.outcome = Some(SummaryOutcome::Failed { reason: "the summary generation is gone".into() });
                        request.decided_at = Some(now);
                        uow.ai().save_summary_request(&request).await?;
                        Ok(None)
                    }
                };
            }
            let record = uow.ai().consent(account).await?;
            let decided = if !self.config.features_enabled || self.s.llm.is_none() {
                Some(SummaryOutcome::Absent { reason: "AI features are disabled".into() })
            } else if !has_lines {
                Some(SummaryOutcome::Absent { reason: "no transcript content".into() })
            } else if !consent::is_valid(record.as_ref()) {
                Some(SummaryOutcome::Denied { reason: "no valid AI consent".into() })
            } else if let Some(existing) = uow.ai().completion_summary(account, job).await?.filter(|s| !consent::covered(record.as_ref(), s.consent_epoch)) {
                Some(SummaryOutcome::Saved { generation: existing.generation })
            } else {
                None
            };
            if let Some(outcome) = decided {
                request.outcome = Some(outcome);
                request.decided_at = Some(now);
                uow.ai().save_summary_request(&request).await?;
                return Ok(None);
            }
            let key = Uuid::from_u128((u128::from(job.0 as u64) << 64) | u128::from(revision as u64));
            let g = generation::accepted(
                NewGeneration {
                    account,
                    job,
                    kind: GenerationKind::CompletionSummary,
                    request_key: key,
                    source_revision: revision,
                    consent_epoch: consent::epoch(record.as_ref()),
                    input: GenerationInput::CompletionSummary { detailed: false },
                },
                now,
            );
            uow.ai().insert_generation(&g).await?;
            request.generation = Some(g.id);
            uow.ai().save_summary_request(&request).await?;
            Ok(Some(g.id))
        }
        .await;
        finish(uow, result).await
    }

    /// Called by `Jobs` in the completing transaction (D5): the durable
    /// producer of the job's summary outcome.
    pub async fn request_summary(uow: &mut dyn JobUow, account: AccountId, job: JobId, now: Micros) -> Result<(), StoreError> {
        let source_revision = uow.ai().source_revision(job).await?;
        let request = SummaryRequest { account, job, source_revision, requested_at: now, generation: None, outcome: None, decided_at: None };
        uow.ai().request_summary(&request).await
    }
}

#[cfg(test)]
#[path = "app/tests.rs"]
mod tests;
