//! taskr's billing identity is taskr's own: the same verifier the server
//! configured (Supabase JWT, or the explicitly enabled dev token) answers who
//! is calling. This adapter only maps its verified principal onto a billing
//! account.
//!
//! A real Supabase subject is a UUID and is kept as it is. taskr's development
//! subjects (`dev-<email>`) are not UUIDs; they hash deterministically into one,
//! so a retry reaches the same account. Nothing here verifies a token itself and
//! nothing falls back to anonymous access.

use std::sync::Arc;

use axum::http::{HeaderMap, header};
use billing_core::types::{AccountId, BoxFuture};
use billing_http::{AuthError, BillingIdentity, Caller};
use taskr_core::shared::access::{Identity, IdentityError};
use uuid::Uuid;

/// Namespace for taskr subjects that are not UUIDs.
pub const SUBJECTS: Uuid = Uuid::from_u128(0x3f0e_5a91_62c4_4d7b_9e18_a2c6_71b4_08d5);

pub struct TaskrIdentity {
    inner: Arc<dyn Identity>,
}

impl TaskrIdentity {
    pub fn new(inner: Arc<dyn Identity>) -> TaskrIdentity {
        TaskrIdentity { inner }
    }
}

impl BillingIdentity for TaskrIdentity {
    fn authenticate<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Caller, AuthError>> {
        Box::pin(async move {
            let token = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| AuthError::Unauthenticated("no bearer token".into()))?;
            match self.inner.verify(token).await {
                Ok(principal) => Ok(Caller {
                    account: AccountId::from_subject(&principal.user_id.0, &SUBJECTS),
                    email: principal.email.as_str().to_string(),
                    full_name: None,
                    // taskr has no billing administrators.
                    is_admin: false,
                }),
                Err(IdentityError::Invalid(why)) => Err(AuthError::Unauthenticated(why)),
                Err(IdentityError::Unavailable(why)) => Err(AuthError::Unavailable(why)),
            }
        })
    }
}
