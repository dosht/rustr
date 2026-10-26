//! Optional `Idempotency-Key` on public starts (D4). Per account, kept for
//! 24 hours and written in the start transaction, after the billing account
//! lock (so two requests with one key serialize). The same body answers the
//! stored 201 again without another charge; another body is 409
//! `request_conflict`, the P4 request-key precedent (`ai/mod.rs`). Without the
//! header nothing changes (the source has none: a retried POST charged twice).

use serde::{Deserialize, Serialize};

use crate::ids::{AccountId, JobId, MICROS_PER_DAY, Micros};
use crate::jobs::app::StartAnswer;

/// How long a key is remembered.
pub const RETENTION: i64 = MICROS_PER_DAY;

/// A caller's key and the digest of the request it came with. The key is the
/// caller's header text; the digest is the host's SHA-256 of the canonical request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdempotencyClaim {
    pub key: String,
    pub request_digest: String,
}

/// The stored record. `jobs` is written with the charge; `answer` once the
/// first response is known (after dispatch).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub account: AccountId,
    pub key: String,
    pub request_digest: String,
    pub request_type: String,
    pub jobs: Vec<JobId>,
    pub answer: Option<StartAnswer>,
    pub created_at: Micros,
}

/// Records created at or before this instant are forgotten.
pub fn expired_before(now: Micros) -> Micros {
    Micros(now.0 - RETENTION)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// Not seen within the window: start, and record it.
    New,
    /// The same request again: answer it, charge nothing.
    Replay(Box<IdempotencyRecord>),
    /// The key belongs to another request.
    Conflict,
}

pub fn lookup(stored: Option<IdempotencyRecord>, claim: &IdempotencyClaim) -> Lookup {
    match stored {
        None => Lookup::New,
        Some(record) if record.request_digest == claim.request_digest => Lookup::Replay(Box::new(record)),
        Some(_) => Lookup::Conflict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(digest: &str) -> IdempotencyRecord {
        IdempotencyRecord {
            account: AccountId(uuid::Uuid::from_u128(1)),
            key: "k".into(),
            request_digest: digest.into(),
            request_type: "audioUrls".into(),
            jobs: vec![JobId(4)],
            answer: None,
            created_at: Micros(0),
        }
    }

    // D4: same key and body replays; same key with another body is a conflict.
    #[test]
    fn d4_same_body_replays_and_another_body_conflicts() {
        let claim = IdempotencyClaim { key: "k".into(), request_digest: "aa".into() };
        assert_eq!(lookup(None, &claim), Lookup::New);
        assert_eq!(lookup(Some(record("aa")), &claim), Lookup::Replay(Box::new(record("aa"))));
        assert_eq!(lookup(Some(record("bb")), &claim), Lookup::Conflict);
        assert_eq!(expired_before(Micros(2 * RETENTION)), Micros(RETENTION));
    }
}
