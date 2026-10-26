//! Host-only identity paths. A receipt credential grants an account id, not roles.

use std::sync::Arc;

use axum::http::HeaderMap;
use billing_core::types::{AccountId, Micros};
use billing_http::AuthError;

use crate::ContactRefresh;

pub struct VerifiedSession {
    pub account: AccountId,
    pub email: String,
    pub full_name: Option<String>,
    pub email_confirmed_at: Option<Micros>,
}

pub struct SessionIdentity {
    pub(super) identity: Arc<ContactRefresh>,
}

impl SessionIdentity {
    /// Ordinary authentication fences both sides of the trusted profile lookup. Reuse its
    /// fetched facts for contact adoption/refresh rather than requesting the user twice.
    pub async fn profile(&self, headers: &HeaderMap) -> Result<VerifiedSession, AuthError> {
        let account = self.identity.inner.authenticate(headers).await?.account;
        if self.identity.erasing(account).await? {
            return Err(AuthError::AccountErased(account));
        }
        let contact = self.identity.directory.verified_contact(account).await;
        if self.identity.erasing(account).await? {
            return Err(AuthError::AccountErased(account));
        }
        match contact.map_err(|e| AuthError::Unavailable(format!("the account profile cannot be confirmed: {e}")))? {
            Some((customer, email_confirmed_at)) => Ok(VerifiedSession {
                account: customer.account,
                email: customer.email,
                full_name: customer.full_name,
                email_confirmed_at,
            }),
            None => Err(AuthError::Unavailable("the trusted account profile directory is not configured".into())),
        }
    }

    /// A read-only receipt must not run ContactRefresh for an unbarred caller. The inner
    /// verifier still validates the JWT cryptographically; all email/admin authority is
    /// discarded before the barrier query. None means a verified but unbarred account.
    pub async fn receipt_account(&self, headers: &HeaderMap) -> Result<Option<AccountId>, AuthError> {
        let account = self.identity.inner.authenticate(headers).await?.account;
        Ok(self.identity.erasing(account).await?.then_some(account))
    }
}
