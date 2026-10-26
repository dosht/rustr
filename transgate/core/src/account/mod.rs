//! Account-level use cases the signed-in app needs beyond jobs and billing
//! (P5 D7): the verified session profile with effective capabilities, job
//! feedback, and support requests with their two ordered mails.

pub mod ports;

use std::sync::Arc;

use billing_core::types::Clock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::flags::{Capabilities, FlagConfig, Subject};
use crate::ids::{AccountId, JobId, Micros};
use crate::jobs::app::JobsError;
use crate::jobs::events::SUPPORT_ADDRESS;
use crate::jobs::ports::{JobStore, JobUow};

// ---- session profile --------------------------------------------------------

/// What the host verified about the caller: identity-provider facts and the
/// customer directory's name. The host builds it after normal authentication,
/// so an erased account never reaches here (410 first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedProfile {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    /// The identity provider's `email_confirmed_at` (WEB-IDN-021 "Joined on"),
    /// never the account creation instant.
    pub email_confirmed_at: Option<Micros>,
}

/// `GET /api/session`: the profile and effective capabilities, never the
/// allow-lists behind them. Balance, plan and consent keep their own reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionProfile {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    /// IDN-026.
    pub display_name: String,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub joined_on: Option<Micros>,
    pub capabilities: Capabilities,
}

/// IDN-026: `full_name`, else the part of the email before `@`.
pub fn display_name(full_name: Option<&str>, email: &str) -> String {
    match full_name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => email.split('@').next().unwrap_or_default().to_string(),
    }
}

/// The session answer for a verified caller on a page locale. `ai_chat` is
/// the AI host's chat switch (FLG-009).
pub fn session_profile(profile: &VerifiedProfile, flags: &FlagConfig, locale: Option<&str>, ai_chat: bool) -> SessionProfile {
    let user_id = profile.account.0.to_string();
    let subject = Subject { email: Some(&profile.email), user_id: Some(&user_id), locale };
    SessionProfile {
        account: profile.account,
        email: profile.email.clone(),
        full_name: profile.full_name.clone(),
        display_name: display_name(profile.full_name.as_deref(), &profile.email),
        joined_on: profile.email_confirmed_at,
        capabilities: flags.capabilities(&subject, ai_chat),
    }
}

// ---- feedback ---------------------------------------------------------------

/// The three source options and their stored values (`job_feedback.rating`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "0 | 5 | 10"))]
#[serde(try_from = "i16", into = "i16")]
pub enum FeedbackRating {
    Disappointed,
    Satisfied,
    Outstanding,
}

impl FeedbackRating {
    pub const ALL: [FeedbackRating; 3] = [FeedbackRating::Disappointed, FeedbackRating::Satisfied, FeedbackRating::Outstanding];

    pub fn value(self) -> i16 {
        match self {
            FeedbackRating::Disappointed => 0,
            FeedbackRating::Satisfied => 5,
            FeedbackRating::Outstanding => 10,
        }
    }
}

impl From<FeedbackRating> for i16 {
    fn from(r: FeedbackRating) -> i16 {
        r.value()
    }
}

impl TryFrom<i16> for FeedbackRating {
    type Error = String;

    fn try_from(v: i16) -> Result<FeedbackRating, String> {
        FeedbackRating::ALL.into_iter().find(|r| r.value() == v).ok_or_else(|| format!("rating must be 0, 5 or 10, got {v}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFeedback {
    pub id: Uuid,
    pub job: JobId,
    pub account: AccountId,
    pub rating: FeedbackRating,
    /// The source stored the comment box as typed, empty included.
    pub comment: String,
    pub created_at: Micros,
}

// ---- support ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SupportId(pub Uuid);

/// One support message. Recipient facts (address, name) are read from the
/// customer directory at delivery, like job mails; the request keeps only
/// what the user wrote and the job they were looking at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportRequest {
    pub id: SupportId,
    pub account: AccountId,
    /// The job route open when the message was sent (`router.query.jobId`).
    pub job: Option<JobId>,
    pub message: String,
    pub created_at: Micros,
}

/// EML-006: the support mail first, then the acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportMail {
    ToSupport,
    Acknowledgement,
}

pub const SUPPORT_ACK_SUBJECT: &str = "We've received your message!";

impl SupportMail {
    pub const ORDER: [SupportMail; 2] = [SupportMail::ToSupport, SupportMail::Acknowledgement];

    /// `None`: the acknowledgement of an owner without an address is skipped.
    pub fn recipient(self, owner_email: Option<&str>) -> Option<String> {
        match self {
            SupportMail::ToSupport => Some(SUPPORT_ADDRESS.to_string()),
            SupportMail::Acknowledgement => owner_email.filter(|e| !e.trim().is_empty()).map(str::to_string),
        }
    }

    /// The source subjects: `SUPPORT: {email}, jobId={jobId}` (an absent job
    /// printed as `undefined`), then the fixed acknowledgement subject.
    pub fn subject(self, request: &SupportRequest, owner_email: Option<&str>) -> String {
        match self {
            SupportMail::ToSupport => {
                let email = owner_email.filter(|e| !e.trim().is_empty()).unwrap_or("undefined");
                let job = request.job.map(|j| j.0.to_string()).unwrap_or_else(|| "undefined".into());
                format!("SUPPORT: {email}, jobId={job}")
            }
            SupportMail::Acknowledgement => SUPPORT_ACK_SUBJECT.to_string(),
        }
    }

    /// Stable per logical mail, so a retried request never sends a mail twice.
    pub fn idempotency_key(self, id: SupportId) -> String {
        match self {
            SupportMail::ToSupport => format!("support-{}-to-support", id.0),
            SupportMail::Acknowledgement => format!("support-{}-acknowledgement", id.0),
        }
    }
}

/// The acknowledgement greeting name: `full_name`, else "there".
pub fn acknowledgement_name(full_name: Option<&str>) -> &str {
    full_name.filter(|n| !n.trim().is_empty()).unwrap_or("there")
}

// ---- use cases --------------------------------------------------------------

#[derive(Clone)]
pub struct AccountServices {
    pub store: Arc<dyn JobStore>,
    pub clock: Arc<dyn Clock>,
}

pub struct Account {
    s: AccountServices,
}

impl Account {
    pub fn new(services: AccountServices) -> Account {
        Account { s: services }
    }

    fn now(&self) -> Micros {
        self.s.clock.now()
    }

    /// The erasure barrier refuses before any transaction opens; inside it the
    /// billing account lock refuses an erasure that opened meanwhile.
    async fn guard(&self, account: AccountId) -> Result<(), JobsError> {
        if self.s.store.barrier(account).await? {
            return Err(JobsError::ErasureInProgress);
        }
        Ok(())
    }

    async fn finish<T>(uow: Box<dyn JobUow>, result: Result<T, JobsError>) -> Result<T, JobsError> {
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

    /// Feedback on an owned job (`FeedbackModal`). Another account's job and a
    /// missing one are the same `NotFound`.
    pub async fn submit_feedback(&self, account: AccountId, job: JobId, rating: FeedbackRating, comment: String) -> Result<JobFeedback, JobsError> {
        self.guard(account).await?;
        let feedback = JobFeedback { id: Uuid::new_v4(), job, account, rating, comment, created_at: self.now() };
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            if uow.lock_jobs(account, &[job]).await?.is_empty() {
                return Err(JobsError::NotFound);
            }
            uow.account().insert_feedback(&feedback).await?;
            Ok::<_, JobsError>(())
        }
        .await;
        Self::finish(uow, result).await?;
        Ok(feedback)
    }

    /// API-F38: queues the support message for ordered delivery. As in the
    /// source, the message is sent as typed and the job is only the route the
    /// person had open: context for support, not an access to anything.
    pub async fn request_support(&self, account: AccountId, job: Option<JobId>, message: String) -> Result<SupportId, JobsError> {
        self.guard(account).await?;
        let request = SupportRequest { id: SupportId(Uuid::new_v4()), account, job, message, created_at: self.now() };
        let mut uow = self.s.store.begin().await?;
        let result = async {
            uow.billing().await?.lock_account(account).await?;
            uow.account().insert_support(&request).await?;
            Ok::<_, JobsError>(())
        }
        .await;
        Self::finish(uow, result).await?;
        Ok(request.id)
    }
}

#[cfg(test)]
mod tests;
