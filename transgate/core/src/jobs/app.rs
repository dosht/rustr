//! Job use cases: drafts, uploads, start, URL and YouTube sources, the
//! provider attempt protocol, callbacks, ingestion, recovery, deletion, edits
//! and queries.
//!
//! Every write is one `JobUow`. Network calls (providers, storage, captions)
//! happen strictly between transactions: claim, call, acknowledge (seam 5/6).

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use billing_core::app::{ChargeError, charge_job_starts, charge_strict, release_subject_charges};
use billing_core::freemium::{Draft, FreemiumLists, is_freemium_app1, web_guard};
use billing_core::ledger::{balance, open_at};
use billing_core::ports::StoreError;
use billing_core::types::Clock;
use media_ports::{
    AccessMethod, CallbackSpec, ExternalStager, ObjectRef, ObjectStore, ObservationOrigin, PaidCaptions,
    ProviderObservation, ProviderPayload, SecretString, SensitiveUrl, StoreKind, TemporaryAccess, Transcriber,
    TranscriptionRequest, VideoMetadata, VideoMetadataSource,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::export::{self, DownloadEvent, ExportBundle, ExportLabels};
use crate::ids::{AccountId, AttemptId, InboxId, JobId, MICROS_PER_MINUTE, Micros, from_media, to_media};
use crate::jobs::correlate;
use crate::jobs::domain::{
    Attempt, AttemptRequest, AttemptState, CLAIM_LEASE_MINUTES, Job, JobError, JobSource, JobStatus, NewJob,
    Provider, RemoteCleanup, StorageProvider, client_reference,
};
use crate::jobs::duration::{self, DurationDecision};
use crate::jobs::events::JobEvent;
use crate::jobs::ingest::{self, IngestPlan};
use crate::jobs::ports::{
    InboxEntry, InboxState, IssuedUpload, JobListQuery, JobStore, JobUow, LanguageUsage, NewInboxEntry, RateDecision,
    RateLimiter, Sleeper,
};
use crate::jobs::retry::{self, StuckAction, SubmitOutcome};
use crate::jobs::routing::{self, Availability, Executable, ModelConfig, Preferences, Route, RoutingConfig};
use crate::languages::Language;
use crate::privacy::erasure::{ErasureItem, ErasureState, ItemState, ItemTarget};
use crate::privacy::settings::UserSettings;
use crate::sources::{self, UrlCheck, UrlKind};
use crate::api::idempotency::{self, IdempotencyClaim, IdempotencyRecord, Lookup};
use crate::transcript::{Line, LineEdit, NewLine, TranslationStatus, captions};

/// Errors a shell maps to HTTP. `http_status` is the intended mapping.
#[derive(Clone, Debug, PartialEq)]
pub enum JobsError {
    NotFound,
    /// 400 with the source message.
    InvalidRequest(String),
    /// JOB-004: "No draft jobs found".
    NoDraftJobs,
    /// JOB-004: "No jobs were updated".
    NoJobsUpdated,
    /// JOB-006/007, BIL-039..041, FRE: 402 `insufficient_balance` with the message.
    InsufficientBalance(String),
    /// PRV-051: 403 "Account suspended".
    Blocked,
    /// RTL-004/005: 429 with the window, for the `X-RateLimit-*` headers.
    RateLimited(RateDecision),
    /// YouTube resolve on a job that is not a draft (409).
    WrongState(String),
    /// 422 "Job does not have a YouTube video ID".
    Unprocessable(String),
    /// UPL-016: 404 "Audio file not available".
    AudioUnavailable,
    /// YouTube metadata: 404 "Video not found or is private".
    VideoNotFound,
    /// 502, an upstream failure the source surfaced directly.
    Upstream(String),
    /// 500 with the source message (YouTube metadata lookup failure).
    Failed(String),
    /// 410: the account is being or was erased (C-11 barrier).
    ErasureInProgress,
    /// 409 `request_conflict`: an `Idempotency-Key` reused for another request (P7 D4).
    RequestConflict,
    /// 503: a required adapter is not configured.
    NotConfigured(String),
    Store(StoreError),
}

impl JobsError {
    pub fn http_status(&self) -> u16 {
        match self {
            JobsError::NotFound | JobsError::AudioUnavailable | JobsError::VideoNotFound => 404,
            JobsError::InvalidRequest(_) | JobsError::NoDraftJobs | JobsError::NoJobsUpdated => 400,
            JobsError::InsufficientBalance(_) => 402,
            JobsError::Blocked => 403,
            JobsError::WrongState(_) | JobsError::RequestConflict => 409,
            JobsError::ErasureInProgress => 410,
            JobsError::Unprocessable(_) => 422,
            JobsError::RateLimited(_) => 429,
            JobsError::Upstream(_) => 502,
            JobsError::Failed(_) => 500,
            JobsError::NotConfigured(_) => 503,
            JobsError::Store(StoreError::Conflict(_)) => 409,
            JobsError::Store(_) => 500,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            JobsError::NotFound => "not_found",
            JobsError::InvalidRequest(_) | JobsError::NoDraftJobs | JobsError::NoJobsUpdated => "invalid_request",
            JobsError::InsufficientBalance(_) => "insufficient_balance",
            JobsError::Blocked => "account_suspended",
            JobsError::RateLimited(_) => "rate_limited",
            JobsError::WrongState(_) => "conflict",
            JobsError::RequestConflict => "request_conflict",
            JobsError::Unprocessable(_) => "unprocessable",
            JobsError::AudioUnavailable => "audio_unavailable",
            JobsError::VideoNotFound => "video_not_found",
            JobsError::Upstream(_) => "upstream_error",
            JobsError::Failed(_) => "internal_error",
            JobsError::ErasureInProgress => "account_deletion_in_progress",
            JobsError::NotConfigured(_) => "not_configured",
            JobsError::Store(StoreError::Conflict(_)) => "conflict",
            JobsError::Store(_) => "database_error",
        }
    }

    /// The source wording where one exists.
    pub fn message(&self) -> String {
        match self {
            JobsError::NotFound => "Job not found".into(),
            JobsError::InvalidRequest(m)
            | JobsError::InsufficientBalance(m)
            | JobsError::WrongState(m)
            | JobsError::Unprocessable(m)
            | JobsError::Upstream(m)
            | JobsError::Failed(m)
            | JobsError::NotConfigured(m) => m.clone(),
            JobsError::NoDraftJobs => "No draft jobs found".into(),
            JobsError::NoJobsUpdated => "No jobs were updated".into(),
            JobsError::Blocked => "Account suspended".into(),
            JobsError::RateLimited(_) => "Too many requests. Please try again in a minute.".into(),
            JobsError::AudioUnavailable => "Audio file not available".into(),
            JobsError::VideoNotFound => "Video not found or is private".into(),
            JobsError::ErasureInProgress => "Account deletion in progress".into(),
            JobsError::RequestConflict => "This Idempotency-Key was already used for a different request".into(),
            JobsError::Store(e) => e.to_string(),
        }
    }
}

impl fmt::Display for JobsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for JobsError {}

impl From<StoreError> for JobsError {
    fn from(e: StoreError) -> Self {
        match e {
            // Billing's own erasure fence (D9): the same refusal as the host barrier.
            StoreError::AccountErased(_) => JobsError::ErasureInProgress,
            other => JobsError::Store(other),
        }
    }
}

impl From<JobError> for JobsError {
    fn from(e: JobError) -> Self {
        match e {
            JobError::WrongStatus { .. }
            | JobError::AttemptInFlight { .. }
            | JobError::StaleAttempt { .. }
            | JobError::RemoteMismatch { .. } => {
                JobsError::WrongState(e.to_string())
            }
            JobError::Invalid(m) => JobsError::InvalidRequest(m),
        }
    }
}

/// Deployment settings the use cases need. Every value is required configuration.
#[derive(Clone, Debug)]
pub struct JobsConfig {
    pub routing: RoutingConfig,
    pub models: ModelConfig,
    /// UPL-012: `STORAGE_PROVIDER` and its bucket for new uploads.
    pub upload_storage: StorageProvider,
    pub upload_bucket: String,
    /// JOB-044: where raw provider payloads are archived.
    pub archive_store: StoreKind,
    pub archive_bucket: String,
    /// Private staging for controlled external URL fetches.
    pub staging_store: StoreKind,
    pub staging_bucket: String,
    /// Public origin for provider callbacks, e.g. `https://transgate.ai`.
    pub callback_base_url: String,
    /// PRV-037 / TRX-041: delete the Soniox transcription after a completed webhook.
    pub soniox_auto_delete: bool,
    /// UPL-030: off only in development.
    pub ssrf_protection: bool,
    /// The freemium locale and language exception lists (host data, FRE).
    pub freemium_lists: FreemiumLists,
}

/// The adapters the use cases call.
#[derive(Clone)]
pub struct JobsServices {
    pub store: Arc<dyn JobStore>,
    pub clock: Arc<dyn Clock>,
    pub objects: Vec<Arc<dyn ObjectStore>>,
    pub transcribers: Vec<Arc<dyn Transcriber>>,
    pub stager: Option<Arc<dyn ExternalStager>>,
    pub sleeper: Arc<dyn Sleeper>,
    pub rate_limiter: Arc<dyn RateLimiter>,
    pub paid_captions: Option<Arc<dyn PaidCaptions>>,
    pub video_metadata: Option<Arc<dyn VideoMetadataSource>>,
}

pub struct Jobs {
    s: JobsServices,
    config: JobsConfig,
}

/// A new upload draft (JOB-002, UPL-001).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewDraft {
    pub name: String,
    pub size_bytes: Option<i64>,
    pub language: Option<Language>,
    pub speakers: bool,
    /// Browser-measured whole minutes, rounded up; `None` when measurement failed.
    pub duration_minutes: Option<i32>,
}

/// Draft option writes (JOB-005, UPL-019/020). `None` leaves a field unchanged;
/// `language: Some(None)` is auto-detect.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DraftOptions {
    pub language: Option<Option<Language>>,
    pub translate_to: Option<Vec<Language>>,
    pub speakers: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartRequest {
    pub job_ids: Vec<JobId>,
    pub locale: Option<String>,
    pub preferences: Preferences,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UrlItem {
    pub audio_url: String,
    pub callback_url: Option<String>,
    /// A stored language name; LNG-027 defaults to English.
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UrlRequest {
    pub items: Vec<UrlItem>,
    pub locale: Option<String>,
    pub preferences: Preferences,
    /// API Whisper options, kept opaque (TRX-022).
    pub whisper_params: Option<Value>,
}

/// The two public start shapes (API-007): exactly one of `jobIds` and `audioUrls`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PublicStart {
    Jobs(StartRequest),
    Urls(UrlRequest),
}

impl PublicStart {
    /// `StartMeta::request_type`.
    pub fn kind(&self) -> &'static str {
        match self {
            PublicStart::Jobs(_) => "jobIds",
            PublicStart::Urls(_) => "audioUrls",
        }
    }

    fn preferences(&self) -> Preferences {
        match self {
            PublicStart::Jobs(r) => r.preferences,
            PublicStart::Urls(r) => r.preferences,
        }
    }

    /// API-007/009: nonempty, and every source and callback URL passes the URL rules.
    fn validate(&self, ssrf_protection: bool) -> Result<(), JobsError> {
        let empty = || JobsError::InvalidRequest("Either jobIds or audioUrls must be provided".into());
        match self {
            PublicStart::Jobs(r) if r.job_ids.is_empty() => Err(empty()),
            PublicStart::Jobs(_) => Ok(()),
            PublicStart::Urls(r) if r.items.is_empty() => Err(empty()),
            PublicStart::Urls(r) => {
                for item in &r.items {
                    if let UrlCheck::Invalid(m) = sources::check_url(&item.audio_url, UrlKind::Audio, ssrf_protection) {
                        return Err(JobsError::InvalidRequest(m));
                    }
                    if let Some(cb) = &item.callback_url
                        && let UrlCheck::Invalid(m) = sources::check_url(cb, UrlKind::Callback, ssrf_protection)
                    {
                        return Err(JobsError::InvalidRequest(m));
                    }
                }
                Ok(())
            }
        }
    }
}

/// A public start's answer; `replayed` is a D4 replay that created and charged nothing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicAnswer {
    pub answer: StartAnswer,
    pub replayed: bool,
}

/// One job's result in the start answer (JOB-011).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobResult {
    pub job_id: JobId,
    pub status: JobStatus,
    pub estimated_completion: Option<Micros>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartMeta {
    pub new_balance: i64,
    pub is_freemium: bool,
    pub total_jobs: usize,
    pub successful_jobs: usize,
    pub failed_jobs: usize,
    pub queued_jobs: usize,
    pub estimated_completion: Option<Micros>,
    /// `jobIds` or `audioUrls`.
    pub request_type: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartAnswer {
    pub results: Vec<JobResult>,
    pub meta: StartMeta,
}

/// The history window context: the browser's UTC offsets now and one month
/// earlier (PRV-032 uses local `setMonth`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarContext {
    pub offset_now_minutes: i32,
    pub offset_then_minutes: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobPage {
    pub jobs: Vec<Job>,
    /// The last id of this page when it was full (keyset cursor, id descending).
    pub next_cursor: Option<JobId>,
}

/// JOB-039/PRV-032: 20 per page.
pub const PAGE_SIZE: u32 = 20;

/// TRX-039: the job and, when completed, its lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobDetail {
    pub job: Job,
    pub lines: Vec<Line>,
}

/// Which screen asked for the download (EXP-013).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportSurface {
    /// The transcript page: translation jobs export only their translation lines.
    TranscriptPage,
    /// The job list and phone details: every stored line, no filter.
    JobList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportKind {
    /// "Download" (EXP-012): the editor toggles; absent means time on, speakers from the job.
    Wysiwyg { show_timestamps: Option<bool>, show_speakers: Option<bool> },
    /// "Download all formats" (EXP-011).
    AllFormats,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRequest {
    pub kind: ExportKind,
    pub surface: ExportSurface,
    /// The translated words of the viewer's locale (file names and headers).
    pub labels: ExportLabels,
}

/// The intake answer for a provider callback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeAck {
    /// Durably stored; acknowledge with 2xx.
    Stored { inbox: InboxId },
    /// The same delivery again.
    Duplicate { inbox: InboxId },
    /// Same attempt and remote id, different payload: held, never applied.
    Conflict { inbox: InboxId },
    /// The account is erased: only the remote identity was recorded for cleanup.
    Erased,
}

/// YouTube resolve answer (UPL-026/027): 200 either way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResolveAnswer {
    Completed { job_id: JobId },
    Failed { job_id: JobId, code: String, message: String },
}

/// What a dispatch or recovery pass did, for logs and tests.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassReport {
    pub examined: u32,
    pub submitted: u32,
    pub rate_limited: u32,
    pub waiting: u32,
    pub failed: u32,
    pub recovered: u32,
    pub errors: u32,
}

/// What a provider listing says about one unresolved attempt (C5, F5).
enum ListingOutcome {
    /// Exactly one record agrees with everything the attempt sent.
    Found(String),
    /// None or several agree: the attempt stays unresolved.
    Unresolved,
    /// The listing could not be read: also unresolved, and reported as an error.
    Failed,
    /// The provider has no listing (Deepgram); only the stuck window gives up.
    NoListing,
}

fn new_secret() -> SecretString {
    SecretString::new(format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()))
}

fn store_kind(storage: StorageProvider) -> Option<StoreKind> {
    storage.store_kind()
}

impl Jobs {
    pub fn new(services: JobsServices, config: JobsConfig) -> Jobs {
        Jobs { s: services, config }
    }

    pub fn config(&self) -> &JobsConfig {
        &self.config
    }

    fn now(&self) -> Micros {
        self.s.clock.now()
    }

    /// The adapter bound to exactly this object's store and bucket. Adapters refuse
    /// other buckets, so a kind-only match would only pick one that rejects the call.
    fn object_store_for(&self, object: &ObjectRef) -> Option<&Arc<dyn ObjectStore>> {
        self.s.objects.iter().find(|o| o.kind() == object.store && o.bucket() == object.bucket)
    }

    fn transcriber(&self, provider: Provider) -> Option<&Arc<dyn Transcriber>> {
        let transport = provider.transport()?;
        self.s.transcribers.iter().find(|t| t.provider() == transport)
    }

    fn configured(&self) -> BTreeSet<Provider> {
        Provider::ALL.into_iter().filter(|p| self.transcriber(*p).is_some()).collect()
    }

    async fn guard(&self, account: AccountId) -> Result<(), JobsError> {
        if self.s.store.barrier(account).await? {
            return Err(JobsError::ErasureInProgress);
        }
        Ok(())
    }

    async fn rollback(uow: Box<dyn JobUow>) {
        let _ = uow.rollback().await;
    }

    /// Saves a job, emitting the event for a status transition in the same transaction.
    async fn save(uow: &mut dyn JobUow, job: &mut Job, before: JobStatus, now: Micros) -> Result<(), JobsError> {
        let new_revision = uow.save_job(job, job.revision).await?;
        job.revision = new_revision;
        if let Some(event) = JobEvent::for_transition(before, job, now) {
            uow.emit(&event).await?;
        }
        Ok(())
    }

    // ---- drafts and uploads -------------------------------------------------

    /// JOB-002, UPL-001: the browser's draft, now created by the server.
    pub async fn create_draft(&self, account: AccountId, draft: NewDraft) -> Result<Job, JobsError> {
        self.guard(account).await?;
        let now = self.now();
        let new = NewJob {
            account,
            status: JobStatus::Draft,
            name: Some(draft.name),
            source: JobSource::Upload { storage: self.config.upload_storage, object: None },
            language: draft.language,
            translate_to: vec![],
            speakers: draft.speakers,
            fix_grammer: None,
            duration_minutes: draft.duration_minutes,
            size_bytes: draft.size_bytes,
            callback_url: None,
            whisper_params: None,
            created_at: now,
        };
        let mut uow = self.s.store.begin().await?;
        let job = match uow.insert_job(&new).await {
            Ok(job) => job,
            Err(e) => {
                Self::rollback(uow).await;
                return Err(e.into());
            }
        };
        uow.commit().await?;
        Ok(job)
    }

    /// JOB-005, UPL-019/020: options are written to the owner's drafts; other
    /// ids are left alone, as the source's RLS update was.
    pub async fn set_draft_options(&self, account: AccountId, ids: &[JobId], options: DraftOptions) -> Result<Vec<Job>, JobsError> {
        self.guard(account).await?;
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            let mut jobs = uow.lock_jobs(account, ids).await?;
            let mut changed = Vec::new();
            for job in jobs.iter_mut().filter(|j| j.status == JobStatus::Draft) {
                let before = job.status;
                job.set_draft_options(options.language, options.translate_to.clone(), options.speakers, now)?;
                Self::save(uow.as_mut(), job, before, now).await?;
                changed.push(job.clone());
            }
            Ok::<_, JobsError>(changed)
        }
        .await;
        match result {
            Ok(jobs) => {
                uow.commit().await?;
                Ok(jobs)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// UPL-009..012: a 900-second PUT URL for the caller's folder. As built, the
    /// job need not exist or be owned for the URL; the storage provider is
    /// recorded only on the caller's own draft. The capability is recorded so
    /// erasure can re-sweep after it expires.
    pub async fn init_upload(&self, account: AccountId, job_id: JobId, file_name: &str) -> Result<TemporaryAccess, JobsError> {
        self.guard(account).await?;
        let now = self.now();
        let kind = store_kind(self.config.upload_storage)
            .ok_or_else(|| JobsError::NotConfigured("uploads need a bucket storage provider".into()))?;
        let object = ObjectRef {
            store: kind,
            bucket: self.config.upload_bucket.clone(),
            key: sources::upload_key(account, job_id, file_name),
            version: None,
        };
        let store = self
            .object_store_for(&object)
            .ok_or_else(|| JobsError::NotConfigured(format!("object store {kind:?} for the upload bucket is not configured")))?
            .clone();
        let access = store
            .sign(&object, AccessMethod::Put, to_media(now), sources::UPLOAD_URL_TTL_SECONDS)
            .await
            .map_err(|e| JobsError::Failed(format!("An error occurred: {e}")))?;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            let mut owned = uow.lock_jobs(account, &[job_id]).await?;
            if let Some(job) = owned.first_mut().filter(|j| j.status == JobStatus::Draft) {
                let before = job.status;
                job.bind_upload(self.config.upload_storage, object.clone(), now)?;
                Self::save(uow.as_mut(), job, before, now).await?;
            }
            let issued = IssuedUpload { account, job: job_id, object: object.clone(), expires_at: from_media(access.expires_at) };
            uow.record_issued_upload(&issued).await?;
            Ok::<_, JobsError>(())
        }
        .await;
        match result {
            Ok(()) => {
                uow.commit().await?;
                Ok(access)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// UPL-015, JOB-037: removing a queued file hard deletes its draft. Billing's
    /// release runs first (BIL-042); the uploaded object stays (JOB-038).
    pub async fn remove_draft(&self, account: AccountId, job_id: JobId) -> Result<(), JobsError> {
        self.guard(account).await?;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            {
                let mut billing = uow.billing().await?;
                release_subject_charges(&mut *billing, account, job_id.subject()).await.map_err(charge_error)?;
            }
            let owned = uow.lock_jobs(account, &[job_id]).await?;
            let Some(job) = owned.first() else { return Err(JobsError::NotFound) };
            if job.status != JobStatus::Draft {
                return Err(JobsError::WrongState(format!("Job {} is not a draft", job.id)));
            }
            uow.hard_delete_job(job_id).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                uow.commit().await?;
                Ok(())
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    // ---- start ---------------------------------------------------------------

    /// JOB-004..012: one transaction locks the account and the owner's drafts in
    /// ascending id, applies the web prechecks and the billing charge, and moves
    /// every draft to `not_started` together. Dispatch follows after commit, one
    /// job after another; a failure of one job is that job's result.
    pub async fn start(&self, account: AccountId, request: StartRequest) -> Result<StartAnswer, JobsError> {
        Ok(self.start_with(account, PublicStart::Jobs(request), None, false).await?.answer)
    }

    /// The body of a `jobIds` start inside its transaction, after the account lock.
    async fn start_drafts(&self, uow: &mut dyn JobUow, account: AccountId, request: &StartRequest, now: Micros, api_balance: bool) -> Result<Vec<Job>, JobsError> {
        let packages = {
            let mut billing = uow.billing().await?;
            billing.lock_account(account).await?;
            billing.packages(account).await?
        };
        let mut drafts: Vec<Job> = uow.lock_jobs(account, &request.job_ids).await?.into_iter().filter(|j| j.status == JobStatus::Draft).collect();
        if drafts.is_empty() {
            return Err(JobsError::NoDraftJobs);
        }
        // JOB-006, BIL-039: the web refuses below zero and leaves zero to the charge; the public API
        // refuses zero too (API-BIL-012).
        let open = open_at(&packages, now);
        let allowed = if api_balance { billing_core::charge::api_precheck(balance(&open)) } else { billing_core::charge::web_precheck(balance(&open)) };
        if !allowed {
            return Err(JobsError::InsufficientBalance("Not enough balance".into()));
        }
        // FRE (app definition 1) web guard over the drafts in ascending id.
        let all: Vec<billing_core::ledger::Package> = packages.iter().map(|p| p.package.clone()).collect();
        if is_freemium_app1(&all) {
            let guard_drafts: Vec<Draft> = drafts
                .iter()
                .map(|j| Draft {
                    id: j.id.0,
                    duration: j.duration_minutes.map(i64::from),
                    language: j.language.map(|l| l.name().to_string()),
                    first_translation_target: j.translation_target().map(|l| l.name().to_string()),
                })
                .collect();
            if let Err(refusal) = web_guard(&guard_drafts, request.locale.as_deref().unwrap_or(""), &self.config.freemium_lists) {
                return Err(JobsError::InsufficientBalance(refusal.message().into()));
            }
        }
        self.charge_and_start(uow, account, &mut drafts, now).await?;
        Ok(drafts)
    }

    /// P7 public start (`POST /api/v1/transcriptions`) with an optional
    /// `Idempotency-Key` (D4): the record is read after the billing account
    /// lock and written in the same transaction as the charge, so a replay of
    /// the same request answers again without creating or charging anything,
    /// and the same key with another request is `RequestConflict`.
    pub async fn start_public(&self, account: AccountId, request: PublicStart, claim: Option<IdempotencyClaim>) -> Result<PublicAnswer, JobsError> {
        self.start_with(account, request, claim, true).await
    }

    /// `api_balance`: the public API refuses a balance of zero or less (API-BIL-012); the web start
    /// refuses only below zero and leaves zero to the charge.
    async fn start_with(&self, account: AccountId, request: PublicStart, claim: Option<IdempotencyClaim>, api_balance: bool) -> Result<PublicAnswer, JobsError> {
        self.guard(account).await?;
        request.validate(self.config.ssrf_protection)?;
        let now = self.now();
        let expired_before = idempotency::expired_before(now);
        let mut uow = self.s.store.begin().await?;
        let started = async {
            if let Some(claim) = &claim {
                uow.billing().await?.lock_account(account).await?;
                let stored = uow.api().idempotency(account, &claim.key, expired_before).await?;
                match idempotency::lookup(stored, claim) {
                    Lookup::Replay(record) => return Ok(Err(*record)),
                    Lookup::Conflict => return Err(JobsError::RequestConflict),
                    Lookup::New => {}
                }
            }
            let drafts = match &request {
                PublicStart::Jobs(r) => self.start_drafts(uow.as_mut(), account, r, now, api_balance).await?,
                PublicStart::Urls(r) => self.url_drafts(uow.as_mut(), account, r, now).await?,
            };
            if let Some(claim) = &claim {
                let record = IdempotencyRecord {
                    account,
                    key: claim.key.clone(),
                    request_digest: claim.request_digest.clone(),
                    request_type: request.kind().into(),
                    jobs: drafts.iter().map(|j| j.id).collect(),
                    answer: None,
                    created_at: now,
                };
                uow.api().insert_idempotency(&record, expired_before).await.map_err(|e| match e {
                    StoreError::Conflict(_) => JobsError::RequestConflict,
                    other => other.into(),
                })?;
            }
            Ok::<_, JobsError>(Ok(drafts))
        }
        .await;
        let drafts = match started {
            Ok(Ok(drafts)) => {
                uow.commit().await?;
                drafts
            }
            Ok(Err(record)) => {
                Self::rollback(uow).await;
                return Ok(PublicAnswer { answer: self.replayed(account, record).await?, replayed: true });
            }
            Err(e) => {
                Self::rollback(uow).await;
                return Err(e);
            }
        };
        let mut results = Vec::new();
        for job in drafts {
            results.push(self.dispatch_reporting(job.id, request.preferences()).await);
        }
        let answer = self.answer(account, results, request.kind()).await?;
        if let Some(claim) = &claim {
            // Best effort: without it a replay rebuilds the answer from the same jobs.
            if let Ok(mut uow) = self.s.store.begin().await {
                match uow.api().save_idempotency_answer(account, &claim.key, &answer).await {
                    Ok(()) => {
                        let _ = uow.commit().await;
                    }
                    Err(_) => Self::rollback(uow).await,
                }
            }
        }
        Ok(PublicAnswer { answer, replayed: false })
    }

    /// The first answer of a recorded start, or the same jobs' current state.
    async fn replayed(&self, account: AccountId, record: IdempotencyRecord) -> Result<StartAnswer, JobsError> {
        if let Some(answer) = record.answer {
            return Ok(answer);
        }
        let mut results = Vec::with_capacity(record.jobs.len());
        for id in record.jobs {
            let status = self.s.store.owned_job(account, id).await?.map_or(JobStatus::Failed, |j| j.status);
            results.push(JobResult { job_id: id, status, estimated_completion: None, error: None });
        }
        let kind = if record.request_type == "audioUrls" { "audioUrls" } else { "jobIds" };
        self.answer(account, results, kind).await
    }

    /// The billable move of every draft, all or nothing (BIL-025..033).
    async fn charge_and_start(&self, uow: &mut dyn JobUow, account: AccountId, drafts: &mut [Job], now: Micros) -> Result<(), JobsError> {
        let completed = uow.completed_job_count(account).await?;
        let starts: Vec<_> = drafts.iter().map(|j| j.billing_start(completed)).collect();
        {
            let mut billing = uow.billing().await?;
            charge_job_starts(&mut *billing, account, &starts, now).await.map_err(charge_error)?;
        }
        if drafts.is_empty() {
            return Err(JobsError::NoJobsUpdated);
        }
        for job in drafts.iter_mut() {
            let before = job.status;
            job.start(now)?;
            Self::save(uow, job, before, now).await?;
        }
        Ok(())
    }

    async fn answer(&self, account: AccountId, results: Vec<JobResult>, request_type: &'static str) -> Result<StartAnswer, JobsError> {
        let now = self.now();
        // BIL-074: balance and freemium flag read after the jobs were handed over.
        let (new_balance, is_freemium) = match self.s.store.begin().await {
            Ok(mut uow) => {
                let read = async {
                    let mut billing = uow.billing().await?;
                    billing.packages(account).await
                }
                .await;
                let _ = uow.rollback().await;
                match read {
                    Ok(packages) => {
                        let all: Vec<_> = packages.iter().map(|p| p.package.clone()).collect();
                        (balance(&open_at(&packages, now)).unwrap_or(0), is_freemium_app1(&all))
                    }
                    Err(_) => (0, false),
                }
            }
            Err(_) => (0, false),
        };
        let estimated_completion = results.iter().filter_map(|r| r.estimated_completion).max();
        let meta = StartMeta {
            new_balance,
            is_freemium,
            total_jobs: results.len(),
            successful_jobs: results
                .iter()
                .filter(|r| matches!(r.status, JobStatus::Running | JobStatus::Completed | JobStatus::Pending))
                .count(),
            failed_jobs: results.iter().filter(|r| r.status == JobStatus::Failed).count(),
            queued_jobs: results.iter().filter(|r| matches!(r.status, JobStatus::Pending | JobStatus::NotStarted)).count(),
            estimated_completion,
            request_type: request_type.into(),
        };
        Ok(StartAnswer { results, meta })
    }

    /// UPL-030/031, JOB-009, BIL-040: API audio URLs. Each URL becomes a draft,
    /// then starts with an unknown duration (deferred charge) after the
    /// above-zero balance check. The HTTP route mounts with P7's API-key principal.
    pub async fn submit_urls(&self, account: AccountId, request: UrlRequest) -> Result<StartAnswer, JobsError> {
        Ok(self.start_public(account, PublicStart::Urls(request), None).await?.answer)
    }

    /// The body of an `audioUrls` start inside its transaction, after the account lock.
    async fn url_drafts(&self, uow: &mut dyn JobUow, account: AccountId, request: &UrlRequest, now: Micros) -> Result<Vec<Job>, JobsError> {
        let english = Language::from_name("English").expect("English");
        let packages = {
            let mut billing = uow.billing().await?;
            billing.lock_account(account).await?;
            billing.packages(account).await?
        };
        let mut drafts = Vec::new();
        for item in &request.items {
            let language = match &item.language {
                None => english,
                Some(name) => Language::from_name(name).ok_or_else(|| JobsError::InvalidRequest(format!("Unsupported language: {name}")))?,
            };
            let new = NewJob {
                account,
                status: JobStatus::Draft,
                name: Some(sources::API_JOB_NAME.into()),
                source: JobSource::ExternalUrl { file_path: Some(item.audio_url.clone()), staged: None },
                language: Some(language),
                translate_to: vec![],
                speakers: true,
                fix_grammer: Some(true),
                duration_minutes: None,
                size_bytes: None,
                callback_url: item.callback_url.clone(),
                whisper_params: request.whisper_params.clone(),
                created_at: now,
            };
            drafts.push(uow.insert_job(&new).await?);
        }
        if !billing_core::charge::api_precheck(balance(&open_at(&packages, now))) {
            return Err(JobsError::InsufficientBalance("Insufficient balance".into()));
        }
        self.charge_and_start(uow, account, &mut drafts, now).await?;
        Ok(drafts)
    }

    // ---- dispatch: claim, call, acknowledge ------------------------------------

    async fn dispatch_reporting(&self, job_id: JobId, prefs: Preferences) -> JobResult {
        match self.dispatch(job_id, prefs).await {
            Ok(r) => r,
            Err(e) => JobResult { job_id, status: JobStatus::Failed, estimated_completion: None, error: Some(e.message()) },
        }
    }

    async fn load_job(&self, id: JobId) -> Result<Job, JobsError> {
        let mut uow = self.s.store.begin().await?;
        let job = uow.lock_job(id).await;
        let _ = uow.rollback().await;
        job?.ok_or(JobsError::NotFound)
    }

    /// Seam 5 lock order for one job: its billing account first, then the job row
    /// (F4). The owner comes from an earlier unlocked read; a job that is gone, or
    /// one another account owns, reads as missing.
    async fn lock_job_after_account(uow: &mut dyn JobUow, account: AccountId, id: JobId) -> Result<Option<Job>, JobsError> {
        uow.billing().await?.lock_account(account).await?;
        Ok(uow.lock_job(id).await?.filter(|j| j.account == account))
    }

    /// Hands one job to providers: routing, then attempts with failover until
    /// one is accepted, rate limited, waiting, uncertain or exhausted.
    pub async fn dispatch(&self, job_id: JobId, prefs: Preferences) -> Result<JobResult, JobsError> {
        let mut tried: Vec<Provider> = Vec::new();
        let mut first_failure: Option<Value> = None;
        loop {
            let job = self.load_job(job_id).await?;
            if !matches!(job.status, JobStatus::NotStarted | JobStatus::Pending) {
                return Ok(JobResult { job_id, status: job.status, estimated_completion: None, error: None });
            }
            // TRX-023, LNG-013: a target without a code fails the job; no fallback.
            if let Some(target) = &job.unmapped_translation_target {
                let message = format!("Translation language mapping failed for job {}: Unsupported language \"{target}\"", job_id.0);
                let failure = serde_json::json!({
                    "message": message,
                    "code": retry::ErrorType::InvalidRequest.as_str(),
                    "provider": Provider::Soniox.as_str(),
                });
                return self.fail_job(job_id, failure).await;
            }
            let availability = self.availability(&job).await?;
            let next = if tried.is_empty() {
                match routing::first_choice(&job, &prefs, &availability, &self.config.routing) {
                    Route::Submit { provider, .. } => Some(provider),
                    Route::Wait => None,
                    Route::Fail { message, .. } => {
                        let failure = retry::exhausted_failure(&message, job.provider.unwrap_or(Provider::Soniox));
                        return self.fail_job(job_id, first_failure.unwrap_or(failure)).await;
                    }
                }
            } else {
                routing::failover(&job, &tried, &prefs, &availability)
            };
            let Some(provider) = next else {
                if tried.is_empty() || first_failure.is_none() {
                    // Capacity only: wait as pending/not_started for the next pass.
                    return self.wait(job_id).await;
                }
                let message = if job.is_translation() { routing::TRANSLATION_UNAVAILABLE } else { routing::ALL_PROVIDERS_FAILED };
                let failure = first_failure.unwrap_or_else(|| retry::exhausted_failure(message, tried[0]));
                return self.fail_job(job_id, failure).await;
            };
            tried.push(provider);
            match self.attempt(job_id, provider).await? {
                AttemptResult::Accepted(job) => {
                    return Ok(JobResult {
                        job_id,
                        status: job.status,
                        estimated_completion: Some(retry::estimated_completion(&job, self.now())),
                        error: None,
                    });
                }
                AttemptResult::RateLimited => {
                    // TRX-014: a rate limit fails over when another provider can take it.
                    let job = self.load_job(job_id).await?;
                    let availability = self.availability(&job).await?;
                    if routing::failover(&job, &tried, &prefs, &availability).is_some() {
                        continue;
                    }
                    return Ok(JobResult {
                        job_id,
                        status: JobStatus::Pending,
                        estimated_completion: None,
                        error: Some("Rate limit exceeded, job queued for retry".into()),
                    });
                }
                AttemptResult::Waiting => return self.wait(job_id).await,
                AttemptResult::Uncertain => {
                    let job = self.load_job(job_id).await?;
                    return Ok(JobResult { job_id, status: job.status, estimated_completion: None, error: None });
                }
                AttemptResult::Failed(failure) => {
                    first_failure.get_or_insert(failure);
                }
                AttemptResult::Busy => {
                    let job = self.load_job(job_id).await?;
                    return Ok(JobResult { job_id, status: job.status, estimated_completion: None, error: None });
                }
            }
        }
    }

    async fn availability(&self, job: &Job) -> Result<Availability, JobsError> {
        let mut availability = Availability { configured: self.configured(), deepgram_inflight: 0 };
        if let Some(t) = self.transcriber(Provider::Deepgram) {
            let mut uow = self.s.store.begin().await?;
            let n = uow.inflight(media_ports::Provider::Deepgram, t.project(), self.now()).await;
            let _ = uow.rollback().await;
            availability.deepgram_inflight = n?;
        }
        let _ = job;
        Ok(availability)
    }

    async fn wait(&self, job_id: JobId) -> Result<JobResult, JobsError> {
        let now = self.now();
        let account = self.load_job(job_id).await?.account;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), account, job_id).await? else { return Err(JobsError::NotFound) };
            if matches!(job.status, JobStatus::NotStarted | JobStatus::Pending) {
                let before = job.status;
                job.wait_for_capacity(retry::next_pass(now), now)?;
                Self::save(uow.as_mut(), &mut job, before, now).await?;
            }
            Ok::<_, JobsError>(job)
        }
        .await;
        match result {
            Ok(job) => {
                uow.commit().await?;
                Ok(JobResult { job_id, status: job.status, estimated_completion: None, error: None })
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    async fn fail_job(&self, job_id: JobId, failure: Value) -> Result<JobResult, JobsError> {
        let now = self.now();
        let message = failure.get("message").and_then(Value::as_str).map(str::to_string);
        let account = self.load_job(job_id).await?.account;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), account, job_id).await? else { return Err(JobsError::NotFound) };
            if matches!(job.status, JobStatus::NotStarted | JobStatus::Pending) {
                let before = job.status;
                job.fail_submission(failure.clone(), now)?;
                Self::save(uow.as_mut(), &mut job, before, now).await?;
            }
            Ok::<_, JobsError>(job)
        }
        .await;
        match result {
            Ok(job) => {
                uow.commit().await?;
                Ok(JobResult { job_id, status: job.status, estimated_completion: None, error: message })
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    fn callback_url(&self, provider: Provider, job: JobId, attempt: AttemptId) -> String {
        let path = match provider {
            Provider::Deepgram => "deepgram",
            _ => "soniox",
        };
        format!("{}/api/{path}/callback?job_id={}&attempt={attempt}", self.config.callback_base_url.trim_end_matches('/'), job.0)
    }

    /// One attempt: claim (transaction), provider call (no transaction),
    /// acknowledgement (transaction).
    async fn attempt(&self, job_id: JobId, provider: Provider) -> Result<AttemptResult, JobsError> {
        let now = self.now();
        let transcriber = self.transcriber(provider).cloned();
        let project = transcriber.as_ref().map(|t| t.project().to_string()).unwrap_or_default();

        // ---- claim ----
        let account = self.load_job(job_id).await?.account;
        let mut uow = self.s.store.begin().await?;
        let claimed = async {
            if let (Some(t), Some(transport)) = (&transcriber, provider.transport()) {
                uow.lock_capacity(transport, t.project()).await?;
            }
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), account, job_id).await? else { return Err(JobsError::NotFound) };
            if !matches!(job.status, JobStatus::NotStarted | JobStatus::Pending) {
                return Ok(Claim::NotEligible);
            }
            let live = match job.current_attempt {
                Some(id) => uow.lock_attempt(id).await?,
                None => None,
            };
            if live.as_ref().is_some_and(|a| a.is_live(now)) {
                return Ok(Claim::Busy);
            }
            let mut availability = Availability { configured: self.configured(), deepgram_inflight: 0 };
            if provider == Provider::Deepgram && let Some(t) = &transcriber {
                availability.deepgram_inflight = uow.inflight(media_ports::Provider::Deepgram, t.project(), now).await?;
            }
            let executable = routing::executable(&job, provider, &availability);
            if executable == Executable::NoCapacity {
                return Ok(Claim::NoCapacity);
            }
            let audio = self.audio_object(&job);
            let model = self.config.models.model_for(provider, job.account).unwrap_or_default();
            let mut attempt = Attempt {
                id: AttemptId::new(),
                job: job.id,
                account: job.account,
                number: job.attempts_made + 1,
                provider,
                project: project.clone(),
                model,
                state: AttemptState::Claimed,
                claimed_at: now,
                lease_until: Micros(now.0 + CLAIM_LEASE_MINUTES * MICROS_PER_MINUTE),
                request: AttemptRequest {
                    language_code: job.language.map(|l| l.code().to_string()),
                    translation_target: job.translation_target().map(|l| l.code().to_string()),
                    diarize: job.speakers,
                    audio: audio.clone().unwrap_or_else(|| ObjectRef {
                        store: self.config.staging_store,
                        bucket: self.config.staging_bucket.clone(),
                        key: String::new(),
                        version: None,
                    }),
                    client_reference: client_reference(job.id),
                },
                callback_secret: new_secret(),
                remote_id: None,
                accepted_at: None,
                last_error: None,
                remote_cleanup: RemoteCleanup::NotRequested,
            };
            if let Some(previous) = live {
                // An expired, unanswered claim is superseded; its identity stays for erasure.
                if previous.state == AttemptState::Claimed {
                    let mut previous = previous;
                    previous.settle(AttemptState::Superseded, None);
                    uow.save_attempt(&previous).await?;
                }
            }
            let before = job.status;
            match executable {
                Executable::Unavailable | Executable::Incompatible => {
                    attempt.state = if executable == Executable::Unavailable { AttemptState::Unavailable } else { AttemptState::Rejected };
                    let failure = retry::exhausted_failure(
                        if executable == Executable::Unavailable { "provider is not available" } else { "provider cannot handle this job" },
                        provider,
                    );
                    attempt.last_error = Some(failure.clone());
                    uow.insert_attempt(&attempt).await?;
                    job.claim(&attempt, None, now)?;
                    Self::save(uow.as_mut(), &mut job, before, now).await?;
                    Ok(Claim::NotSent(failure))
                }
                _ => {
                    // URL jobs stage into private storage; the target is recorded first.
                    if let JobSource::ExternalUrl { staged: None, .. } = &job.source && let Some(object) = &audio && let JobSource::ExternalUrl { staged, .. } = &mut job.source {
                        *staged = Some(object.clone());
                    }
                    uow.insert_attempt(&attempt).await?;
                    job.claim(&attempt, None, now)?;
                    Self::save(uow.as_mut(), &mut job, before, now).await?;
                    Ok(Claim::Claimed(Box::new(job), Box::new(attempt)))
                }
            }
        }
        .await;
        let (job, attempt) = match claimed {
            Ok(Claim::Claimed(job, attempt)) => {
                uow.commit().await?;
                (*job, *attempt)
            }
            Ok(Claim::NotSent(failure)) => {
                uow.commit().await?;
                return Ok(AttemptResult::Failed(failure));
            }
            Ok(Claim::NoCapacity) => {
                Self::rollback(uow).await;
                return Ok(AttemptResult::Waiting);
            }
            Ok(Claim::Busy) | Ok(Claim::NotEligible) => {
                Self::rollback(uow).await;
                return Ok(AttemptResult::Busy);
            }
            Err(e) => {
                Self::rollback(uow).await;
                return Err(e);
            }
        };
        let transcriber = transcriber.expect("executable provider has a transcriber");

        // ---- call (no transaction) ----
        let outcome = match self.signed_audio(&job).await {
            Ok(audio) => {
                let request = TranscriptionRequest {
                    attempt: attempt.id.0,
                    provider: transcriber.provider(),
                    project: attempt.project.clone(),
                    model: attempt.model.clone(),
                    language: attempt.request.language_code.clone(),
                    translation_target: attempt.request.translation_target.clone(),
                    diarize: attempt.request.diarize,
                    audio,
                    client_reference: attempt.request.client_reference.clone(),
                    callback: CallbackSpec {
                        url: SensitiveUrl::new(self.callback_url(provider, job.id, attempt.id)),
                        secret: attempt.callback_secret.clone(),
                    },
                };
                retry::classify(provider, &transcriber.submit(&request).await)
            }
            Err(failure) => SubmitOutcome::NotSent { failure },
        };

        // ---- acknowledge ----
        self.acknowledge(&attempt, outcome).await
    }

    fn audio_object(&self, job: &Job) -> Option<ObjectRef> {
        match &job.source {
            JobSource::Upload { object: Some(o), .. } => Some(o.clone()),
            JobSource::Upload { object: None, storage } => store_kind(*storage).map(|kind| ObjectRef {
                store: kind,
                bucket: self.config.upload_bucket.clone(),
                key: sources::upload_key(job.account, job.id, job.name.as_deref().unwrap_or("")),
                version: None,
            }),
            JobSource::ExternalUrl { staged: Some(o), .. } => Some(o.clone()),
            JobSource::ExternalUrl { staged: None, .. } => Some(ObjectRef {
                store: self.config.staging_store,
                bucket: self.config.staging_bucket.clone(),
                key: format!("staging/{}/{}", job.account, job.id.0),
                version: None,
            }),
            JobSource::YouTube { .. } => None,
        }
    }

    /// API-015, D10: the callback's `audio_url`, signed at send for the stored
    /// object only (nothing is fetched or staged for a callback). `None` when
    /// the job has no stored audio; the source sent the body without it then.
    pub async fn callback_audio_url(&self, job: &Job) -> Option<String> {
        let object = self.audio_object(job)?;
        let store = self.object_store_for(&object)?;
        store.head(&object).await.ok()??;
        let access = store.sign(&object, AccessMethod::Get, to_media(self.now()), sources::PROVIDER_URL_TTL_SECONDS).await.ok()?;
        Some(access.url.expose().to_string())
    }

    /// JOB-043: a fresh 3600-second GET for the provider. External URLs are
    /// fetched through the controlled stager first (destination and redirect
    /// checks); the provider never receives the customer's URL.
    async fn signed_audio(&self, job: &Job) -> Result<TemporaryAccess, Value> {
        let now = self.now();
        let not_sent = |m: String| json!({ "message": m, "code": "provider_unavailable" });
        let object = self.audio_object(job).ok_or_else(|| not_sent("job has no audio source".into()))?;
        let store = self.object_store_for(&object).ok_or_else(|| not_sent(format!("object store {:?} is not configured", object.store)))?;
        if let JobSource::ExternalUrl { file_path, .. } = &job.source {
            // JOB-043: `file_path`, else the legacy `callback_url`.
            let source = file_path.clone().filter(|p| !p.is_empty()).or_else(|| job.callback_url.clone());
            let source = source.ok_or_else(|| not_sent("external job has no audio URL".into()))?;
            if store.head(&object).await.map_err(|e| not_sent(e.to_string()))?.is_none() {
                let stager = self.s.stager.as_ref().ok_or_else(|| not_sent("external URL staging is not configured".into()))?;
                stager
                    .stage(&SensitiveUrl::new(source), &object, store.as_ref())
                    .await
                    .map_err(|e| json!({ "message": e.to_string(), "code": "invalid_request" }))?;
            }
        }
        store
            .sign(&object, AccessMethod::Get, to_media(now), sources::PROVIDER_URL_TTL_SECONDS)
            .await
            .map_err(|e| not_sent(e.to_string()))
    }

    async fn acknowledge(&self, claimed: &Attempt, outcome: SubmitOutcome) -> Result<AttemptResult, JobsError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            // F3c: under an erasure barrier nothing is applied (and billing refuses the
            // account), but an accepted remote is still learned for cleanup and erasure.
            if uow.lock_barrier(claimed.account).await?.is_some() {
                if let (SubmitOutcome::Accepted { remote_id }, Some(transport)) = (&outcome, claimed.provider.transport()) {
                    let remote = media_ports::RemoteIdentity { provider: transport, project: claimed.project.clone(), id: remote_id.clone() };
                    Self::learn_remote_under_barrier(uow.as_mut(), claimed, &remote).await?;
                }
                return Ok(AttemptResult::Busy);
            }
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), claimed.account, claimed.job).await? else {
                return Err(JobsError::NotFound);
            };
            let Some(mut attempt) = uow.lock_attempt(claimed.id).await? else { return Err(JobsError::NotFound) };
            let current = job.current_attempt == Some(attempt.id);
            let before = job.status;
            let r = match outcome {
                SubmitOutcome::Accepted { remote_id } => {
                    let open = matches!(attempt.state, AttemptState::Claimed | AttemptState::Uncertain);
                    if open && current && matches!(job.status, JobStatus::NotStarted | JobStatus::Pending) {
                        attempt.accept(&remote_id, now)?;
                        job.accept(&attempt, &remote_id, now)?;
                        Self::save(uow.as_mut(), &mut job, before, now).await?;
                        AttemptResult::Accepted(Box::new(job.clone()))
                    } else if attempt.state == AttemptState::Accepted && attempt.remote_id.as_deref() == Some(remote_id.as_str()) {
                        // The callback already adopted this identity (W3): nothing changes.
                        AttemptResult::Busy
                    } else {
                        // F3a: a late acknowledgement cannot overwrite a newer attempt,
                        // completion, cancellation or deletion, but its identity is learned
                        // and that remote copy cleaned up. A different bound identity is a conflict.
                        if attempt.learn_remote(&remote_id)? {
                            attempt.remote_cleanup = RemoteCleanup::Pending;
                        }
                        if open {
                            attempt.settle(AttemptState::Superseded, None);
                        }
                        AttemptResult::Busy
                    }
                }
                SubmitOutcome::RateLimited { details, http_status } => {
                    attempt.settle(AttemptState::RateLimited, Some(json!({ "message": "Rate limit exceeded", "httpStatus": http_status })));
                    if current {
                        job.rate_limited(&attempt, details.as_deref(), http_status, retry::next_pass(now), now)?;
                        Self::save(uow.as_mut(), &mut job, before, now).await?;
                    }
                    AttemptResult::RateLimited
                }
                SubmitOutcome::Rejected { failure } => {
                    attempt.settle(AttemptState::Rejected, Some(failure.clone()));
                    AttemptResult::Failed(failure)
                }
                SubmitOutcome::NotSent { failure } => {
                    attempt.settle(AttemptState::Unavailable, Some(failure.clone()));
                    AttemptResult::Failed(failure)
                }
                SubmitOutcome::Uncertain { reason } => {
                    attempt.settle(AttemptState::Uncertain, Some(json!({ "message": reason })));
                    AttemptResult::Uncertain
                }
            };
            uow.save_attempt(&attempt).await?;
            Ok::<_, JobsError>(r)
        }
        .await;
        match result {
            Ok(r) => {
                uow.commit().await?;
                Ok(r)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// F3c: an authenticated remote identity that arrives once the account's erasure
    /// barrier exists. While the attempt row is still there it learns the identity
    /// (the cascade's manifest then captures it). Once the cascade removed the row,
    /// or when the row already holds another identity, it becomes an erasure item of
    /// its own and a closed erasure reopens, so the remote work is still deleted.
    /// No billing lock, no lifecycle change.
    async fn learn_remote_under_barrier(uow: &mut dyn JobUow, claimed: &Attempt, remote: &media_ports::RemoteIdentity) -> Result<(), JobsError> {
        if let Some(mut a) = uow.lock_attempt(claimed.id).await? && a.learn_remote(&remote.id).is_ok() {
            a.remote_cleanup = match a.remote_cleanup {
                RemoteCleanup::NotRequested => RemoteCleanup::Pending,
                other => other,
            };
            if matches!(a.state, AttemptState::Claimed | AttemptState::Uncertain) {
                a.settle(AttemptState::Superseded, None);
            }
            uow.save_attempt(&a).await?;
            return Ok(());
        }
        let Some(mut record) = uow.lock_barrier(claimed.account).await? else { return Ok(()) };
        let items = uow.erasure_items(record.id).await?;
        if items.iter().any(|i| matches!(&i.target, ItemTarget::ProviderCopy { remote: r, .. } if r == remote)) {
            return Ok(());
        }
        let item = ErasureItem {
            id: Uuid::new_v4(),
            erasure: record.id,
            target: ItemTarget::ProviderCopy { provider: claimed.provider, remote: remote.clone() },
            state: ItemState::Pending,
            attempts: 0,
        };
        uow.insert_erasure_items(std::slice::from_ref(&item)).await?;
        if matches!(record.state, ErasureState::FullyErased | ErasureState::ErasedWithExceptions) {
            record.state = ErasureState::LocalErased;
            record.closed_at = None;
            uow.save_erasure(&record).await?;
        }
        Ok(())
    }

    /// JOB-018/019 and the C4 correction: one pass over due `pending` and
    /// orphaned `not_started` jobs of every provider, pending before not_started,
    /// oldest first. Deepgram keeps its slot budget, five submissions per pass,
    /// one second apart, and stops at its first rate limit.
    pub async fn dispatch_pass(&self) -> Result<PassReport, JobsError> {
        let now = self.now();
        let candidates = self.s.store.dispatch_candidates(now, retry::PASS_SCAN_LIMIT).await?;
        let mut report = PassReport::default();
        let mut deepgram_budget = {
            let job = candidates.first();
            match job {
                Some(j) => self.availability(j).await?.deepgram_free_slots().min(retry::DEEPGRAM_PER_PASS),
                None => 0,
            }
        };
        let mut deepgram_stopped = false;
        let mut previous_deepgram = false;
        for job in candidates {
            report.examined += 1;
            let availability = self.availability(&job).await?;
            let first = match routing::first_choice(&job, &Preferences::default(), &availability, &self.config.routing) {
                Route::Submit { provider, .. } => Some(provider),
                _ => None,
            };
            let is_deepgram = first == Some(Provider::Deepgram);
            if is_deepgram && (deepgram_stopped || deepgram_budget == 0) {
                report.waiting += 1;
                continue;
            }
            if is_deepgram && previous_deepgram {
                self.s.sleeper.sleep(retry::DEEPGRAM_SPACING_MS).await;
            }
            match self.dispatch(job.id, Preferences::default()).await {
                Ok(r) => match r.status {
                    JobStatus::Running => {
                        report.submitted += 1;
                        if is_deepgram {
                            deepgram_budget -= 1;
                        }
                    }
                    JobStatus::Pending if r.error.is_some() => {
                        report.rate_limited += 1;
                        if is_deepgram {
                            deepgram_stopped = true;
                        }
                    }
                    JobStatus::Failed => report.failed += 1,
                    _ => report.waiting += 1,
                },
                Err(_) => report.errors += 1,
            }
            previous_deepgram = is_deepgram;
        }
        Ok(report)
    }

    /// Claims whose lease expired unanswered and uncertain attempts (W2, C5).
    /// Soniox is reconciled by its listing: exactly one record that agrees with
    /// everything the attempt sent (`correlate::matches`) is adopted. None,
    /// several or a failed listing leave the attempt unresolved, at any age: a
    /// miss is not proof of nonacceptance, so it is never redispatched (F5).
    /// Only a provider without a listing (Deepgram) gives up after the 30-minute
    /// stuck window: the attempt is superseded (its identity kept) and the job
    /// dispatched again, which may buy duplicate provider work.
    pub async fn reconcile_pass(&self) -> Result<PassReport, JobsError> {
        let now = self.now();
        let mut report = PassReport::default();
        for attempt in self.s.store.unresolved_attempts(now, retry::PASS_SCAN_LIMIT).await? {
            report.examined += 1;
            match self.find_remote(&attempt).await {
                ListingOutcome::Found(remote_id) => {
                    match self.acknowledge(&attempt, SubmitOutcome::Accepted { remote_id }).await {
                        Ok(_) => report.recovered += 1,
                        Err(_) => report.errors += 1,
                    }
                    continue;
                }
                ListingOutcome::Failed => {
                    report.errors += 1;
                    continue;
                }
                ListingOutcome::Unresolved => continue,
                ListingOutcome::NoListing => {}
            }
            let window = crate::jobs::domain::STUCK_AFTER_MINUTES * MICROS_PER_MINUTE;
            if now.0 - attempt.claimed_at.0 < window {
                continue;
            }
            match self.supersede(&attempt).await {
                Ok(true) => match self.dispatch(attempt.job, Preferences::default()).await {
                    Ok(_) => report.submitted += 1,
                    Err(_) => report.errors += 1,
                },
                Ok(false) => {}
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    async fn find_remote(&self, attempt: &Attempt) -> ListingOutcome {
        if !matches!(attempt.provider, Provider::Soniox | Provider::SonioxV5) {
            return ListingOutcome::NoListing;
        }
        // A Soniox attempt whose transport is not configured cannot be listed now;
        // that is not a miss either.
        let Some(transcriber) = self.transcriber(attempt.provider) else { return ListingOutcome::Failed };
        let Ok(records) = transcriber.find_by_reference(&attempt.request.client_reference).await else { return ListingOutcome::Failed };
        let matching: Vec<_> = records.iter().filter(|r| correlate::matches(r, attempt)).collect();
        match matching.as_slice() {
            [one] => ListingOutcome::Found(one.remote.id.clone()),
            _ => ListingOutcome::Unresolved,
        }
    }

    async fn supersede(&self, attempt: &Attempt) -> Result<bool, JobsError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(job) = Self::lock_job_after_account(uow.as_mut(), attempt.account, attempt.job).await? else { return Ok(false) };
            let Some(mut a) = uow.lock_attempt(attempt.id).await? else { return Ok(false) };
            if !matches!(a.state, AttemptState::Claimed | AttemptState::Uncertain) || job.current_attempt != Some(a.id) {
                return Ok(false);
            }
            a.settle(AttemptState::Superseded, Some(json!({ "message": "no answer within the stuck window", "at": crate::time::iso8601(now) })));
            uow.save_attempt(&a).await?;
            Ok::<_, JobsError>(matches!(job.status, JobStatus::NotStarted | JobStatus::Pending))
        }
        .await;
        match result {
            Ok(r) => {
                uow.commit().await?;
                Ok(r)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    // ---- callbacks, ingestion and recovery -------------------------------------

    /// Durable intake of an authenticated callback (C-6). The host loads the
    /// attempt named by the callback query, verifies the request with the
    /// attempt's binding through the transport, then calls this before replying.
    pub async fn intake(&self, attempt: &Attempt, observation: ProviderObservation, fingerprint: String) -> Result<IntakeAck, JobsError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            // The host barrier comes first: once erasure opened, billing refuses the account lock.
            if uow.lock_barrier(attempt.account).await?.is_some() {
                // The account is erased: keep only the remote identity for cleanup (F3c).
                Self::learn_remote_under_barrier(uow.as_mut(), attempt, &observation.remote).await?;
                return Ok(IntakeAck::Erased);
            }
            uow.billing().await?.lock_account(attempt.account).await?;
            let entry = NewInboxEntry {
                attempt: attempt.id,
                job: attempt.job,
                account: attempt.account,
                remote_id: observation.remote.id.clone(),
                origin: observation.origin,
                observation,
                fingerprint: fingerprint.clone(),
                received_at: now,
            };
            let (stored, new) = uow.insert_inbox(&entry).await?;
            if new {
                return Ok(IntakeAck::Stored { inbox: stored.id });
            }
            if stored.entry.fingerprint == fingerprint {
                Ok(IntakeAck::Duplicate { inbox: stored.id })
            } else {
                // F6: a different body for the same delivery is held, never applied.
                uow.hold_inbox_conflict(stored.id, &entry).await?;
                Ok(IntakeAck::Conflict { inbox: stored.id })
            }
        }
        .await;
        match result {
            Ok(ack) => {
                uow.commit().await?;
                Ok(ack)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// Applies a stored callback. Soniox's transcript is fetched first (no
    /// transaction) while its webhook-reported duration stays as received (D6).
    pub async fn ingest(&self, inbox: InboxId) -> Result<(), JobsError> {
        let entry = {
            let mut uow = self.s.store.begin().await?;
            let e = uow.lock_inbox(inbox).await;
            let _ = uow.rollback().await;
            e?.ok_or(JobsError::NotFound)?
        };
        if entry.state != InboxState::Pending {
            return Ok(());
        }
        let attempt = self.s.store.attempt(entry.entry.attempt).await?.ok_or(JobsError::NotFound)?;
        let mut observation = entry.entry.observation.clone();
        if let ProviderPayload::Soniox { reported, transcript: None } = &observation.payload && reported.status == "completed" {
            let transcriber = self
                .transcriber(attempt.provider)
                .ok_or_else(|| JobsError::NotConfigured("Soniox transport is not configured".into()))?;
            match transcriber.complete_callback(observation.clone()).await {
                Ok(completed) => observation = completed,
                Err(media_ports::TransportError::Unavailable { operation, .. }) => {
                    self.inbox_failure(inbox, &format!("transcript fetch unavailable: {operation}")).await?;
                    return Ok(());
                }
                Err(e) => {
                    // JOB-029: a definite fetch failure is a coded ingestion failure.
                    return self.fail_ingestion(&entry, &attempt, &e.to_string()).await;
                }
            }
        }
        let applied = self.apply(&attempt, &observation, Some(inbox)).await;
        match applied {
            Ok(()) => {
                if self.config.soniox_auto_delete
                    && entry.entry.origin == ObservationOrigin::Webhook
                    && matches!(&observation.payload, ProviderPayload::Soniox { reported, .. } if reported.status == "completed")
                {
                    let _ = self.delete_remote_copy(attempt.id).await;
                }
                Ok(())
            }
            Err(JobsError::Store(StoreError::Unavailable(m))) => {
                self.inbox_failure(inbox, &m).await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn inbox_failure(&self, inbox: InboxId, message: &str) -> Result<(), JobsError> {
        let mut uow = self.s.store.begin().await?;
        let r = uow.record_inbox_failure(inbox, message).await;
        match r {
            Ok(()) => uow.commit().await?,
            Err(e) => {
                Self::rollback(uow).await;
                return Err(e.into());
            }
        }
        Ok(())
    }

    async fn fail_ingestion(&self, entry: &InboxEntry, attempt: &Attempt, message: &str) -> Result<(), JobsError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), attempt.account, attempt.job).await? else {
                return Err(JobsError::NotFound);
            };
            if job.current_attempt == Some(attempt.id) && job.status.accepts_results() {
                let before = job.status;
                job.fail_result(attempt.id, ingest::ingestion_error(message), now)?;
                Self::save(uow.as_mut(), &mut job, before, now).await?;
            }
            uow.settle_inbox(entry.id, InboxState::Applied, Some(message), now).await?;
            Ok::<_, JobsError>(())
        }
        .await;
        match result {
            Ok(()) => {
                uow.commit().await?;
                Ok(())
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// Pending inbox entries, oldest first (restart and transient-failure recovery).
    pub async fn ingest_pass(&self) -> Result<PassReport, JobsError> {
        let mut report = PassReport::default();
        for entry in self.s.store.pending_inbox(retry::PASS_SCAN_LIMIT).await? {
            report.examined += 1;
            match self.ingest(entry.id).await {
                Ok(()) => report.recovered += 1,
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    /// One transaction: duration rule (deferred charge in a savepoint), lines,
    /// status and outbox (JOB-028, seam 5). Lines never reach a job that is no
    /// longer waiting for this attempt's result.
    async fn apply(&self, attempt: &Attempt, observation: &ProviderObservation, inbox: Option<InboxId>) -> Result<(), JobsError> {
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            // The host barrier first: once erasure opened, billing refuses the account lock.
            if uow.lock_barrier(attempt.account).await?.is_some() {
                Self::learn_remote_under_barrier(uow.as_mut(), attempt, &observation.remote).await?;
                if let Some(id) = inbox {
                    uow.settle_inbox(id, InboxState::Ignored, Some("account erased"), now).await?;
                }
                return Ok(None);
            }
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), attempt.account, attempt.job).await? else {
                return Err(JobsError::NotFound);
            };
            let Some(mut a) = uow.lock_attempt(attempt.id).await? else { return Err(JobsError::NotFound) };
            if let Some(id) = inbox {
                match uow.lock_inbox(id).await? {
                    Some(e) if e.state == InboxState::Pending => {}
                    _ => return Ok(None),
                }
            }
            // Callback before the acknowledgement committed (W3): adopt the remote id.
            if job.current_attempt == Some(a.id) && matches!(a.state, AttemptState::Claimed | AttemptState::Uncertain) {
                a.accept(&observation.remote.id, now)?;
                if matches!(job.status, JobStatus::NotStarted | JobStatus::Pending) {
                    let before = job.status;
                    job.accept(&a, &observation.remote.id, now)?;
                    Self::save(uow.as_mut(), &mut job, before, now).await?;
                }
            }
            let stale = job.current_attempt != Some(a.id) || a.remote_id.as_deref().is_some_and(|r| r != observation.remote.id);
            let plan = if stale {
                IngestPlan::Ignore { reason: "result of a superseded attempt".into() }
            } else {
                ingest::plan(&job, observation)
            };
            let before = job.status;
            let mut archive: Option<(&'static str, Value)> = None;
            match plan {
                IngestPlan::Ignore { reason } => {
                    // F3b: the identity this result names is learned and cleaned up. An
                    // attempt already bound to another identity cannot hold a second one,
                    // so the delivery itself is held as a conflict (it keeps the remote id).
                    let (state, note) = match a.learn_remote(&observation.remote.id) {
                        Ok(_) => {
                            a.remote_cleanup = match a.remote_cleanup {
                                RemoteCleanup::NotRequested => RemoteCleanup::Pending,
                                other => other,
                            };
                            (InboxState::Ignored, reason)
                        }
                        Err(e) => (InboxState::Conflict, e.to_string()),
                    };
                    if let Some(id) = inbox {
                        uow.settle_inbox(id, state, Some(&note), now).await?;
                    }
                    uow.save_attempt(&a).await?;
                    return Ok(None);
                }
                IngestPlan::ProviderFailed { failure } => {
                    job.fail_result(a.id, failure.clone(), now)?;
                    a.settle(AttemptState::Finished, Some(failure));
                }
                IngestPlan::Complete { duration, lines, archive_name } => {
                    self.apply_duration(uow.as_mut(), &mut job, duration, now).await?;
                    archive = Some((archive_name, serde_json::to_value(&observation.payload).unwrap_or(Value::Null)));
                    match self.insert_lines(uow.as_mut(), &job, &lines).await? {
                        Ok(()) => {
                            job.complete(a.id, now)?;
                            // D5: the durable producer of the general summary outcome.
                            crate::ai::Ai::request_summary(uow.as_mut(), job.account, job.id, now).await?;
                        }
                        Err(message) => job.fail_result(a.id, ingest::insert_failure(&job, observation, &message, lines.len()), now)?,
                    }
                    a.settle(AttemptState::Finished, None);
                }
                IngestPlan::NoSegments { duration, failure, archive_name } => {
                    self.apply_duration(uow.as_mut(), &mut job, duration, now).await?;
                    archive = Some((archive_name, serde_json::to_value(&observation.payload).unwrap_or(Value::Null)));
                    job.fail_result(a.id, failure.clone(), now)?;
                    a.settle(AttemptState::Finished, Some(failure));
                }
            }
            Self::save(uow.as_mut(), &mut job, before, now).await?;
            uow.save_attempt(&a).await?;
            if let Some(id) = inbox {
                uow.settle_inbox(id, InboxState::Applied, None, now).await?;
            }
            // F2: the archive object is recorded with the result, before the PUT.
            let archive = match archive {
                Some((name, body)) => {
                    let intent = self.archive_intent(&job, name);
                    uow.record_archived_payload(&intent).await?;
                    Some((intent, body))
                }
                None => None,
            };
            Ok::<_, JobsError>(archive)
        }
        .await;
        match result {
            Ok(archive) => {
                uow.commit().await?;
                if let Some((intent, body)) = archive {
                    self.archive_payload(&intent, body).await;
                }
                Ok(())
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// BIL-034..037: the duration decision. The strict deferred debit runs in a
    /// savepoint: a refusal or storage error there leaves delivery intact (BIL-036).
    async fn apply_duration(&self, uow: &mut dyn JobUow, job: &mut Job, decision: DurationDecision, now: Micros) -> Result<(), JobsError> {
        match decision {
            DurationDecision::Keep => {}
            DurationDecision::NoDuration => {
                job.deferred_charge = duration::deferred_after(job, decision, None);
            }
            DurationDecision::Lower { minutes } => job.set_duration(minutes as i32, now),
            DurationDecision::SetAndCharge { minutes } => {
                job.set_duration(minutes as i32, now);
                uow.savepoint("deferred_debit").await?;
                let charged = {
                    let mut billing = uow.billing().await?;
                    charge_strict(&mut *billing, job.account, job.id.subject(), minutes, now).await
                };
                let ok = match charged {
                    Ok(_) => {
                        uow.release_savepoint("deferred_debit").await?;
                        true
                    }
                    Err(_) => {
                        uow.rollback_to_savepoint("deferred_debit").await?;
                        false
                    }
                };
                job.deferred_charge = duration::deferred_after(job, decision, Some(ok));
            }
        }
        Ok(())
    }

    /// TRX-032: one insert for the transcript. A deterministic failure (a
    /// constraint) fails the job with `database_error`; an unavailable database
    /// aborts the whole ingestion for a retry.
    async fn insert_lines(&self, uow: &mut dyn JobUow, job: &Job, lines: &[NewLine]) -> Result<Result<(), String>, JobsError> {
        uow.savepoint("lines").await?;
        match uow.insert_lines(job, lines).await {
            Ok(_) => {
                uow.release_savepoint("lines").await?;
                Ok(Ok(()))
            }
            Err(StoreError::Conflict(m)) => {
                uow.rollback_to_savepoint("lines").await?;
                Ok(Err(m))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// TRX-041, PRV-037: delete the provider copy and record the outcome.
    pub async fn delete_remote_copy(&self, attempt: AttemptId) -> Result<RemoteCleanup, JobsError> {
        let Some(a) = self.s.store.attempt(attempt).await? else { return Err(JobsError::NotFound) };
        let Some(remote) = a.remote() else { return Ok(a.remote_cleanup) };
        let transcriber = self.transcriber(a.provider).ok_or_else(|| JobsError::NotConfigured("transport".into()))?;
        let now = self.now();
        let outcome = match transcriber.delete_remote(&remote).await {
            Ok(media_ports::DeleteOutcome::Deleted) => RemoteCleanup::Deleted { at: now },
            Ok(media_ports::DeleteOutcome::Absent) => RemoteCleanup::Absent { at: now },
            Ok(media_ports::DeleteOutcome::Retained { reason, .. }) => RemoteCleanup::Retained { reason },
            Ok(media_ports::DeleteOutcome::Unsupported { reason }) => RemoteCleanup::Unsupported { reason },
            Err(e) => RemoteCleanup::Failed {
                attempts: match &a.remote_cleanup {
                    RemoteCleanup::Failed { attempts, .. } => attempts + 1,
                    _ => 1,
                },
                last_error: e.to_string(),
            },
        };
        let mut uow = self.s.store.begin().await?;
        let saved = async {
            let Some(mut locked) = uow.lock_attempt(attempt).await? else { return Ok::<_, StoreError>(()) };
            locked.remote_cleanup = outcome.clone();
            uow.save_attempt(&locked).await
        }
        .await;
        match saved {
            Ok(()) => uow.commit().await?,
            Err(e) => {
                Self::rollback(uow).await;
                return Err(e.into());
            }
        }
        Ok(outcome)
    }

    /// Retries provider-copy deletions that were requested but not finished.
    pub async fn remote_cleanup_pass(&self) -> Result<PassReport, JobsError> {
        let mut report = PassReport::default();
        for a in self.s.store.remote_cleanup_due(retry::PASS_SCAN_LIMIT).await? {
            report.examined += 1;
            match self.delete_remote_copy(a.id).await {
                Ok(_) => report.recovered += 1,
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    /// JOB-020..025: stuck running jobs, each in isolation. Deepgram resets to
    /// pending; Soniox claims, asks for the status and re-ingests or gives the
    /// claim back.
    pub async fn stuck_pass(&self) -> Result<PassReport, JobsError> {
        let now = self.now();
        let before = Micros(now.0 - crate::jobs::domain::STUCK_AFTER_MINUTES * MICROS_PER_MINUTE);
        let mut report = PassReport::default();
        for job in self.s.store.stuck_candidates(before, retry::PASS_SCAN_LIMIT).await? {
            report.examined += 1;
            let r = match retry::stuck_action(&job) {
                StuckAction::ResetDeepgram => self.reset_deepgram(job.id).await,
                StuckAction::FailSoniox { code, message, details } => self.fail_recovery(job.id, code, &message, details).await,
                StuckAction::RecoverSoniox { expected_attempts, transcription_id } => {
                    self.recover_soniox(job.id, expected_attempts, &transcription_id).await
                }
                StuckAction::None => Ok(()),
            };
            match r {
                Ok(()) => report.recovered += 1,
                Err(_) => report.errors += 1,
            }
        }
        Ok(report)
    }

    async fn with_job<F>(&self, id: JobId, f: F) -> Result<(), JobsError>
    where
        F: for<'u> FnOnce(&'u mut Job, Micros) -> Result<bool, JobsError>,
    {
        let now = self.now();
        let account = self.load_job(id).await?.account;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            let Some(mut job) = Self::lock_job_after_account(uow.as_mut(), account, id).await? else { return Err(JobsError::NotFound) };
            let before = job.status;
            if f(&mut job, now)? {
                Self::save(uow.as_mut(), &mut job, before, now).await?;
            }
            Ok::<_, JobsError>(())
        }
        .await;
        match result {
            Ok(()) => {
                uow.commit().await?;
                Ok(())
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    async fn reset_deepgram(&self, id: JobId) -> Result<(), JobsError> {
        self.with_job(id, |job, now| {
            if crate::jobs::domain::is_stuck(job, now) && job.provider == Some(Provider::Deepgram) {
                job.reset_stuck_deepgram(now)?;
                job.current_attempt = None;
                Ok(true)
            } else {
                Ok(false)
            }
        })
        .await
    }

    async fn fail_recovery(&self, id: JobId, code: &'static str, message: &str, details: Value) -> Result<(), JobsError> {
        self.with_job(id, |job, now| {
            if job.status != JobStatus::Running {
                return Ok(false);
            }
            job.fail_recovery(code, message, now)?;
            if let Some(e) = job.error.as_mut() {
                e["details"] = details;
            }
            Ok(true)
        })
        .await
    }

    async fn recover_soniox(&self, id: JobId, expected: i32, transcription_id: &str) -> Result<(), JobsError> {
        // Claim: compare-and-swap of recovery_attempts (JOB-024).
        let mut claimed = false;
        self.with_job(id, |job, now| {
            if job.status != JobStatus::Running || job.recovery_attempts != expected {
                return Ok(false);
            }
            job.claim_recovery(expected, now)?;
            claimed = true;
            Ok(true)
        })
        .await?;
        if !claimed {
            return Ok(());
        }
        let job = self.load_job(id).await?;
        let provider = job.provider.unwrap_or(Provider::Soniox);
        let transcriber = self.transcriber(provider).ok_or_else(|| JobsError::NotConfigured("Soniox".into()))?.clone();
        let attempt = match job.current_attempt {
            Some(a) => self.s.store.attempt(a).await?,
            None => None,
        };
        let remote = media_ports::RemoteIdentity {
            provider: media_ports::Provider::Soniox,
            project: attempt.as_ref().map(|a| a.project.clone()).unwrap_or_else(|| transcriber.project().to_string()),
            id: transcription_id.to_string(),
        };
        let status = match transcriber.status(&remote).await {
            Ok(o) => o,
            Err(e) => {
                self.release_recovery(id).await?;
                return Err(JobsError::Upstream(e.to_string()));
            }
        };
        let reported_status = match &status.payload {
            ProviderPayload::Soniox { reported, .. } => reported.status.clone(),
            _ => String::new(),
        };
        if retry::recovery_status(&reported_status) == retry::RecoveryStatus::StillWorking {
            return self.release_recovery(id).await;
        }
        // Terminal: fetch the result on the recovery path. A failed ingest keeps the increment.
        let observation = if reported_status == "completed" {
            match transcriber.result(&remote).await {
                Ok(mut o) => {
                    if let (ProviderPayload::Soniox { reported, .. }, ProviderPayload::Soniox { reported: polled, .. }) =
                        (&mut o.payload, &status.payload)
                    {
                        // Recovery keeps its own polled observation (D6).
                        *reported = polled.clone();
                    }
                    o.origin = ObservationOrigin::RecoveryPoll;
                    o
                }
                Err(e) => return Err(JobsError::Upstream(e.to_string())),
            }
        } else {
            let mut o = status;
            o.origin = ObservationOrigin::RecoveryPoll;
            o
        };
        let attempt = match attempt {
            Some(a) => a,
            None => return Err(JobsError::WrongState("imported job without an attempt record".into())),
        };
        self.apply(&attempt, &observation, None).await
    }

    async fn release_recovery(&self, id: JobId) -> Result<(), JobsError> {
        self.with_job(id, |job, now| {
            job.release_recovery(now);
            Ok(true)
        })
        .await
    }

    /// JOB-040: canceling exists as a core operation; no route or screen calls it.
    pub async fn cancel(&self, account: AccountId, id: JobId) -> Result<Job, JobsError> {
        self.guard(account).await?;
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            let mut jobs = uow.lock_jobs(account, &[id]).await?;
            let Some(job) = jobs.first_mut() else { return Err(JobsError::NotFound) };
            let before = job.status;
            job.cancel(now)?;
            Self::save(uow.as_mut(), job, before, now).await?;
            Ok::<_, JobsError>(job.clone())
        }
        .await;
        match result {
            Ok(j) => {
                uow.commit().await?;
                Ok(j)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    /// JOB-036, PRV-011: soft delete; charges stay (BIL-042).
    pub async fn delete_job(&self, account: AccountId, id: JobId) -> Result<(), JobsError> {
        self.delete_jobs(account, Some(id)).await.map(|_| ())
    }

    /// PRV-003/011 "Delete all": every job not yet deleted.
    pub async fn delete_all(&self, account: AccountId) -> Result<u32, JobsError> {
        self.delete_jobs(account, None).await
    }

    async fn delete_jobs(&self, account: AccountId, only: Option<JobId>) -> Result<u32, JobsError> {
        self.guard(account).await?;
        let now = self.now();
        let ids: Vec<JobId> = match only {
            Some(id) => vec![id],
            None => {
                let query = JobListQuery { account, statuses: JobStatus::ALL.to_vec(), since: None, before: None, limit: u32::MAX };
                self.s.store.list_jobs(&query).await?.into_iter().map(|j| j.id).collect()
            }
        };
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            let mut jobs = uow.lock_jobs(account, &ids).await?;
            if only.is_some() && jobs.is_empty() {
                return Err(JobsError::NotFound);
            }
            let mut n = 0;
            for job in jobs.iter_mut() {
                let before = job.status;
                if job.soft_delete(now) {
                    Self::save(uow.as_mut(), job, before, now).await?;
                    // Active AI work on a deleted source is fenced; completed artifacts stay.
                    uow.ai().bump_source_revision(job.id).await?;
                    n += 1;
                }
            }
            Ok::<_, JobsError>(n)
        }
        .await;
        match result {
            Ok(n) => {
                uow.commit().await?;
                Ok(n)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    // ---- transcript edits (D7) -------------------------------------------------

    /// EDT-002/005/006: last write wins, update-only, any status of an owned job.
    pub async fn edit(&self, account: AccountId, edit: LineEdit) -> Result<u64, JobsError> {
        self.guard(account).await?;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            let job_id = match &edit {
                LineEdit::Text { line, .. } | LineEdit::Speaker { line, .. } => {
                    uow.lock_line(*line).await?.filter(|l| l.account == account).map(|l| l.job).ok_or(JobsError::NotFound)?
                }
                LineEdit::RenameSpeaker { job, .. } => *job,
            };
            if uow.lock_jobs(account, &[job_id]).await?.is_empty() {
                return Err(JobsError::NotFound);
            }
            let n = match &edit {
                LineEdit::Text { line, text } => u64::from(uow.update_line_text(*line, text).await?),
                LineEdit::Speaker { line, speaker } => u64::from(uow.update_line_speaker(*line, speaker).await?),
                LineEdit::RenameSpeaker { job, from, to } => uow.rename_speaker(*job, from, to).await?,
            };
            if n > 0 {
                // An edited transcript fences AI work admitted on the old text.
                uow.ai().bump_source_revision(job_id).await?;
            }
            Ok::<_, JobsError>(n)
        }
        .await;
        match result {
            Ok(n) => {
                uow.commit().await?;
                Ok(n)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    // ---- queries ---------------------------------------------------------------

    /// JOB-039, PRV-032: listed statuses, the one-month browser window unless
    /// `unlimited_history`, id descending, 20 per page.
    pub async fn list(&self, account: AccountId, before: Option<JobId>, calendar: CalendarContext) -> Result<JobPage, JobsError> {
        self.guard(account).await?;
        let settings = self.s.store.settings(account).await?;
        let since = history_since(&settings, self.now(), calendar);
        let query = JobListQuery { account, statuses: JobStatus::LISTED.to_vec(), since, before, limit: PAGE_SIZE };
        let jobs = self.s.store.list_jobs(&query).await?;
        let next_cursor = (jobs.len() as u32 == PAGE_SIZE).then(|| jobs.last().map(|j| j.id)).flatten();
        Ok(JobPage { jobs, next_cursor })
    }

    /// UPL-033, PRV-012, TRX-039: any owned job by id; lines when completed.
    pub async fn detail(&self, account: AccountId, id: JobId) -> Result<JobDetail, JobsError> {
        self.guard(account).await?;
        let job = self.s.store.owned_job(account, id).await?.ok_or(JobsError::NotFound)?;
        let lines = if job.status == JobStatus::Completed { self.s.store.lines(id).await? } else { vec![] };
        Ok(JobDetail { job, lines })
    }

    /// The transcript page reads lines of any owned job (EDT-003 has no status rule).
    pub async fn lines(&self, account: AccountId, id: JobId) -> Result<Vec<Line>, JobsError> {
        self.guard(account).await?;
        self.s.store.owned_job(account, id).await?.ok_or(JobsError::NotFound)?;
        Ok(self.s.store.lines(id).await?)
    }

    /// JOB-003/UPL-008 (`getAllDraftJobs`): every draft of the owner, oldest
    /// first, with no history window or page limit. The queue restores them on
    /// load and rereads them while a duration is pending.
    pub async fn drafts(&self, account: AccountId) -> Result<Vec<Job>, JobsError> {
        self.guard(account).await?;
        let query = JobListQuery { account, statuses: vec![JobStatus::Draft], since: None, before: None, limit: u32::MAX };
        let mut jobs = self.s.store.list_jobs(&query).await?;
        jobs.reverse();
        Ok(jobs)
    }

    /// UPL-021: the user's most used languages and targets.
    pub async fn language_usage(&self, account: AccountId) -> Result<LanguageUsage, JobsError> {
        self.guard(account).await?;
        Ok(self.s.store.language_usage(account).await?)
    }

    // ---- export ----------------------------------------------------------------

    /// EXP-001..016, FRE-022: the owner's completed job as an export model plus
    /// its `transcript_downloaded` event. No freemium or blocked gate: downloads
    /// are never blocked. The host only turns the bundle into zip bytes.
    pub async fn export(&self, account: AccountId, id: JobId, request: ExportRequest) -> Result<(ExportBundle, DownloadEvent), JobsError> {
        self.guard(account).await?;
        let job = self.s.store.owned_job(account, id).await?.ok_or(JobsError::NotFound)?;
        // EXP-001: download is offered only for completed jobs.
        if job.status != JobStatus::Completed {
            return Err(JobsError::WrongState(format!("Job is not completed (current: {})", job.status)));
        }
        let mut lines = self.s.store.lines(id).await?;
        // EXP-013: the transcript page exports only translation lines of a translation job.
        if request.surface == ExportSurface::TranscriptPage && job.is_translation() {
            lines.retain(|l| l.translation_status == Some(TranslationStatus::Translation));
        }
        let name = job.name.as_deref();
        Ok(match request.kind {
            ExportKind::Wysiwyg { show_timestamps, show_speakers } => {
                // `downloadTranscript` defaults: time on, speakers from the job.
                let timestamps = show_timestamps.unwrap_or(true);
                let speakers = show_speakers.unwrap_or(job.speakers);
                (
                    export::wysiwyg(name, job.speakers, &lines, timestamps, speakers, &request.labels),
                    export::download_event(id.0, false, name, &lines, timestamps, speakers),
                )
            }
            ExportKind::AllFormats => (
                export::all_formats(name, job.speakers, &lines, &request.labels),
                export::download_event(id.0, true, name, &lines, true, job.speakers),
            ),
        })
    }

    // ---- YouTube ---------------------------------------------------------------

    async fn youtube_guard(&self, account: AccountId, key: &str, limit: u32) -> Result<(), JobsError> {
        self.guard(account).await?;
        if self.s.store.settings(account).await?.is_blocked() {
            return Err(JobsError::Blocked);
        }
        let decision = self.s.rate_limiter.hit(&format!("{account}:{key}"), limit, 60_000, self.now()).await?;
        if !decision.allowed {
            return Err(JobsError::RateLimited(decision));
        }
        Ok(())
    }

    async fn fetch_video_metadata(&self, video_id: &str) -> Result<Result<VideoMetadata, media_ports::CaptionError>, JobsError> {
        let source = self.s.video_metadata.as_ref().ok_or_else(|| JobsError::NotConfigured("YouTube metadata".into()))?;
        Ok(source.fetch(video_id).await)
    }

    /// `POST /api/youtube/metadata` (RTL-004: 20 per user per minute). The source
    /// checks no blocked flag here, only the session.
    pub async fn youtube_metadata(&self, account: AccountId, raw_input: &str) -> Result<VideoMetadata, JobsError> {
        self.guard(account).await?;
        let decision = self.s.rate_limiter.hit(&format!("{account}:youtube_metadata"), 20, 60_000, self.now()).await?;
        if !decision.allowed {
            return Err(JobsError::RateLimited(decision));
        }
        if raw_input.is_empty() {
            return Err(JobsError::InvalidRequest("videoId is required".into()));
        }
        let video_id =
            sources::youtube_video_id(raw_input).ok_or_else(|| JobsError::InvalidRequest("Invalid YouTube URL or video ID".into()))?;
        match self.fetch_video_metadata(&video_id).await? {
            Ok(m) => Ok(m),
            Err(media_ports::CaptionError::InvalidVideo | media_ports::CaptionError::VideoUnavailable) => Err(JobsError::VideoNotFound),
            Err(_) => Err(JobsError::Failed("Failed to fetch metadata".into())),
        }
    }

    /// UPL-022..025, LNG-026, PRV-051, RTL-004. Answers `{ job, metadata }`.
    pub async fn youtube_add(&self, account: AccountId, url: &str, language: Option<&str>) -> Result<(Job, VideoMetadata), JobsError> {
        self.youtube_guard(account, "youtube_add", 10).await?;
        if url.is_empty() {
            return Err(JobsError::InvalidRequest("youtubeUrl is required".into()));
        }
        let video_id = sources::youtube_video_id(url).ok_or_else(|| JobsError::InvalidRequest("Invalid YouTube URL".into()))?;
        let requested = match language.filter(|l| !l.is_empty()) {
            None => None,
            Some(name) => Some(Language::from_name(name).ok_or_else(|| JobsError::InvalidRequest(format!("Unsupported language: {name}")))?),
        };
        let metadata = match self.fetch_video_metadata(&video_id).await? {
            Ok(m) => m,
            Err(media_ports::CaptionError::InvalidVideo | media_ports::CaptionError::VideoUnavailable) => {
                return Err(JobsError::VideoNotFound);
            }
            Err(_) => return Err(JobsError::Upstream("Failed to fetch video metadata".into())),
        };
        let watch = sources::youtube_watch_url(&video_id);
        let now = self.now();
        let new = NewJob {
            account,
            status: JobStatus::Draft,
            name: Some(metadata.title.clone()),
            source: JobSource::YouTube { video_id: Some(video_id.clone()), youtube_url: Some(watch.clone()), transcript_source: None },
            language: Some(sources::youtube_language(requested, metadata.language.as_deref())),
            translate_to: vec![],
            speakers: false,
            fix_grammer: Some(true),
            duration_minutes: sources::youtube_duration_minutes(metadata.duration_seconds),
            size_bytes: Some(0),
            callback_url: None,
            whisper_params: Some(sources::youtube_whisper_params(&video_id, metadata.language.as_deref())),
            created_at: now,
        };
        let mut uow = self.s.store.begin().await?;
        match uow.insert_job(&new).await {
            Ok(job) => {
                uow.commit().await?;
                Ok((job, metadata))
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e.into())
            }
        }
    }

    /// UPL-026..028, JOB-035: captions resolved into lines and `completed` in one
    /// transaction, never billed; a resolver error fails the draft.
    pub async fn youtube_resolve(&self, account: AccountId, id: JobId) -> Result<ResolveAnswer, JobsError> {
        self.youtube_guard(account, "youtube_resolve", 5).await?;
        let job = self.s.store.owned_job(account, id).await?.ok_or(JobsError::NotFound)?;
        if job.status != JobStatus::Draft {
            return Err(JobsError::WrongState(format!("Job is not in draft status (current: {})", job.status)));
        }
        let video_id = match &job.source {
            JobSource::YouTube { video_id: Some(v), .. } => Some(v.clone()),
            _ => job.whisper_params.as_ref().and_then(|p| p.get("youtube_video_id")).and_then(Value::as_str).map(str::to_string),
        };
        let Some(video_id) = video_id else {
            return Err(JobsError::Unprocessable("Job does not have a YouTube video ID".into()));
        };
        let captions_source = self.s.paid_captions.as_ref().ok_or_else(|| JobsError::NotConfigured("YouTube captions".into()))?;
        let fetched = captions_source.fetch(&video_id).await;
        let now = self.now();
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            let mut jobs = uow.lock_jobs(account, &[id]).await?;
            let Some(job) = jobs.first_mut() else { return Err(JobsError::NotFound) };
            if job.status != JobStatus::Draft {
                return Err(JobsError::WrongState(format!("Job is not in draft status (current: {})", job.status)));
            }
            let before = job.status;
            let answer = match fetched {
                Ok(doc) => {
                    let lines = captions::normalize(&doc.cues);
                    match self.insert_lines(uow.as_mut(), job, &lines).await? {
                        Ok(()) => {
                            job.complete_youtube("transcriptapi", now)?;
                            ResolveAnswer::Completed { job_id: id }
                        }
                        Err(_) => {
                            let message = "Failed to save transcript lines";
                            job.fail_youtube("unknown_error", message, now)?;
                            ResolveAnswer::Failed { job_id: id, code: "unknown_error".into(), message: message.into() }
                        }
                    }
                }
                Err(e) => {
                    let (code, message) = sources::caption_failure(&e, &video_id);
                    job.fail_youtube(code, &message, now)?;
                    ResolveAnswer::Failed { job_id: id, code: code.into(), message }
                }
            };
            Self::save(uow.as_mut(), job, before, now).await?;
            Ok::<_, JobsError>(answer)
        }
        .await;
        match result {
            Ok(a) => {
                uow.commit().await?;
                Ok(a)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e)
            }
        }
    }

    // ---- settings --------------------------------------------------------------

    pub async fn settings(&self, account: AccountId) -> Result<UserSettings, JobsError> {
        self.guard(account).await?;
        Ok(self.s.store.settings(account).await?)
    }

    /// PRV-027 (C6): only the auto-delete preference is user-writable; the same
    /// settings-row lock the retention sweep takes.
    pub async fn set_auto_delete(&self, account: AccountId, value: bool) -> Result<UserSettings, JobsError> {
        self.update_settings(account, |s| s.set_auto_delete(value)).await
    }

    /// PRV-036: the legacy toggle, stored and read by nothing.
    pub async fn set_deleted_monthly(&self, account: AccountId, value: bool) -> Result<UserSettings, JobsError> {
        self.update_settings(account, |s| s.set_deleted_monthly(value)).await
    }

    async fn update_settings(&self, account: AccountId, f: impl FnOnce(&mut UserSettings) -> bool) -> Result<UserSettings, JobsError> {
        self.guard(account).await?;
        let mut uow = self.s.store.begin().await?;
        let result = async {
            // Preference writes take the same account lock as the retention sweep.
            match uow.billing().await?.lock_account(account).await {
                Ok(()) | Err(StoreError::AccountMissing(_)) => {}
                Err(e) => return Err(e),
            }
            let mut s = uow.lock_settings(account).await?;
            if f(&mut s) {
                uow.save_settings(&s).await?;
            }
            Ok::<_, StoreError>(s)
        }
        .await;
        match result {
            Ok(s) => {
                uow.commit().await?;
                Ok(s)
            }
            Err(e) => {
                Self::rollback(uow).await;
                Err(e.into())
            }
        }
    }
}

enum Claim {
    Claimed(Box<Job>, Box<Attempt>),
    NotSent(Value),
    NoCapacity,
    Busy,
    NotEligible,
}

enum AttemptResult {
    Accepted(Box<Job>),
    RateLimited,
    Waiting,
    Uncertain,
    Failed(Value),
    Busy,
}

fn charge_error(e: ChargeError) -> JobsError {
    match e {
        // BIL-041: every refusal is the insufficient-balance answer.
        ChargeError::Refused(_) => JobsError::InsufficientBalance("Insufficient balance to start transcription".into()),
        ChargeError::Store(e) => JobsError::Store(e),
    }
}

/// PRV-032/033: `None` (no window) with `unlimited_history`, else one calendar month back.
pub fn history_since(settings: &UserSettings, now: Micros, calendar: CalendarContext) -> Option<Micros> {
    if settings.unlimited_history {
        return None;
    }
    Some(crate::time::minus_one_calendar_month_local(now, calendar.offset_now_minutes, calendar.offset_then_minutes))
}

/// Player URLs and the payload archive (owned separately; reaches `Jobs` as a child).
mod resources;

#[cfg(test)]
mod tests;
