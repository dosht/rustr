//! The three job mails (EML-004/005) as plain text. Core decides who is mailed and which
//! subject and delivery key each message has (`transgate_core::jobs::events`); the recipient
//! facts come from the trusted customer directory, never from the event. The source rendered
//! React Email HTML; the delivery adapter sends text, so each template keeps its words and its
//! fields in plain form.

use billing_core::ports::MailMessage;
use serde_json::Value;
use transgate_core::jobs::events::{JobMail, SUPPORT_ADDRESS};
use transgate_core::{AccountId, JobId};

/// What the directory knows about the job's owner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Owner {
    pub email: Option<String>,
    pub full_name: Option<String>,
}

/// The stable key of one logical message: a retry of the same event and target is the same key,
/// so a provider that honors it sends nothing twice.
pub fn idempotency_key(event: i64, mail: &JobMail) -> String {
    format!("job-event-{event}-{}", mail.target_key())
}

/// `None` when there is nobody to send to: the user mail of an owner without an address.
pub fn render(mail: &JobMail, job: JobId, account: AccountId, owner: &Owner) -> Option<MailMessage> {
    let subject = mail.subject().to_string();
    match mail {
        JobMail::UserFailedFile => {
            let to = owner.email.clone().filter(|e| !e.trim().is_empty())?;
            let name = owner.full_name.as_deref().filter(|n| !n.trim().is_empty()).unwrap_or("there");
            Some(MailMessage {
                to,
                subject,
                text: format!(
                    "Hello {name},\n\n\
                     We wanted to inform you that our system has detected an issue with one of your files. \
                     We are currently working to fix it automatically and expect to resolve the issue within the next few hours.\n\n\
                     If the problem continues, a member of our support team will contact you to provide further assistance.\n\n\
                     We apologize for any inconvenience this may cause and appreciate your understanding.\n"
                ),
            })
        }
        JobMail::SupportJobFailed { error_json, language } | JobMail::SupportPartialErrors { error_json, language } => {
            let heading = match mail {
                JobMail::SupportJobFailed { .. } => format!("Job {job} failed."),
                _ => format!("Job {job} completed with errors."),
            };
            let email = owner.email.as_deref().filter(|e| !e.trim().is_empty()).unwrap_or("unknown email");
            Some(MailMessage {
                to: SUPPORT_ADDRESS.to_string(),
                subject,
                text: format!("{heading}\n\nUser ID: {account}\nUser email: {email}\nLanguage: {language}\n\n{}", error_details(error_json)),
            })
        }
    }
}

/// The source's error display: one block per error with stage, category, message, segment and
/// traceback, or a line saying the details cannot be parsed.
fn error_details(error_json: &str) -> String {
    let parsed: Value = serde_json::from_str(error_json).unwrap_or(Value::Null);
    let items: Vec<&Value> = match &parsed {
        Value::Array(items) => items.iter().collect(),
        Value::Null => vec![],
        other => vec![other],
    };
    let blocks: Vec<String> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let object = item.as_object()?;
            let mut lines = vec![format!("Error #{}", i + 1)];
            for (label, key) in [("Stage", "stage"), ("Category", "category"), ("Message", "message"), ("Segment", "segment"), ("Traceback", "traceback")] {
                if let Some(text) = object.get(key).and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    lines.push(format!("{label}: {text}"));
                }
            }
            Some(lines.join("\n"))
        })
        .collect();
    if blocks.is_empty() { "Unable to parse error details".to_string() } else { blocks.join("\n\n") }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn owner(email: Option<&str>, name: Option<&str>) -> Owner {
        Owner { email: email.map(str::to_string), full_name: name.map(str::to_string) }
    }

    const ACCOUNT: AccountId = AccountId(Uuid::from_u128(7));

    #[test]
    fn the_user_mail_greets_by_name_or_there_and_is_skipped_without_an_address() {
        let named = render(&JobMail::UserFailedFile, JobId(9), ACCOUNT, &owner(Some("ada@example.test"), Some("Ada"))).unwrap();
        assert_eq!((named.to.as_str(), named.subject.as_str()), ("ada@example.test", "Failed File Detected"));
        assert!(named.text.starts_with("Hello Ada,\n"));
        let anonymous = render(&JobMail::UserFailedFile, JobId(9), ACCOUNT, &owner(Some("ada@example.test"), None)).unwrap();
        assert!(anonymous.text.starts_with("Hello there,\n"));
        assert_eq!(render(&JobMail::UserFailedFile, JobId(9), ACCOUNT, &owner(None, Some("Ada"))), None);
        assert_eq!(render(&JobMail::UserFailedFile, JobId(9), ACCOUNT, &owner(Some("  "), None)), None, "a blank address is no address");
    }

    #[test]
    fn the_support_mails_carry_job_user_language_and_the_parsed_errors() {
        let error = json!([{"stage": "transcription", "category": "partial", "message": "Word repeated", "segment": "00:05-00:09", "traceback": null}]).to_string();
        let mail = JobMail::SupportJobFailed { error_json: error.clone(), language: "English".into() };
        let sent = render(&mail, JobId(42), ACCOUNT, &owner(Some("ada@example.test"), None)).unwrap();
        assert_eq!((sent.to.as_str(), sent.subject.as_str()), ("support@transgate.ai", "Job Failed"));
        let user_id = format!("User ID: {ACCOUNT}");
        for needle in ["Job 42 failed.", user_id.as_str(), "User email: ada@example.test", "Language: English", "Error #1", "Stage: transcription", "Category: partial", "Message: Word repeated", "Segment: 00:05-00:09"] {
            assert!(sent.text.contains(needle), "{needle}\n{}", sent.text);
        }
        assert!(!sent.text.contains("Traceback"), "a null traceback is not shown");

        let partial = render(&JobMail::SupportPartialErrors { error_json: error, language: "unknown".into() }, JobId(42), ACCOUNT, &Owner::default()).unwrap();
        assert_eq!(partial.subject, "Job Partial Errors");
        assert!(partial.text.contains("completed with errors") && partial.text.contains("User email: unknown email"));
    }

    #[test]
    fn an_error_that_is_not_the_structured_shape_says_so_instead_of_inventing_fields() {
        for bad in ["\"unknown error\"", "not json", "42", "[]", "null"] {
            let mail = JobMail::SupportJobFailed { error_json: bad.into(), language: "x".into() };
            let sent = render(&mail, JobId(1), ACCOUNT, &Owner::default()).unwrap();
            assert!(sent.text.ends_with("Unable to parse error details"), "{bad}: {}", sent.text);
        }
        let single = JobMail::SupportJobFailed { error_json: json!({"stage": "upload", "message": "boom"}).to_string(), language: "x".into() };
        assert!(render(&single, JobId(1), ACCOUNT, &Owner::default()).unwrap().text.contains("Error #1\nStage: upload\nMessage: boom"), "a lone object is one error");
    }

    #[test]
    fn a_retry_of_the_same_event_and_target_has_the_same_key_and_another_target_another() {
        let user = JobMail::UserFailedFile;
        let support = JobMail::SupportJobFailed { error_json: "null".into(), language: "x".into() };
        assert_eq!(idempotency_key(7, &user), idempotency_key(7, &user));
        assert_ne!(idempotency_key(7, &user), idempotency_key(7, &support));
        assert_ne!(idempotency_key(7, &user), idempotency_key(8, &user));
    }
}
