//! Ports of the API use cases. `transgate/jobs-postgres` implements [`ApiTx`],
//! [`ApiRateLimiter`] and [`CallbackOutbox`]; the host supplies [`ApiCrypto`]
//! and [`CallbackTransport`]. Same `StoreResult`/`BoxFuture` conventions as P3.
//!
//! Erasure (D10): `JobUow::erase_account_rows` also deletes every key, key use,
//! idempotency record, delivery, attempt and signing secret of the account.
//! The history rule clears `callback_deliveries.payload` with the lines it
//! copied, and the P4 consent DELETE purge clears payloads that carry a summary.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use billing_core::ports::StoreError;

use crate::api::KeyId;
use crate::api::callbacks::{CallbackDelivery, DeliveryId, SendOutcome, Settlement};
use crate::api::idempotency::IdempotencyRecord;
use crate::api::keys::ApiKey;
use crate::api::limits::{RateCounts, RateLimits, RateWindows};
use crate::ai::ports::SummaryOutcome;
use crate::ids::{AccountId, JobId, Micros};
use crate::jobs::domain::Job;
use crate::transcript::Line;
use crate::jobs::app::StartAnswer;
use crate::jobs::ports::{BoxFuture, StoreResult};

/// The API rows of one `JobUow` transaction (`JobUow::api()`), so key writes
/// share the billing account lock and erasure barrier of every account write.
pub trait ApiTx: Send {
    /// `StoreError::Conflict` when the id or digest already exists.
    fn insert_key<'a>(&'a mut self, key: &'a ApiKey) -> StoreResult<'a, ()>;
    /// The key `FOR UPDATE`, any owner.
    fn lock_key(&mut self, id: KeyId) -> StoreResult<'_, Option<ApiKey>>;
    /// Updates `name`, `active`, `last_used_at`, `digest`, `legacy_bcrypt` and `hint`.
    fn save_key<'a>(&'a mut self, key: &'a ApiKey) -> StoreResult<'a, ()>;
    /// The one key with this digest (unique index), any state.
    fn key_by_digest<'a>(&'a mut self, digest: &'a str) -> StoreResult<'a, Option<ApiKey>>;
    /// D2 legacy scan set: active, not expired at `now` (`expires_at` null or
    /// not before `now`), no digest yet, a bcrypt hash present.
    fn legacy_candidates(&mut self, now: Micros) -> StoreResult<'_, Vec<ApiKey>>;
    /// API-005: the account's keys, newest first.
    fn list_keys(&mut self, account: AccountId) -> StoreResult<'_, Vec<ApiKey>>;
    /// D11: one audit row per authenticated request.
    fn record_use<'a>(&'a mut self, usage: &'a KeyUse) -> StoreResult<'a, ()>;

    /// D4: the account's record for `key` created after `expired_before`.
    fn idempotency<'a>(&'a mut self, account: AccountId, key: &'a str, expired_before: Micros) -> StoreResult<'a, Option<IdempotencyRecord>>;
    /// Inserts, replacing an expired record of the same key;
    /// `StoreError::Conflict` while a live one exists.
    fn insert_idempotency<'a>(&'a mut self, record: &'a IdempotencyRecord, expired_before: Micros) -> StoreResult<'a, ()>;
    /// Stores the first answer of a recorded start.
    fn save_idempotency_answer<'a>(&'a mut self, account: AccountId, key: &'a str, answer: &'a StartAnswer) -> StoreResult<'a, ()>;

    /// D5 fan-out from the event relay's transaction: inserts unless a delivery
    /// for `delivery.event_id` exists (UNIQUE); `true` when inserted.
    fn insert_delivery<'a>(&'a mut self, delivery: &'a CallbackDelivery) -> StoreResult<'a, bool>;
}

/// D11: one authenticated request, recorded after the answer is known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyUse {
    pub key: KeyId,
    pub account: AccountId,
    pub at: Micros,
    pub endpoint: String,
    pub method: String,
    pub status: u16,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub duration_ms: u32,
}

/// Cryptography the core may not link (no crypto crates in core).
pub trait ApiCrypto: Send + Sync {
    /// Standard padded base64 of 32 CSPRNG bytes (44 characters).
    fn random_token(&self) -> String;
    /// Lowercase hex SHA-256.
    fn sha256_hex(&self, data: &[u8]) -> String;
    /// Lowercase hex HMAC-SHA256.
    fn hmac_sha256_hex(&self, secret: &[u8], message: &[u8]) -> String;
    /// D2: pgcrypto `crypt(key, hash) = hash` for a migrated `$2a$` hash. May
    /// run on a blocking pool; `false` for a malformed hash.
    fn verify_legacy<'a>(&'a self, key: &'a str, bcrypt_hash: &'a str) -> BoxFuture<'a, bool>;
}

/// D3: consumes one unit in the key's minute **and** hour window in one
/// statement, only when both counts are below their limits, and returns the
/// counts after the call (unchanged when denied). Contract reference:
/// [`crate::api::limits::consume`].
pub trait ApiRateLimiter: Send + Sync {
    fn consume(&self, key: KeyId, windows: RateWindows, limits: RateLimits) -> BoxFuture<'_, Result<RateCounts, StoreError>>;
}

/// A leased delivery. Every write below changes the row only while it still
/// carries this claim's `token` (a worker whose lease ran out cannot settle a
/// newer claim); `false` means the claim was superseded and nothing changed.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaimedDelivery {
    pub delivery: CallbackDelivery,
    pub token: Uuid,
}

/// One attempt record, written before the POST (D5, API-D07) and completed after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallbackAttempt {
    pub delivery: DeliveryId,
    /// 1-based, continuing the delivery's count.
    pub n: u32,
    pub started_at: Micros,
    pub outcome: Option<SendOutcome>,
    pub finished_at: Option<Micros>,
}

pub trait CallbackOutbox: Send + Sync {
    /// Pending deliveries due at `now`, oldest first, each leased for `lease_seconds`.
    fn claim_deliveries(&self, limit: u32, lease_seconds: i64, now: Micros) -> StoreResult<'_, Vec<ClaimedDelivery>>;
    /// Releases the lease without an attempt, due again at `until` (D7 wait).
    fn postpone<'a>(&'a self, claim: &'a ClaimedDelivery, until: Micros) -> StoreResult<'a, bool>;
    /// In one transaction: the attempt row, `attempts = attempt.n` and the body
    /// about to be sent (without `audio_url`, D10). Must commit before the POST.
    fn begin_attempt<'a>(&'a self, claim: &'a ClaimedDelivery, attempt: &'a CallbackAttempt, payload: &'a Value) -> StoreResult<'a, bool>;
    /// In one transaction: the attempt's outcome, then the delivery's state,
    /// next due time and delivery time from `settlement`; the lease is released.
    fn finish_attempt<'a>(&'a self, claim: &'a ClaimedDelivery, attempt: &'a CallbackAttempt, settlement: &'a Settlement) -> StoreResult<'a, bool>;
    /// Ends a delivery without an attempt (its job is gone): `Abandoned`.
    fn abandon<'a>(&'a self, claim: &'a ClaimedDelivery, reason: &'a str, now: Micros) -> StoreResult<'a, bool>;
    /// D5 per-account signing secret: stores `candidate` when the account has
    /// none, and returns the stored one.
    fn signing_secret<'a>(&'a self, account: AccountId, candidate: &'a str) -> StoreResult<'a, String>;
}

/// One POST as sent (API-016, D5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackRequest {
    pub url: String,
    pub body: Vec<u8>,
    /// `Transgate-Delivery-Id`.
    pub delivery: DeliveryId,
    /// `Transgate-Signature`.
    pub signature: String,
}

/// The host's HTTP client: `POST`, `Content-Type: application/json`,
/// `User-Agent: Transgate-Callback/1.0`, both D5 headers, a 10-second timeout,
/// no redirects, and the API-D06 destination check at send time against the
/// address actually connected to (resolve, validate, pin). A refused
/// destination is [`SendOutcome::Refused`] and nothing is sent.
pub trait CallbackTransport: Send + Sync {
    fn post<'a>(&'a self, request: &'a CallbackRequest) -> BoxFuture<'a, SendOutcome>;
}

/// What a delivery reads at send time (API-015). `crate::api::app::JobsCallbackContent`
/// implements it over the job store, `Jobs` and `Ai`.
pub trait CallbackContent: Send + Sync {
    /// The P4 summary outcome; `None` while undecided (D7).
    fn summary_outcome(&self, job: JobId) -> BoxFuture<'_, Result<Option<SummaryOutcome>, String>>;
    /// The job, its lines and the summary its owner can currently read
    /// (consent-fenced); `None` when the job is gone.
    fn content(&self, account: AccountId, job: JobId) -> BoxFuture<'_, Result<Option<CallbackContentFacts>, String>>;
    /// A freshly signed audio URL, or `None` (D10).
    fn audio_url<'a>(&'a self, job: &'a Job) -> BoxFuture<'a, Option<String>>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallbackContentFacts {
    pub job: Job,
    pub lines: Vec<Line>,
    pub summary: Option<String>,
}
