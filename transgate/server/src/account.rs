//! Account composition: trusted billing-host identity facts feed pure account presentation.

use std::sync::Arc;

use axum::Router;
use axum::http::HeaderMap;
use billing_core::types::{AccountId, BoxFuture};
use billing_http::AuthError;
use transgate_billing::Transgate;
use transgate_billing::session::SessionIdentity;
use transgate_core::account::{Account, AccountServices, VerifiedProfile};
use transgate_core::flags::FlagConfig;
use transgate_core::privacy::app::Privacy;
use transgate_http::account::{AccountHttp, AccountIdentity};

struct Profiles(Arc<SessionIdentity>);

impl AccountIdentity for Profiles {
    fn profile<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<VerifiedProfile, AuthError>> {
        Box::pin(async move {
            let profile = self.0.profile(headers).await?;
            Ok(VerifiedProfile {
                account: profile.account,
                email: profile.email,
                full_name: profile.full_name,
                email_confirmed_at: profile.email_confirmed_at,
            })
        })
    }

    fn receipt_account<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Option<AccountId>, AuthError>> {
        Box::pin(self.0.receipt_account(headers))
    }
}

pub fn flags_from(get: &dyn Fn(&str) -> Option<String>) -> Result<FlagConfig, String> {
    match get("TRANSGATE_FEATURE_FLAGS") {
        None => Ok(FlagConfig::default()),
        Some(json) => serde_json::from_str(&json).map_err(|error| format!("TRANSGATE_FEATURE_FLAGS must contain a FlagConfig JSON object: {error}")),
    }
}

pub fn router(billing: &Transgate, services: AccountServices, privacy: Arc<Privacy>, flags: FlagConfig, ai_chat: bool) -> Router {
    let clock = services.clock.clone();
    transgate_http::account::router(Arc::new(AccountHttp {
        account: Arc::new(Account::new(services)),
        privacy,
        identity: billing.identity.clone(),
        profiles: Arc::new(Profiles(billing.session.clone())),
        clock,
        flags,
        ai_chat,
    }))
}
