//! The public API (P7): API-key actors and scopes, the pure `can` decision,
//! key verification, the per-key limiter, idempotent public starts and the
//! customer callback policy. Decisions: `docs/plans/p7-api/decisions.md`.
//!
//! Pure Rust like the rest of the core: hashing, key generation, bcrypt and
//! HMAC come through [`ports::ApiCrypto`]; rows through [`ports::ApiTx`] on the
//! existing `JobUow`; the limiter, callback outbox and transport are host ports.
//! Order on every public request (D1): authenticate, then `can`, then consume
//! the limiter, then the use case.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use billing_core::ports::StoreError;

use crate::jobs::app::JobsError;

pub mod app;
pub mod auth;
pub mod callbacks;
pub mod idempotency;
pub mod keys;
pub mod limits;
pub mod ports;

pub use auth::{Action, Actor, Resource, Scope, can};

/// One API key (the source uuid is kept on migration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyId(pub Uuid);

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

/// Errors a shell maps to HTTP; `http_status` and `code` are the intended mapping.
#[derive(Clone, Debug, PartialEq)]
pub enum ApiError {
    /// 401: missing, malformed, unknown, inactive or expired key (API-001/003).
    /// The message is the source's where it had one.
    Unauthorized(&'static str),
    /// 403: the actor may not do this (API-D01 scopes, API-D02 owner).
    Forbidden,
    /// 404: not found, or not the actor's.
    NotFound,
    /// 400 with a message.
    Invalid(String),
    /// 429 with the window state for the rate headers (API-D04, RTL-003).
    RateLimited(limits::ApiRate),
    /// 409 `request_conflict`: an `Idempotency-Key` reused for another body (D4).
    RequestConflict,
    /// 410: the account is being erased.
    Erased,
    /// 503: the limiter or another required store failed (D3 fail closed).
    Unavailable(String),
    /// A job use case's own refusal, mapped by `JobsError`.
    Jobs(JobsError),
}

impl ApiError {
    pub fn http_status(&self) -> u16 {
        match self {
            ApiError::Unauthorized(_) => 401,
            ApiError::Forbidden => 403,
            ApiError::NotFound => 404,
            ApiError::Invalid(_) => 400,
            ApiError::RateLimited(_) => 429,
            ApiError::RequestConflict => 409,
            ApiError::Erased => 410,
            ApiError::Unavailable(_) => 503,
            ApiError::Jobs(e) => e.http_status(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::Forbidden => "forbidden",
            ApiError::NotFound => "not_found",
            ApiError::Invalid(_) => "invalid_request",
            ApiError::RateLimited(_) => "rate_limited",
            ApiError::RequestConflict => "request_conflict",
            ApiError::Erased => "account_deletion_in_progress",
            ApiError::Unavailable(_) => "unavailable",
            ApiError::Jobs(e) => e.code(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            ApiError::Unauthorized(m) => (*m).into(),
            ApiError::Forbidden => "Insufficient permissions".into(),
            ApiError::NotFound => "Not found".into(),
            ApiError::Invalid(m) => m.clone(),
            ApiError::RateLimited(_) => "Rate limit exceeded".into(),
            ApiError::RequestConflict => "This Idempotency-Key was already used for a different request".into(),
            ApiError::Erased => "Account deletion in progress".into(),
            ApiError::Unavailable(_) => "The service is temporarily unavailable".into(),
            ApiError::Jobs(e) => e.message(),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ApiError {}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::AccountErased(_) => ApiError::Erased,
            other => ApiError::Unavailable(other.to_string()),
        }
    }
}

impl From<JobsError> for ApiError {
    fn from(e: JobsError) -> Self {
        match e {
            JobsError::ErasureInProgress => ApiError::Erased,
            JobsError::RequestConflict => ApiError::RequestConflict,
            other => ApiError::Jobs(other),
        }
    }
}
