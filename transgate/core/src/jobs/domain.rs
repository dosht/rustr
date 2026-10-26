//! The Job aggregate: the twelve stored statuses, the explicit transitions and
//! the provider attempt that fences every external call.
//!
//! Every write goes through a method here, so no caller can set an arbitrary
//! column the way the source's owner RLS allowed (JOB-045 correction). Lifecycle
//! saves are revision-checked by the store; line edits are not (D7).

use std::fmt;

use media_ports::{ObjectRef, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ids::{AccountId, AttemptId, JobId, MICROS_PER_MINUTE, Micros};
use crate::languages::Language;

/// JOB-001: all twelve values, including the deprecated ones that still occur
/// in production rows. Nothing is folded into another value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Draft,
    NotStarted,
    Pending,
    Running,
    Completed,
    Failed,
    Canceled,
    Deleted,
    SpeakerLabeling,
    Uploaded,
    Processing,
    Ready,
}

impl JobStatus {
    /// Insert order of the source enum.
    pub const ALL: [JobStatus; 12] = [
        JobStatus::Draft,
        JobStatus::NotStarted,
        JobStatus::Pending,
        JobStatus::Running,
        JobStatus::Completed,
        JobStatus::Failed,
        JobStatus::Canceled,
        JobStatus::Deleted,
        JobStatus::SpeakerLabeling,
        JobStatus::Uploaded,
        JobStatus::Processing,
        JobStatus::Ready,
    ];

    /// Statuses the job list shows (JOB-036, PRV-012).
    pub const LISTED: [JobStatus; 6] = [
        JobStatus::Running,
        JobStatus::NotStarted,
        JobStatus::Pending,
        JobStatus::SpeakerLabeling,
        JobStatus::Completed,
        JobStatus::Failed,
    ];

    /// The "in progress" filter (jobs-transcription 2.1).
    pub const IN_PROGRESS: [JobStatus; 4] =
        [JobStatus::Running, JobStatus::NotStarted, JobStatus::Pending, JobStatus::SpeakerLabeling];

    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Draft => "draft",
            JobStatus::NotStarted => "not_started",
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Canceled => "canceled",
            JobStatus::Deleted => "deleted",
            JobStatus::SpeakerLabeling => "speaker_labeling",
            JobStatus::Uploaded => "uploaded",
            JobStatus::Processing => "processing",
            JobStatus::Ready => "ready",
        }
    }

    pub fn parse(text: &str) -> Option<JobStatus> {
        JobStatus::ALL.into_iter().find(|s| s.as_str() == text)
    }

    pub fn is_listed(self) -> bool {
        JobStatus::LISTED.contains(&self)
    }

    pub fn is_in_progress(self) -> bool {
        JobStatus::IN_PROGRESS.contains(&self)
    }

    /// A provider result may only be applied while the job waits for one
    /// (T11/T12: running or pending).
    pub fn accepts_results(self) -> bool {
        matches!(self, JobStatus::Running | JobStatus::Pending)
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The source `transcript_provider` enum. `runpod` is kept as stored data and
/// a routing slot; it has no transport (PRD section 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Runpod,
    Deepgram,
    Soniox,
    SonioxV5,
}

impl Provider {
    pub const ALL: [Provider; 4] = [Provider::Runpod, Provider::Deepgram, Provider::Soniox, Provider::SonioxV5];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Runpod => "runpod",
            Provider::Deepgram => "deepgram",
            Provider::Soniox => "soniox",
            Provider::SonioxV5 => "soniox_v5",
        }
    }

    pub fn parse(text: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.as_str() == text)
    }

    /// The shared transport that executes this product provider, if any.
    pub fn transport(self) -> Option<media_ports::Provider> {
        match self {
            Provider::Soniox | Provider::SonioxV5 => Some(media_ports::Provider::Soniox),
            Provider::Deepgram => Some(media_ports::Provider::Deepgram),
            Provider::Runpod => None,
        }
    }

    /// Capability table `supportsTranslation` (LNG-014).
    pub fn supports_translation(self) -> bool {
        matches!(self, Provider::Soniox | Provider::SonioxV5)
    }

    /// `canHandle` (TRX-011..013, LNG-010/015/016). Soniox takes auto-detect and
    /// its 53 languages; Deepgram refuses auto-detect; RunPod's table took any.
    pub fn can_handle(self, language: Option<Language>) -> bool {
        match self {
            Provider::Soniox | Provider::SonioxV5 => language.is_none_or(|l| l.soniox()),
            Provider::Deepgram => language.is_some_and(|l| l.deepgram()),
            Provider::Runpod => true,
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The source `storage_provider_type` values recorded on the job (UPL-012).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageProvider {
    Gcs,
    Cloudflare,
    ExternalUrl,
}

impl StorageProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            StorageProvider::Gcs => "gcs",
            StorageProvider::Cloudflare => "cloudflare",
            StorageProvider::ExternalUrl => "external_url",
        }
    }

    pub fn parse(text: &str) -> Option<StorageProvider> {
        match text {
            "gcs" => Some(StorageProvider::Gcs),
            "cloudflare" => Some(StorageProvider::Cloudflare),
            "external_url" => Some(StorageProvider::ExternalUrl),
            _ => None,
        }
    }

    pub fn store_kind(self) -> Option<media_ports::StoreKind> {
        match self {
            StorageProvider::Gcs => Some(media_ports::StoreKind::Gcs),
            StorageProvider::Cloudflare => Some(media_ports::StoreKind::R2),
            StorageProvider::ExternalUrl => None,
        }
    }
}

/// Where a job's media comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobSource {
    /// Browser upload to the configured bucket. `object` is set when the upload
    /// URL is issued; imported rows may have none.
    Upload { storage: StorageProvider, object: Option<ObjectRef> },
    /// API audio URL (UPL-031). `file_path` holds the URL; JOB-043 falls back to
    /// the legacy `callback_url` when `file_path` is empty. `staged` is the
    /// private copy the controlled fetch produced, never a signed URL.
    ExternalUrl { file_path: Option<String>, staged: Option<ObjectRef> },
    /// Signed-in YouTube captions (UPL-024).
    YouTube { video_id: Option<String>, youtube_url: Option<String>, transcript_source: Option<String> },
}

impl JobSource {
    pub fn storage_provider(&self) -> StorageProvider {
        match self {
            JobSource::Upload { storage, .. } => *storage,
            JobSource::ExternalUrl { .. } | JobSource::YouTube { .. } => StorageProvider::ExternalUrl,
        }
    }

    pub fn is_youtube(&self) -> bool {
        matches!(self, JobSource::YouTube { .. })
    }
}

/// Deferred billing state for jobs started without a duration (BIL-028/034..036).
/// The store persists it with the job, so a repeated callback can never charge twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferredCharge {
    /// Duration known at start (or never billed, like YouTube).
    None,
    /// Started with a NULL duration; the first applied result decides.
    Pending,
    Charged,
    /// The strict deduction refused or failed; the transcript is still delivered (BIL-036).
    Refused,
    /// The provider reported no usable length: no update and no charge (BIL-035).
    NoDuration,
}

/// A job row. Fields mirror the source columns plus the attempt fence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub account: AccountId,
    /// Optimistic concurrency token for lifecycle saves.
    pub revision: i64,
    pub status: JobStatus,
    /// `job_name`: the original file name, the video title or "API Transcription".
    pub name: Option<String>,
    pub source: JobSource,
    /// `None` is auto-detect (UPL-017).
    pub language: Option<Language>,
    /// Only the first entry is ever sent (TRL-002).
    pub translate_to: Vec<Language>,
    pub speakers: bool,
    pub fix_grammer: Option<bool>,
    /// Billed whole minutes (`smallint`); `None` until known.
    pub duration_minutes: Option<i32>,
    pub size_bytes: Option<i64>,
    /// `transcription_provider`: the stored pin, then the provider that accepted.
    pub provider: Option<Provider>,
    /// The model requested by the accepted attempt (D11 keeps it readable).
    pub model: Option<String>,
    /// API customer's result webhook (P7 delivers it).
    pub callback_url: Option<String>,
    /// API Whisper options kept opaque (TRX-022); no remaining provider acts on them.
    pub whisper_params: Option<Value>,
    /// Free-shape source error; imported RunPod rows may hold an array.
    pub error: Option<Value>,
    pub retry_count: i32,
    pub recovery_attempts: i32,
    pub soniox_transcription_id: Option<String>,
    pub started_at: Option<Micros>,
    pub created_at: Micros,
    pub updated_at: Micros,
    /// The live or last attempt; superseded attempts stay in the attempt store.
    pub current_attempt: Option<AttemptId>,
    pub attempts_made: i32,
    pub deferred_charge: DeferredCharge,
    /// When the dispatch pass may next submit this job.
    pub next_dispatch_at: Option<Micros>,
}

impl Job {
    pub fn is_translation(&self) -> bool {
        !self.translate_to.is_empty()
    }

    pub fn translation_target(&self) -> Option<Language> {
        self.translate_to.first().copied()
    }

    /// Lower case extension of `job_name` (the `qta` rule, TRX-003).
    pub fn file_extension(&self) -> Option<String> {
        let name = self.name.as_deref()?;
        let (_, ext) = name.rsplit_once('.')?;
        Some(ext.to_lowercase())
    }

    /// Billing's view of this job at start (BIL-025..028).
    pub fn billing_start(&self, other_completed_jobs: u64) -> billing_core::charge::JobStart {
        billing_core::charge::JobStart {
            subject: self.id.subject(),
            duration: self.duration_minutes.map(i64::from),
            other_completed_jobs,
        }
    }
}

/// What makes a draft.
#[derive(Clone, Debug, PartialEq)]
pub struct NewJob {
    pub account: AccountId,
    pub status: JobStatus,
    pub name: Option<String>,
    pub source: JobSource,
    pub language: Option<Language>,
    pub translate_to: Vec<Language>,
    pub speakers: bool,
    pub fix_grammer: Option<bool>,
    pub duration_minutes: Option<i32>,
    pub size_bytes: Option<i64>,
    pub callback_url: Option<String>,
    pub whisper_params: Option<Value>,
    pub created_at: Micros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobError {
    WrongStatus { job: JobId, expected: &'static str, actual: JobStatus },
    /// The attempt is not the job's live attempt: a stale acknowledgement or callback.
    StaleAttempt { job: JobId, attempt: AttemptId },
    /// A live attempt already holds the job.
    AttemptInFlight { job: JobId, attempt: AttemptId },
    /// The attempt already holds another provider identity (one POST, one remote).
    RemoteMismatch { attempt: AttemptId },
    Invalid(String),
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::WrongStatus { job, expected, actual } => {
                write!(f, "Job {job} is in unexpected state: {actual} (expected {expected})")
            }
            JobError::StaleAttempt { job, attempt } => write!(f, "attempt {attempt} is not the live attempt of job {job}"),
            JobError::AttemptInFlight { job, attempt } => write!(f, "job {job} already has live attempt {attempt}"),
            JobError::RemoteMismatch { attempt } => write!(f, "attempt {attempt} already holds a different remote identity"),
            JobError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for JobError {}

/// JOB-020: a running job is stuck 30 minutes after `started_at`.
pub const STUCK_AFTER_MINUTES: i64 = 30;

pub fn is_stuck(job: &Job, now: Micros) -> bool {
    job.status == JobStatus::Running
        && job.started_at.is_some_and(|s| s.0 < now.0 - STUCK_AFTER_MINUTES * MICROS_PER_MINUTE)
}

fn iso(at: Micros) -> String {
    crate::time::iso8601(at)
}

impl Job {
    fn touch(&mut self, now: Micros) {
        self.updated_at = now;
    }

    fn require(&self, expected: &'static str, ok: bool) -> Result<(), JobError> {
        if ok {
            Ok(())
        } else {
            Err(JobError::WrongStatus { job: self.id, expected, actual: self.status })
        }
    }

    /// JOB-005, UPL-019/020: options live on the draft and change only there.
    pub fn set_draft_options(
        &mut self,
        language: Option<Option<Language>>,
        translate_to: Option<Vec<Language>>,
        speakers: Option<bool>,
        now: Micros,
    ) -> Result<(), JobError> {
        self.require("draft", self.status == JobStatus::Draft)?;
        if let Some(language) = language {
            self.language = language;
        }
        if let Some(targets) = translate_to {
            self.translate_to = targets;
        }
        if let Some(speakers) = speakers {
            self.speakers = speakers;
        }
        self.touch(now);
        Ok(())
    }

    /// The browser-measured duration of a draft (UPL-001/006). The source let
    /// the owner write it until start; after start it is server-owned.
    pub fn set_draft_duration(&mut self, minutes: Option<i32>, now: Micros) -> Result<(), JobError> {
        self.require("draft", self.status == JobStatus::Draft)?;
        self.duration_minutes = minutes;
        self.touch(now);
        Ok(())
    }

    /// Records the object the upload URL points at (UPL-010/012).
    pub fn bind_upload(&mut self, storage: StorageProvider, object: ObjectRef, now: Micros) -> Result<(), JobError> {
        self.require("draft", self.status == JobStatus::Draft)?;
        self.source = JobSource::Upload { storage, object: Some(object) };
        self.touch(now);
        Ok(())
    }

    /// T04 (JOB-004): draft to not_started, the only billable move. The caller
    /// charges in the same transaction; a NULL duration defers the charge.
    pub fn start(&mut self, now: Micros) -> Result<(), JobError> {
        self.require("draft", self.status == JobStatus::Draft)?;
        self.status = JobStatus::NotStarted;
        self.deferred_charge =
            if self.duration_minutes.is_none() { DeferredCharge::Pending } else { DeferredCharge::None };
        self.next_dispatch_at = Some(now);
        self.touch(now);
        Ok(())
    }

    /// Persist an attempt claim before any provider call (JOB-D02 correction).
    pub fn claim(&mut self, attempt: &Attempt, live: Option<&Attempt>, now: Micros) -> Result<(), JobError> {
        self.require("not_started or pending", matches!(self.status, JobStatus::NotStarted | JobStatus::Pending))?;
        if let Some(live) = live.filter(|a| a.is_live(now)) {
            return Err(JobError::AttemptInFlight { job: self.id, attempt: live.id });
        }
        self.current_attempt = Some(attempt.id);
        // The source writes the selected provider before submitting
        // (`updateJobProvider`), so a rate-limited job retries the same provider.
        self.provider = Some(attempt.provider);
        self.attempts_made += 1;
        self.touch(now);
        Ok(())
    }

    fn require_current(&self, attempt: AttemptId) -> Result<(), JobError> {
        if self.current_attempt == Some(attempt) {
            Ok(())
        } else {
            Err(JobError::StaleAttempt { job: self.id, attempt })
        }
    }

    /// T05 (JOB-014): accepted by the provider; the accepting provider becomes
    /// the job's provider and Soniox keeps its transcription id.
    pub fn accept(&mut self, attempt: &Attempt, remote_id: &str, now: Micros) -> Result<(), JobError> {
        self.require_current(attempt.id)?;
        self.require("not_started or pending", matches!(self.status, JobStatus::NotStarted | JobStatus::Pending))?;
        self.status = JobStatus::Running;
        self.started_at = Some(now);
        self.provider = Some(attempt.provider);
        self.model = Some(attempt.model.clone());
        if matches!(attempt.provider, Provider::Soniox | Provider::SonioxV5) {
            self.soniox_transcription_id = Some(remote_id.to_string());
        }
        self.next_dispatch_at = None;
        self.touch(now);
        Ok(())
    }

    /// T08 (JOB-015): a rate limit moves the job to pending and counts a retry.
    pub fn rate_limited(&mut self, attempt: &Attempt, details: Option<&str>, http_status: Option<u16>, retry_at: Micros, now: Micros) -> Result<(), JobError> {
        self.require_current(attempt.id)?;
        self.require(
            "pending, not_started or running",
            matches!(self.status, JobStatus::Pending | JobStatus::NotStarted | JobStatus::Running),
        )?;
        self.status = JobStatus::Pending;
        self.retry_count += 1;
        let mut error = json!({ "message": "Rate limit exceeded, will retry", "last_attempt": iso(now) });
        if let Some(details) = details {
            error["details"] = json!(details);
        }
        if let Some(status) = http_status {
            error["httpStatus"] = json!(status);
        }
        self.error = Some(error);
        self.next_dispatch_at = Some(retry_at);
        self.touch(now);
        Ok(())
    }

    /// Capacity or configuration kept every compatible provider from taking the
    /// job: it waits as pending for the next dispatch pass, with no failure.
    pub fn wait_for_capacity(&mut self, retry_at: Micros, now: Micros) -> Result<(), JobError> {
        self.require("not_started or pending", matches!(self.status, JobStatus::NotStarted | JobStatus::Pending))?;
        self.next_dispatch_at = Some(retry_at);
        self.touch(now);
        Ok(())
    }

    /// T16 (JOB-016): a definite submit failure once failover is exhausted.
    pub fn fail_submission(&mut self, failure: Value, now: Micros) -> Result<(), JobError> {
        self.require("not_started or pending", matches!(self.status, JobStatus::NotStarted | JobStatus::Pending))?;
        self.status = JobStatus::Failed;
        self.error = Some(failure);
        self.next_dispatch_at = None;
        self.touch(now);
        Ok(())
    }

    /// T11/T12: the live attempt's result is applied in one transaction.
    pub fn complete(&mut self, attempt: AttemptId, now: Micros) -> Result<(), JobError> {
        self.require_current(attempt)?;
        self.require("running or pending", self.status.accepts_results())?;
        self.status = JobStatus::Completed;
        self.touch(now);
        Ok(())
    }

    /// T15/T17/T18 and the no-speech rule (JOB-031, TRX-040): a coded failure
    /// of the live attempt while the job waits for a result.
    pub fn fail_result(&mut self, attempt: AttemptId, failure: Value, now: Micros) -> Result<(), JobError> {
        self.require_current(attempt)?;
        self.require("running or pending", self.status.accepts_results())?;
        self.status = JobStatus::Failed;
        self.error = Some(failure);
        self.touch(now);
        Ok(())
    }

    /// T09 (JOB-021): a stuck Deepgram job goes back to pending; no maximum.
    pub fn reset_stuck_deepgram(&mut self, now: Micros) -> Result<(), JobError> {
        self.require("stuck running", is_stuck(self, now))?;
        self.status = JobStatus::Pending;
        self.retry_count += 1;
        self.error = Some(json!({
            "message": "Job was stuck in running state, reset for retry",
            "stuck_recovery": true,
            "stuck_at": iso(now),
        }));
        self.next_dispatch_at = Some(now);
        self.touch(now);
        Ok(())
    }

    /// JOB-024: the recovery claim is a compare-and-swap of `recovery_attempts`.
    pub fn claim_recovery(&mut self, expected: i32, now: Micros) -> Result<(), JobError> {
        self.require("running", self.status == JobStatus::Running)?;
        if self.recovery_attempts != expected {
            return Err(JobError::Invalid(format!(
                "recovery claim lost: expected {expected}, found {}",
                self.recovery_attempts
            )));
        }
        self.recovery_attempts += 1;
        self.touch(now);
        Ok(())
    }

    /// JOB-024: still processing, or the status call failed: the claim is given back.
    pub fn release_recovery(&mut self, now: Micros) {
        self.recovery_attempts = (self.recovery_attempts - 1).max(0);
        self.touch(now);
    }

    /// JOB-023: exhausted or unrecoverable recovery fails the running job.
    pub fn fail_recovery(&mut self, code: &str, message: &str, now: Micros) -> Result<(), JobError> {
        self.require("running", self.status == JobStatus::Running)?;
        self.status = JobStatus::Failed;
        self.error = Some(json!({ "message": message, "code": code, "provider": "soniox" }));
        self.touch(now);
        Ok(())
    }

    /// T22 (JOB-040): a core operation with no route or screen, as in the source.
    pub fn cancel(&mut self, now: Micros) -> Result<(), JobError> {
        self.require(
            "not_started, pending or running",
            matches!(self.status, JobStatus::NotStarted | JobStatus::Pending | JobStatus::Running),
        )?;
        self.status = JobStatus::Canceled;
        self.error = Some(json!({ "message": "Job canceled by user" }));
        self.next_dispatch_at = None;
        self.touch(now);
        Ok(())
    }

    /// T23 (JOB-036): soft delete from any status. Returns false when already deleted.
    pub fn soft_delete(&mut self, now: Micros) -> bool {
        if self.status == JobStatus::Deleted {
            return false;
        }
        self.status = JobStatus::Deleted;
        self.next_dispatch_at = None;
        self.touch(now);
        true
    }

    /// T13 (JOB-035): YouTube captions resolved; lines are inserted in the same transaction.
    pub fn complete_youtube(&mut self, transcript_source: &str, now: Micros) -> Result<(), JobError> {
        self.require("draft", self.status == JobStatus::Draft && self.source.is_youtube())?;
        if let JobSource::YouTube { transcript_source: ts, .. } = &mut self.source {
            *ts = Some(transcript_source.to_string());
        }
        self.status = JobStatus::Completed;
        self.touch(now);
        Ok(())
    }

    /// T20: YouTube resolve error; the client then deletes the draft (UPL-027/028).
    pub fn fail_youtube(&mut self, code: &str, message: &str, now: Micros) -> Result<(), JobError> {
        self.require("draft", self.status == JobStatus::Draft && self.source.is_youtube())?;
        self.status = JobStatus::Failed;
        self.error = Some(json!({ "code": code, "message": message }));
        self.touch(now);
        Ok(())
    }

    /// BIL-034..037 decisions applied to the job; billing itself happens in the store transaction.
    pub fn set_duration(&mut self, minutes: i32, now: Micros) {
        self.duration_minutes = Some(minutes);
        self.touch(now);
    }
}

/// Attempt lifecycle. Every state stays readable after the attempt is superseded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    /// Committed before the provider call; the lease bounds how long it may stay unanswered.
    Claimed,
    Accepted,
    /// The call may have reached the provider; nonacceptance is not proved.
    Uncertain,
    RateLimited,
    Rejected,
    /// No transport or configuration for this provider (removed RunPod, missing key).
    Unavailable,
    /// Its result was applied (completed or coded failure).
    Finished,
    /// A later attempt replaced it; its remote identity is retained for erasure.
    Superseded,
}

/// The request facts persisted with a claim, before HTTP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttemptRequest {
    pub language_code: Option<String>,
    pub translation_target: Option<String>,
    pub diarize: bool,
    /// The media the signed access is issued for; never the signed URL itself.
    pub audio: ObjectRef,
    /// Source `client_reference_id` `job_{id}` (TRX-015).
    pub client_reference: String,
}

/// Remote cleanup of a provider-side copy (TRX-041, PRV-037, erasure).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RemoteCleanup {
    NotRequested,
    Pending,
    Deleted { at: Micros },
    Absent { at: Micros },
    /// Soniox refuses to delete while processing; retried later.
    Retained { reason: String },
    /// No provider delete API (Deepgram); retention is the provider's (D3).
    Unsupported { reason: String },
    Failed { attempts: i32, last_error: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: AttemptId,
    pub job: JobId,
    pub account: AccountId,
    /// 1-based, monotonic per job.
    pub number: i32,
    pub provider: Provider,
    /// The configured provider project/account the transport used.
    pub project: String,
    pub model: String,
    pub state: AttemptState,
    pub claimed_at: Micros,
    pub lease_until: Micros,
    pub request: AttemptRequest,
    /// Per-attempt callback credential (C-6). Redacted in debug output.
    pub callback_secret: SecretString,
    pub remote_id: Option<String>,
    pub accepted_at: Option<Micros>,
    pub last_error: Option<Value>,
    pub remote_cleanup: RemoteCleanup,
}

/// How long a claim may stay unanswered before recovery may reconcile it.
pub const CLAIM_LEASE_MINUTES: i64 = 2;

impl Attempt {
    /// A claimed attempt within its lease, or an accepted or uncertain one, holds the job.
    pub fn is_live(&self, now: Micros) -> bool {
        match self.state {
            AttemptState::Claimed => now.0 < self.lease_until.0,
            AttemptState::Accepted | AttemptState::Uncertain => true,
            _ => false,
        }
    }

    /// The binding the host passes to `Transcriber::verify_callback` for a
    /// callback that names this attempt. Before acceptance commits the remote id
    /// is unknown, and core decides adoption (W3).
    pub fn binding(&self) -> Option<media_ports::CallbackBinding> {
        Some(media_ports::CallbackBinding {
            attempt: self.id.0,
            provider: self.provider.transport()?,
            project: self.project.clone(),
            expected_remote_id: self.remote_id.clone(),
            secret: self.callback_secret.clone(),
        })
    }

    pub fn remote(&self) -> Option<media_ports::RemoteIdentity> {
        Some(media_ports::RemoteIdentity {
            provider: self.provider.transport()?,
            project: self.project.clone(),
            id: self.remote_id.clone()?,
        })
    }

    pub fn accept(&mut self, remote_id: &str, now: Micros) -> Result<(), JobError> {
        match self.state {
            AttemptState::Claimed | AttemptState::Uncertain => {
                self.state = AttemptState::Accepted;
                self.remote_id = Some(remote_id.to_string());
                self.accepted_at = Some(now);
                Ok(())
            }
            AttemptState::Accepted if self.remote_id.as_deref() == Some(remote_id) => Ok(()),
            _ => Err(JobError::StaleAttempt { job: self.job, attempt: self.id }),
        }
    }

    /// F3: a provider identity learned after the attempt stopped being the live one
    /// (a late acknowledgement or callback). The attempt keeps it so cleanup and
    /// erasure can find the remote work; nothing is revived. `true` when it is new.
    pub fn learn_remote(&mut self, remote_id: &str) -> Result<bool, JobError> {
        match self.remote_id.as_deref() {
            None => {
                self.remote_id = Some(remote_id.to_string());
                Ok(true)
            }
            Some(bound) if bound == remote_id => Ok(false),
            Some(_) => Err(JobError::RemoteMismatch { attempt: self.id }),
        }
    }

    pub fn settle(&mut self, state: AttemptState, error: Option<Value>) {
        self.state = state;
        if error.is_some() {
            self.last_error = error;
        }
    }
}

/// The client reference every submission of a job carries (TRX-015).
pub fn client_reference(job: JobId) -> String {
    format!("job_{}", job.0)
}

#[cfg(test)]
mod tests;
