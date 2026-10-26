//! Account endpoints share ordinary auth, except the deliberately read-only receipt path.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use billing_core::types::{BoxFuture, Clock};
use billing_http::{AuthError, BillingIdentity, Caller};
use serde::Deserialize;
use transgate_core::AccountId;
use transgate_core::account::{Account, VerifiedProfile, session_profile};
use transgate_core::flags::FlagConfig;
use transgate_core::jobs::app::JobsError;
use transgate_core::privacy::app::Privacy;

use super::{Denied, HttpError};

/// Host composition verifies credentials and trusted directory facts. The receipt operation
/// must not hydrate contacts and can return only a barred subject, never a privileged actor.
pub trait AccountIdentity: Send + Sync {
    fn profile<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<VerifiedProfile, AuthError>>;
    fn receipt_account<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Option<AccountId>, AuthError>>;
}

pub struct AccountHttp {
    pub account: Arc<Account>,
    pub privacy: Arc<Privacy>,
    pub identity: Arc<dyn BillingIdentity>,
    pub profiles: Arc<dyn AccountIdentity>,
    pub clock: Arc<dyn Clock>,
    pub flags: FlagConfig,
    pub ai_chat: bool,
}

pub fn router(state: Arc<AccountHttp>) -> Router {
    Router::new()
        .route("/api/session", get(session))
        .route("/api/jobs/{id}/feedback", post(feedback))
        .route("/api/support", post(support))
        .route("/api/account/erasure/status", get(receipt))
        .with_state(state)
        .layer(middleware::from_fn(no_store))
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

impl AccountHttp {
    async fn caller(&self, headers: &HeaderMap) -> Result<Caller, HttpError> {
        self.identity.authenticate(headers).await.map_err(|e| super::authentication_error(e, Denied::Json))
    }

    fn failure(&self, error: JobsError) -> Response {
        super::jobs_error_at(self.clock.as_ref(), error)
    }

    /// An owned-job route (feedback): a missing billing account owns no jobs, so it is `not_found`.
    fn owned_failure(&self, error: JobsError) -> Response {
        super::owned_error_at(self.clock.as_ref(), error)
    }
}

#[derive(Deserialize)]
struct SessionQuery {
    locale: Option<String>,
}

async fn session(State(state): State<Arc<AccountHttp>>, headers: HeaderMap, Query(query): Query<SessionQuery>) -> Response {
    match state.profiles.profile(&headers).await {
        Ok(profile) => super::ok_json(StatusCode::OK, &session_profile(&profile, &state.flags, query.locale.as_deref(), state.ai_chat)),
        Err(error) => super::authentication_error(error, Denied::Json).into_response(),
    }
}

async fn feedback(State(state): State<Arc<AccountHttp>>, headers: HeaderMap, Path(id): Path<i64>, body: Bytes) -> Response {
    let caller = match state.caller(&headers).await {
        Ok(caller) => caller,
        Err(error) => return error.into_response(),
    };
    let request = match transgate_api::account::parse_feedback(&body) {
        Ok(request) => request,
        Err(error) => return super::invalid(error),
    };
    match state.account.submit_feedback(caller.account, transgate_core::JobId(id), request.rating, request.comment).await {
        Ok(feedback) => super::ok_json(StatusCode::CREATED, &feedback),
        Err(error) => state.owned_failure(error),
    }
}

async fn support(State(state): State<Arc<AccountHttp>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = match state.caller(&headers).await {
        Ok(caller) => caller,
        Err(error) => return error.into_response(),
    };
    let request = match transgate_api::account::parse_support(&body) {
        Ok(request) => request,
        Err(error) => return super::invalid(error),
    };
    match state.account.request_support(caller.account, request.job, request.message).await {
        Ok(id) => (StatusCode::ACCEPTED, axum::Json(transgate_api::account::support_accepted(id))).into_response(),
        Err(error) => state.failure(error),
    }
}

async fn receipt(State(state): State<Arc<AccountHttp>>, headers: HeaderMap) -> Response {
    let account = match state.profiles.receipt_account(&headers).await {
        Ok(Some(account)) => account,
        Ok(None) => return missing_receipt(),
        Err(error) => return super::authentication_error(error, Denied::Json).into_response(),
    };
    match state.privacy.erasure_status(account).await {
        Ok(receipt) => super::ok_json(StatusCode::OK, &receipt),
        Err(JobsError::NotFound) => missing_receipt(),
        Err(error) => state.failure(error),
    }
}

fn missing_receipt() -> Response {
    super::error(StatusCode::NOT_FOUND, "not_found", "Account erasure request not found")
}
