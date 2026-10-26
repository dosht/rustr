//! One retry decision for every provider (PRD section 3): submission outcome
//! classification, the dispatch queue rules and stuck-job recovery.
//!
//! Adapters never retry or change job status on their own; they report a
//! `Submission` or a `TransportError` and this module decides.

use media_ports::{Submission, TransportError};
use serde_json::{Value, json};

use crate::ids::{MICROS_PER_MINUTE, Micros};
use crate::jobs::domain::{Job, Provider};

/// Source `ErrorType` values stored in `error.code` (JOB-017, JOB-046).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorType {
    RateLimited,
    InvalidApiKey,
    InvalidRequest,
    ProviderError,
    ProviderUnavailable,
    Timeout,
    NetworkError,
    DatabaseError,
}

impl ErrorType {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorType::RateLimited => "rate_limited",
            ErrorType::InvalidApiKey => "invalid_api_key",
            ErrorType::InvalidRequest => "invalid_request",
            ErrorType::ProviderError => "provider_error",
            ErrorType::ProviderUnavailable => "provider_unavailable",
            ErrorType::Timeout => "timeout",
            ErrorType::NetworkError => "network_error",
            ErrorType::DatabaseError => "database_error",
        }
    }

    /// JOB-017: rate limited, timeout, network and provider errors are retryable.
    pub fn retryable(self) -> bool {
        matches!(self, ErrorType::RateLimited | ErrorType::Timeout | ErrorType::NetworkError | ErrorType::ProviderError)
    }
}

/// What a submission outcome means for the job.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmitOutcome {
    Accepted { remote_id: String },
    /// JOB-015: pending, retry count + 1, the source's rate-limit error.
    RateLimited { details: Option<String>, http_status: Option<u16> },
    /// A definite rejection; the attempt failed and failover may follow (DEF-06).
    Rejected { failure: Value },
    /// The request may have reached the provider; nonacceptance is not proved.
    Uncertain { reason: String },
    /// Nothing was sent: configuration missing, unsafe destination, no transport.
    NotSent { failure: Value },
}

fn soniox_class(status: Option<u16>, code: &str) -> ErrorType {
    let lower = code.to_lowercase();
    match status {
        Some(429) => ErrorType::RateLimited,
        _ if lower.contains("rate limit") => ErrorType::RateLimited,
        Some(401) | Some(403) => ErrorType::InvalidApiKey,
        Some(s) if (400..500).contains(&s) => ErrorType::InvalidRequest,
        Some(s) if s >= 500 => ErrorType::ProviderError,
        _ if lower.contains("timeout") => ErrorType::Timeout,
        _ => ErrorType::ProviderError,
    }
}

/// Deepgram: HTTP 429, `RATE_LIMIT_EXCEEDED` or a message mentioning "rate limit" (JOB-015).
fn deepgram_rate_limited(status: Option<u16>, code: &str) -> bool {
    status == Some(429) || code == "RATE_LIMIT_EXCEEDED" || code.contains("rate limit")
}

/// Classifies a transport answer for the attempt's product provider.
pub fn classify(provider: Provider, result: &Result<Submission, TransportError>) -> SubmitOutcome {
    match result {
        Ok(Submission::Accepted { remote }) => SubmitOutcome::Accepted { remote_id: remote.id.clone() },
        Ok(Submission::Rejected { status, code }) => match provider {
            Provider::Deepgram => {
                if deepgram_rate_limited(*status, code) {
                    SubmitOutcome::RateLimited { details: None, http_status: *status }
                } else {
                    SubmitOutcome::Rejected { failure: json!({ "message": code }) }
                }
            }
            _ => {
                let class = soniox_class(*status, code);
                if class == ErrorType::RateLimited {
                    SubmitOutcome::RateLimited { details: Some(code.clone()), http_status: *status }
                } else {
                    SubmitOutcome::Rejected {
                        failure: json!({
                            "message": code,
                            "code": class.as_str(),
                            "provider": "soniox",
                            "httpStatus": status,
                        }),
                    }
                }
            }
        },
        Ok(Submission::Uncertain { status, operation }) => {
            SubmitOutcome::Uncertain { reason: format!("{operation} (status {status:?})") }
        }
        Err(TransportError::Rejected { status, code }) => {
            classify(provider, &Ok(Submission::Rejected { status: *status, code: code.clone() }))
        }
        Err(TransportError::Unavailable { status, operation }) => {
            SubmitOutcome::Uncertain { reason: format!("{operation} unavailable (status {status:?})") }
        }
        Err(e) => SubmitOutcome::NotSent {
            failure: json!({
                "message": e.to_string(),
                "code": ErrorType::ProviderUnavailable.as_str(),
                "provider": provider.as_str(),
            }),
        },
    }
}

/// The failure stored when no provider is left (`handleFailover`'s end).
pub fn exhausted_failure(message: &str, provider: Provider) -> Value {
    json!({ "message": message, "code": ErrorType::ProviderUnavailable.as_str(), "provider": provider.as_str() })
}

/// Dispatch pass cadence and limits (JOB-018/019, CRN-002/003).
pub const PASS_INTERVAL_MINUTES: i64 = 3;
/// At most five Deepgram submissions per pass, and never more than free slots.
pub const DEEPGRAM_PER_PASS: u32 = 5;
/// One second between Deepgram submissions.
pub const DEEPGRAM_SPACING_MS: u64 = 1_000;
/// Candidates a pass examines; a scan bound, not a customer retry limit.
pub const PASS_SCAN_LIMIT: u32 = 200;

/// When a rate-limited job may be tried again: the next pass.
pub fn next_pass(now: Micros) -> Micros {
    Micros(now.0 + PASS_INTERVAL_MINUTES * MICROS_PER_MINUTE)
}

/// JOB-023: the Soniox recovery cap.
pub const MAX_RECOVERY_ATTEMPTS: i32 = 3;

/// What a stuck running job needs (JOB-020..024).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StuckAction {
    /// Deepgram: reset to pending and resubmit later, no maximum (JOB-021).
    ResetDeepgram,
    /// Soniox: claim, then ask Soniox for the stored transcription's status.
    RecoverSoniox { expected_attempts: i32, transcription_id: String },
    FailSoniox { code: &'static str, message: String, details: Value },
    /// Imported RunPod or speaker-labeling work: no runtime recovery; P9 drains it.
    None,
}

pub fn stuck_action(job: &Job) -> StuckAction {
    match job.provider {
        Some(Provider::Deepgram) => StuckAction::ResetDeepgram,
        Some(Provider::Soniox) | Some(Provider::SonioxV5) => {
            if job.recovery_attempts >= MAX_RECOVERY_ATTEMPTS {
                return StuckAction::FailSoniox {
                    code: "recovery_attempts_exhausted",
                    message: format!("Recovery gave up after {} attempts", job.recovery_attempts),
                    details: json!({ "attempts": job.recovery_attempts, "transcriptionId": job.soniox_transcription_id }),
                };
            }
            match &job.soniox_transcription_id {
                None => StuckAction::FailSoniox {
                    code: "recovery_unavailable",
                    message: "Job cannot be recovered: no Soniox transcription id was recorded at submission".into(),
                    details: json!({ "startedAt": job.started_at.map(crate::time::iso8601) }),
                },
                Some(id) => {
                    StuckAction::RecoverSoniox { expected_attempts: job.recovery_attempts, transcription_id: id.clone() }
                }
            }
        }
        _ => StuckAction::None,
    }
}

/// A Soniox status observation during recovery (JOB-022/024).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// `queued` or `processing`: give the claim back and leave the job alone.
    StillWorking,
    /// `completed` or `error`: ingest through the same path as a webhook.
    Terminal,
}

pub fn recovery_status(status: &str) -> RecoveryStatus {
    match status {
        "queued" | "processing" => RecoveryStatus::StillWorking,
        _ => RecoveryStatus::Terminal,
    }
}

/// Estimated completion for the start answer: `duration / 15`, at least one
/// minute, five minutes assumed when unknown (both providers' source estimate).
pub fn estimated_completion(job: &Job, now: Micros) -> Micros {
    let minutes = i64::from(job.duration_minutes.filter(|d| *d != 0).unwrap_or(5));
    let processing = ((minutes as f64) / 15.0).ceil().max(1.0) as i64;
    Micros(now.0 + processing * MICROS_PER_MINUTE)
}

#[cfg(test)]
mod tests {
    use media_ports::RemoteIdentity;

    use super::*;

    // JOB-017
    #[test]
    fn soniox_errors_classify_like_the_source() {
        let rejected = |s, c: &str| classify(Provider::Soniox, &Ok(Submission::Rejected { status: s, code: c.into() }));
        assert!(matches!(rejected(Some(429), "x"), SubmitOutcome::RateLimited { .. }));
        assert!(matches!(rejected(None, "Rate limit reached"), SubmitOutcome::RateLimited { .. }));
        let code = |o: SubmitOutcome| match o {
            SubmitOutcome::Rejected { failure } => failure["code"].as_str().unwrap().to_string(),
            other => panic!("{other:?}"),
        };
        assert_eq!(code(rejected(Some(401), "x")), "invalid_api_key");
        assert_eq!(code(rejected(Some(403), "x")), "invalid_api_key");
        assert_eq!(code(rejected(Some(422), "x")), "invalid_request");
        assert_eq!(code(rejected(Some(503), "x")), "provider_error");
        assert_eq!(code(rejected(None, "request timeout")), "timeout");
    }

    // JOB-015: Deepgram's three rate-limit signals.
    #[test]
    fn deepgram_rate_limits_are_detected() {
        let rejected = |s, c: &str| classify(Provider::Deepgram, &Ok(Submission::Rejected { status: s, code: c.into() }));
        assert!(matches!(rejected(Some(429), "x"), SubmitOutcome::RateLimited { .. }));
        assert!(matches!(rejected(Some(400), "RATE_LIMIT_EXCEEDED"), SubmitOutcome::RateLimited { .. }));
        assert!(matches!(rejected(Some(400), "Bad Request"), SubmitOutcome::Rejected { .. }));
    }

    // Seam 6: a lost answer after the POST is uncertain, never a definite rejection.
    #[test]
    fn lost_answers_are_uncertain() {
        let out = classify(Provider::Soniox, &Ok(Submission::Uncertain { status: None, operation: "submit".into() }));
        assert!(matches!(out, SubmitOutcome::Uncertain { .. }));
        let accepted = classify(
            Provider::Deepgram,
            &Ok(Submission::Accepted {
                remote: RemoteIdentity { provider: media_ports::Provider::Deepgram, project: "p".into(), id: "r1".into() },
            }),
        );
        assert_eq!(accepted, SubmitOutcome::Accepted { remote_id: "r1".into() });
        let not_sent = classify(Provider::Soniox, &Err(TransportError::Configuration("no key".into())));
        assert!(matches!(not_sent, SubmitOutcome::NotSent { .. }));
    }

    #[test]
    fn recovery_status_follows_the_source() {
        assert_eq!(recovery_status("queued"), RecoveryStatus::StillWorking);
        assert_eq!(recovery_status("processing"), RecoveryStatus::StillWorking);
        assert_eq!(recovery_status("completed"), RecoveryStatus::Terminal);
        assert_eq!(recovery_status("error"), RecoveryStatus::Terminal);
    }
}
