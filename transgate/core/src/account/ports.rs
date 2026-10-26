//! Rows the account use cases write inside a [`crate::jobs::ports::JobUow`]
//! (`JobUow::account()`), so a write shares the transaction, the billing
//! account lock and the erasure barrier fence of every other account write.
//! `transgate/jobs-postgres` implements it; the fake in `crate::jobs::testing`
//! mirrors it.
//!
//! Erasure and hard delete: `JobUow::erase_account_rows` deletes the account's
//! feedback and undelivered support requests; `JobUow::hard_delete_job`
//! deletes that job's feedback (source PRV-005 cascade).

use crate::account::{JobFeedback, SupportRequest};
use crate::ids::Micros;
use crate::jobs::ports::StoreResult;

pub trait AccountTx: Send {
    /// Appends one rating. The source allows any number per job.
    fn insert_feedback<'a>(&'a mut self, feedback: &'a JobFeedback) -> StoreResult<'a, ()>;

    /// Queues one support request; delivery sends its two mails in order.
    fn insert_support<'a>(&'a mut self, request: &'a SupportRequest) -> StoreResult<'a, ()>;
}

/// The delivery side of the support outbox, used by the host's mail worker
/// with the same claim/lease/ack protocol as job event mails.
///
/// A claim is fenced by its `attempts` number, which every claim increments:
/// `ack_support` and `release_support` change the row only while it still
/// carries the claim's number, so a worker whose lease expired during mail
/// I/O can neither delete nor reschedule a newer claim. (A mail it sent late
/// is not repeated: each mail has a stable idempotency key.)
pub trait SupportOutbox: Send + Sync {
    /// Unclaimed requests and those whose lease or retry delay ran out,
    /// oldest first; each is leased for `lease_seconds` and its `attempts`
    /// incremented.
    fn claim_support(&self, limit: u32, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<ClaimedSupport>>;
    /// Both mails were sent: the request and its message are deleted.
    /// `false` when the claim was superseded (nothing changed).
    fn ack_support<'a>(&'a self, claim: &'a ClaimedSupport) -> StoreResult<'a, bool>;
    /// A mail failed: claimable again at `retry_at`, the error kept for
    /// operators. `false` when the claim was superseded (nothing changed).
    fn release_support<'a>(&'a self, claim: &'a ClaimedSupport, error: &'a str, retry_at: Micros) -> StoreResult<'a, bool>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimedSupport {
    pub request: SupportRequest,
    /// The claim's fencing number.
    pub attempts: i32,
}
