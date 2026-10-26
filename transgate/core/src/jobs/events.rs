//! Outbox events written in the same transaction as the transition that caused
//! them, and the job mails derived from them (JOB-033/034, EML-004/005).
//!
//! The source posted every row update to a hook that mailed on the *current*
//! status, so any later write to a failed row mailed again. Events here exist
//! once per genuine transition (recorded correction C7).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{AccountId, JobId, Micros};
use crate::jobs::domain::{Job, JobStatus, Provider};
use crate::languages::Language;

pub const SUPPORT_ADDRESS: &str = "support@transgate.ai";
pub const FAILED_FILE_SUBJECT: &str = "Failed File Detected";
pub const JOB_FAILED_SUBJECT: &str = "Job Failed";
pub const JOB_PARTIAL_ERRORS_SUBJECT: &str = "Job Partial Errors";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    /// A job entered `completed`. P4 (general summary) and P7 (customer
    /// callback) consume it; neither can fail the job (JOB-033).
    Completed {
        job: JobId,
        account: AccountId,
        provider: Option<Provider>,
        language: Option<Language>,
        /// The row's error at completion; non-null means a partial-errors mail.
        error: Option<Value>,
        callback_url: Option<String>,
        at: Micros,
    },
    /// A job entered `failed`.
    Failed {
        job: JobId,
        account: AccountId,
        language: Option<Language>,
        error: Option<Value>,
        at: Micros,
    },
}

impl JobEvent {
    /// The event for a transition, if it has one.
    pub fn for_transition(before: JobStatus, after: &Job, at: Micros) -> Option<JobEvent> {
        if before == after.status {
            return None;
        }
        match after.status {
            JobStatus::Completed => Some(JobEvent::Completed {
                job: after.id,
                account: after.account,
                provider: after.provider,
                language: after.language,
                error: after.error.clone(),
                callback_url: after.callback_url.clone(),
                at,
            }),
            JobStatus::Failed => Some(JobEvent::Failed {
                job: after.id,
                account: after.account,
                language: after.language,
                error: after.error.clone(),
                at,
            }),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            JobEvent::Completed { .. } => "job.completed",
            JobEvent::Failed { .. } => "job.failed",
        }
    }

    pub fn job(&self) -> JobId {
        match self {
            JobEvent::Completed { job, .. } | JobEvent::Failed { job, .. } => *job,
        }
    }
}

/// Who receives which job mail. Recipient addresses and names come from the
/// trusted customer directory, never from the event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum JobMail {
    /// EML-004 user mail: greeting `full_name` or "there"; skipped without an email address.
    UserFailedFile,
    /// EML-004/005 support mail with job id, JSON error ("unknown error" when null),
    /// user id, user email ("unknown email") and language ("unknown").
    SupportJobFailed { error_json: String, language: String },
    SupportPartialErrors { error_json: String, language: String },
}

impl JobMail {
    pub fn subject(&self) -> &'static str {
        match self {
            JobMail::UserFailedFile => FAILED_FILE_SUBJECT,
            JobMail::SupportJobFailed { .. } => JOB_FAILED_SUBJECT,
            JobMail::SupportPartialErrors { .. } => JOB_PARTIAL_ERRORS_SUBJECT,
        }
    }

    /// Stable delivery target key, so a retry after a partial failure skips what was sent.
    pub fn target_key(&self) -> &'static str {
        match self {
            JobMail::UserFailedFile => "user_failed_file",
            JobMail::SupportJobFailed { .. } => "support_job_failed",
            JobMail::SupportPartialErrors { .. } => "support_partial_errors",
        }
    }
}

fn error_json(error: &Option<Value>) -> String {
    match error {
        Some(v) if !v.is_null() => v.to_string(),
        _ => Value::String("unknown error".into()).to_string(),
    }
}

fn language_text(language: Option<Language>) -> String {
    language.map(|l| l.name().to_string()).unwrap_or_else(|| "unknown".into())
}

/// EML-004: a failure mails the user, then support. EML-005: a completion with
/// a non-null error mails support only.
pub fn mails_for(event: &JobEvent) -> Vec<JobMail> {
    match event {
        JobEvent::Failed { error, language, .. } => vec![
            JobMail::UserFailedFile,
            JobMail::SupportJobFailed { error_json: error_json(error), language: language_text(*language) },
        ],
        JobEvent::Completed { error: Some(e), language, .. } if !e.is_null() => {
            vec![JobMail::SupportPartialErrors { error_json: error_json(&Some(e.clone())), language: language_text(*language) }]
        }
        JobEvent::Completed { .. } => vec![],
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn completed(error: Option<Value>) -> JobEvent {
        JobEvent::Completed {
            job: JobId(1),
            account: AccountId(uuid::Uuid::nil()),
            provider: Some(Provider::Soniox),
            language: Language::from_name("English"),
            error,
            callback_url: None,
            at: Micros(0),
        }
    }

    // EML-005: a completion that kept an earlier rate-limit error still mails support.
    #[test]
    fn completion_with_error_mails_support_only() {
        let mails = mails_for(&completed(Some(json!({"message": "Rate limit exceeded, will retry"}))));
        assert_eq!(mails.len(), 1);
        assert_eq!(mails[0].subject(), "Job Partial Errors");
        assert!(mails_for(&completed(None)).is_empty());
    }

    // EML-004: user first, then support, with the source fallbacks.
    #[test]
    fn failure_mails_user_and_support() {
        let mails = mails_for(&JobEvent::Failed {
            job: JobId(2),
            account: AccountId(uuid::Uuid::nil()),
            language: None,
            error: None,
            at: Micros(0),
        });
        assert_eq!(mails[0], JobMail::UserFailedFile);
        assert_eq!(
            mails[1],
            JobMail::SupportJobFailed { error_json: "\"unknown error\"".into(), language: "unknown".into() }
        );
    }
}
