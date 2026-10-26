//! What applying a provider observation does, decided purely (JOB-027..031,
//! TRX-024..034, TRX-040, BIL-034..037).
//!
//! Source order is kept: a Soniox `error` status fails the job before any
//! duration work; otherwise the duration rule (including the deferred charge)
//! runs before segmentation, so a no-speech result is still charged when its
//! duration was unknown, exactly as the source's separate statements did.

use media_ports::{ProviderObservation, ProviderPayload};
use serde_json::{Value, json};

use crate::jobs::domain::{Job, JobStatus};
use crate::jobs::duration::{self, DurationDecision};
use crate::transcript::{NewLine, deepgram, soniox};

#[derive(Clone, Debug, PartialEq)]
pub enum IngestPlan {
    /// JOB-027: a completed job ignores further results. A deleted, canceled or
    /// failed job writes no lines either (JOB-D01 correction C1).
    Ignore { reason: String },
    /// The provider reported failure: coded failure, no duration step.
    ProviderFailed { failure: Value },
    /// Duration rule, then the lines and completion. `archive_name` is the raw
    /// payload file kept best effort (JOB-044).
    Complete { duration: DurationDecision, lines: Vec<NewLine>, archive_name: &'static str },
    /// Duration rule, then a coded failure (no speech: JOB-031, TRX-040).
    NoSegments { duration: DurationDecision, failure: Value, archive_name: &'static str },
}

pub const SONIOX_ARCHIVE: &str = "soniox_transcript.json";
pub const DEEPGRAM_ARCHIVE: &str = "deepgram_transcript.json";

/// `{account}/{job}/{write}/{file}` (JOB-044, decided difference): one immutable
/// object per archive write, so finding the object proves that one PUT (F2). The
/// source wrote `jobs/{user_id}/{job_id}/{file}` with upsert.
pub fn archive_key(job: &Job, write: uuid::Uuid, file: &str) -> String {
    format!("{}/{}/{}/{}", job.account, job.id.0, write, file)
}

pub fn plan(job: &Job, observation: &ProviderObservation) -> IngestPlan {
    if job.status == JobStatus::Completed {
        return IngestPlan::Ignore { reason: "job already completed".into() };
    }
    if !job.status.accepts_results() {
        return IngestPlan::Ignore { reason: format!("late result for a {} job", job.status) };
    }
    match &observation.payload {
        ProviderPayload::Soniox { reported, transcript } => {
            if reported.status == "failed" || reported.status == "error" {
                let message = reported
                    .error
                    .as_ref()
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown error");
                let code = reported.error.as_ref().and_then(|e| e.get("code")).cloned().unwrap_or(Value::Null);
                return IngestPlan::ProviderFailed {
                    failure: json!({
                        "message": message,
                        "code": code,
                        "provider": "soniox",
                        "details": {
                            "status": reported.status,
                            "fullError": reported.error,
                            "transcriptionId": reported.id,
                        },
                    }),
                };
            }
            let decision = duration::decide(job, duration::provider_minutes(observation));
            let tokens = transcript.as_ref().map(|t| t.tokens.as_slice()).unwrap_or(&[]);
            let lines = soniox::normalize(tokens, job.speakers);
            if lines.is_empty() {
                return IngestPlan::NoSegments {
                    duration: decision,
                    failure: json!({
                        "message": "No transcription segments found",
                        "code": "no_speech_detected",
                        "provider": "soniox",
                        "details": {
                            "tokensCount": tokens.len(),
                            "transcriptionId": reported.id,
                            "hasMetadata": transcript.as_ref().is_some_and(|t| !t.metadata.is_null()),
                        },
                    }),
                    archive_name: SONIOX_ARCHIVE,
                };
            }
            IngestPlan::Complete { duration: decision, lines, archive_name: SONIOX_ARCHIVE }
        }
        ProviderPayload::Deepgram(result) => {
            let decision = duration::decide(job, duration::provider_minutes(observation));
            let lines = deepgram::normalize(result, job.speakers);
            if lines.is_empty() {
                return IngestPlan::NoSegments {
                    duration: decision,
                    failure: json!({ "message": "No transcription segments found" }),
                    archive_name: DEEPGRAM_ARCHIVE,
                };
            }
            IngestPlan::Complete { duration: decision, lines, archive_name: DEEPGRAM_ARCHIVE }
        }
    }
}

/// TRX-032: a deterministic line insert failure fails the job with `database_error`.
pub fn insert_failure(job: &Job, observation: &ProviderObservation, message: &str, lines: usize) -> Value {
    match observation.payload {
        ProviderPayload::Soniox { .. } => json!({
            "message": "Failed to insert transcripts",
            "code": "database_error",
            "provider": "soniox",
            "details": { "errorMessage": message, "segmentsCount": lines, "jobId": job.id.0 },
        }),
        ProviderPayload::Deepgram(_) => json!({ "message": "Failed to insert transcripts", "details": message }),
    }
}

/// JOB-029: a Soniox ingestion that throws after the acknowledgement fails a
/// running or pending job with `callback_ingestion_error`.
pub fn ingestion_error(message: &str) -> Value {
    json!({ "message": message, "code": "callback_ingestion_error", "provider": "soniox" })
}

/// A stable digest of an observation, to tell an identical redelivery from a
/// conflicting payload under the same attempt and remote id (C-6). FNV-1a over
/// the serialized payload: authentication and identity binding already happened,
/// so this only compares deliveries, it does not protect anything.
pub fn fingerprint(observation: &ProviderObservation) -> String {
    let bytes = serde_json::to_vec(&observation.payload).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests;
